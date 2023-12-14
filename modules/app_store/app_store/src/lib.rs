use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::{HashMap, HashSet};
use uqbar_process_lib::kernel_types as kt;
use uqbar_process_lib::uqbar::process::standard as wit;
use uqbar_process_lib::{
    get_capability, get_payload, get_typed_state, grant_messaging, println, receive, set_state,
    share_capability, Address, Message, NodeId, PackageId, ProcessId, Request, Response,
};

wit_bindgen::generate!({
    path: "../../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

#[allow(dead_code)]
mod ft_worker_lib;
use ft_worker_lib::{
    spawn_receive_transfer, spawn_transfer, FTWorkerCommand, FTWorkerResult, FileTransferContext,
};

struct Component;

/// Uqbar App Store:
/// acts as both a local package manager and a protocol to share packages across the network.
/// packages are apps; apps are packages. we use an onchain app listing contract to determine
/// what apps are available to download and what node(s) to download them from.
///
/// once we know that list, we can request a package from a node and download it locally.
/// (we can also manually download an "untracked" package if we know its name and distributor node)
/// packages that are downloaded can then be installed!
///
/// installed packages can be managed:
/// - given permissions (necessary to complete install)
/// - uninstalled / suspended
/// - deleted ("undownloaded")
/// - set to automatically update if a new version is available (on by default)

//
// app store types
//

/// this process's saved state
#[derive(Debug, Serialize, Deserialize)]
struct State {
    pub packages: HashMap<PackageId, PackageState>,
    pub requested_packages: HashSet<PackageId>,
}

/// state of an individual package we have downloaded
#[derive(Debug, Serialize, Deserialize)]
struct PackageState {
    pub mirrored_from: NodeId,
    pub listing_data: PackageListing,
    pub mirroring: bool,   // are we serving this package to others?
    pub auto_update: bool, // if we get a listing data update, will we try to download it?
}

/// just a sketch of what we might get from chain
#[derive(Debug, Serialize, Deserialize)]
struct PackageListing {
    pub name: String,
    pub publisher: NodeId,
    pub description: Option<String>,
    pub website: Option<String>,
    pub version: kt::PackageVersion,
    pub version_hash: String, // sha256 hash of the package zip or whatever
}

//
// app store API
//

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)] // untagged as a meta-type for all requests
pub enum Req {
    LocalRequest(LocalRequest),
    RemoteRequest(RemoteRequest),
    FTWorkerCommand(FTWorkerCommand),
    FTWorkerResult(FTWorkerResult),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)] // untagged as a meta-type for all responses
pub enum Resp {
    RemoteResponse(RemoteResponse),
    FTWorkerResult(FTWorkerResult),
    // note that we do not need to ourselves handle local responses, as
    // those are given to others rather than received.
    NewPackageResponse(NewPackageResponse),
    DownloadResponse(DownloadResponse),
    InstallResponse(InstallResponse),
}

/// Local requests take this form.
#[derive(Debug, Serialize, Deserialize)]
pub enum LocalRequest {
    /// expects a zipped package as payload: create a new package from it
    /// if requested, will return a NewPackageResponse indicating success/failure
    NewPackage {
        package: PackageId,
        mirror: bool, // sets whether we will mirror this package
    },
    /// no payload; try to download a package from a specified node
    /// if requested, will return a DownloadResponse indicating success/failure
    Download {
        package: PackageId,
        install_from: NodeId,
    },
    /// no payload; select a downloaded package and install it
    /// if requested, will return an InstallResponse indicating success/failure
    Install(PackageId),
    /// no payload; select an installed package and uninstall it
    /// no response will be given
    Uninstall(PackageId),
    /// no payload; select a downloaded package and delete it
    /// no response will be given
    Delete(PackageId),
}

/// Remote requests, those sent between instantiations of this process
/// on different nodes, take this form.
#[derive(Debug, Serialize, Deserialize)]
pub enum RemoteRequest {
    /// no payload; request a package from a node
    /// remote node must return RemoteResponse::DownloadApproved,
    /// at which point requester can expect a FTWorkerRequest::Receive
    Download(PackageId),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum RemoteResponse {
    DownloadApproved,
    DownloadDenied, // TODO expand on why
}

// TODO for all: expand these to elucidate why something failed
// these are locally-given responses to local requests

#[derive(Debug, Serialize, Deserialize)]
pub enum NewPackageResponse {
    Success,
    Failure,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum DownloadResponse {
    Started,
    Failure,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum InstallResponse {
    Success,
    Failure,
}

impl Guest for Component {
    fn init(our: String) {
        let our = Address::from_str(&our).unwrap();
        // begin by granting messaging capabilities to http_server and terminal,
        // so that they can send us requests.
        grant_messaging(
            &our,
            vec![
                ProcessId::new(Some("http_server"), "sys", "uqbar"),
                ProcessId::new(Some("terminal"), "terminal", "uqbar"),
                ProcessId::new(Some("vfs"), "sys", "uqbar"),
            ],
        );
        println!("{}: start", our.process);

        // load in our saved state or initalize a new one if none exists
        let mut state =
            get_typed_state(|bytes| Ok(bincode::deserialize(bytes)?)).unwrap_or(State {
                packages: HashMap::new(),
                requested_packages: HashSet::new(),
            });

        // active the main messaging loop: handle requests and responses
        loop {
            let (source, message) = match receive() {
                Ok((source, message)) => (source, message),
                Err((error, _context)) => {
                    // TODO handle net errors more usefully based on their context
                    println!("net error: {:?}", error.kind);
                    continue;
                }
            };
            match handle_message(&our, &source, &mut state, &message) {
                Ok(()) => {}
                Err(e) => println!("app-store: error handling message: {:?}", e),
            }
        }
    }
}

fn handle_message(
    our: &Address,
    source: &Address,
    mut state: &mut State,
    message: &Message,
) -> anyhow::Result<()> {
    match message {
        Message::Request(req) => {
            match &serde_json::from_slice::<Req>(&req.ipc) {
                Ok(Req::LocalRequest(local_request)) => {
                    match handle_local_request(&our, &source, local_request, &mut state) {
                        Ok(None) => return Ok(()),
                        Ok(Some(resp)) => {
                            if req.expects_response.is_some() {
                                Response::new().ipc(serde_json::to_vec(&resp)?).send()?;
                            }
                        }
                        Err(err) => {
                            println!("app-store: local request error: {:?}", err);
                        }
                    }
                }
                Ok(Req::RemoteRequest(remote_request)) => {
                    match handle_remote_request(&our, &source, remote_request, &mut state) {
                        Ok(None) => return Ok(()),
                        Ok(Some(resp)) => {
                            if req.expects_response.is_some() {
                                Response::new().ipc(serde_json::to_vec(&resp)?).send()?;
                            }
                        }
                        Err(err) => {
                            println!("app-store: remote request error: {:?}", err);
                        }
                    }
                }
                Ok(Req::FTWorkerResult(FTWorkerResult::ReceiveSuccess(name))) => {
                    // do with file what you'd like here
                    println!("file_transfer: successfully received {:?}", name);
                    // remove leading / and .zip from file name to get package ID
                    let package_id = match PackageId::from_str(name[1..].trim_end_matches(".zip")) {
                        Ok(package_id) => package_id,
                        Err(e) => {
                            println!("app store: bad package filename: {}", name);
                            return Err(anyhow::anyhow!(e));
                        }
                    };
                    if state.requested_packages.remove(&package_id) {
                        // auto-take zip from payload and request ourself with New
                        Request::new()
                            .target(our.clone())
                            .inherit(true)
                            .ipc(serde_json::to_vec(&Req::LocalRequest(
                                LocalRequest::NewPackage {
                                    package: package_id,
                                    mirror: true,
                                },
                            ))?)
                            .send()?;
                    }
                }
                Ok(Req::FTWorkerCommand(_)) => {
                    spawn_receive_transfer(&our, &req.ipc);
                }
                e => {
                    return Err(anyhow::anyhow!(
                        "app store bad request: {:?}, error {:?}",
                        req.ipc,
                        e
                    ))
                }
            }
        }
        Message::Response((response, context)) => {
            match &serde_json::from_slice::<Resp>(&response.ipc) {
                Ok(Resp::RemoteResponse(remote_response)) => match remote_response {
                    RemoteResponse::DownloadApproved => {
                        println!("app store: download approved, should be starting");
                    }
                    RemoteResponse::DownloadDenied => {
                        println!("app store: could not download package from that node!");
                    }
                },
                Ok(Resp::FTWorkerResult(ft_worker_result)) => {
                    let Ok(context) =
                        serde_json::from_slice::<FileTransferContext>(&context.as_ref().unwrap())
                    else {
                        return Err(anyhow::anyhow!("file_transfer: got weird local request"));
                    };
                    match ft_worker_result {
                        FTWorkerResult::SendSuccess => {
                            println!(
                                "file_transfer: successfully shared app {} in {:.4}s",
                                context.file_name,
                                std::time::SystemTime::now()
                                    .duration_since(context.start_time)
                                    .unwrap()
                                    .as_secs_f64(),
                            );
                        }
                        e => return Err(anyhow::anyhow!("file_transfer: {:?}", e)),
                    }
                }
                _ => return Err(anyhow::anyhow!("bad response from file transfer worker")),
            }
        }
    }
    Ok(())
}

fn handle_local_request(
    our: &Address,
    source: &Address,
    request: &LocalRequest,
    state: &mut State,
) -> anyhow::Result<Option<Resp>> {
    if our.node != source.node {
        return Err(anyhow::anyhow!("local request from non-local node"));
    }
    match request {
        LocalRequest::NewPackage { package, mirror } => {
            let Some(mut payload) = get_payload() else {
                return Err(anyhow::anyhow!("no payload"));
            };

            Request::new()
                .target(Address::from_str("our@vfs:sys:uqbar")?)
                .ipc(serde_json::to_vec(&kt::VfsRequest {
                    drive: package.to_string(),
                    action: kt::VfsAction::New,
                })?)
                .send_and_await_response(5)??;

            // produce the version hash for this new package
            let mut hasher = sha2::Sha256::new();
            hasher.update(&payload.bytes);
            let version_hash = format!("{:x}", hasher.finalize());

            // add zip bytes
            payload.mime = Some("application/zip".to_string());
            Request::new()
                .target(Address::from_str("our@vfs:sys:uqbar")?)
                .ipc(serde_json::to_vec(&kt::VfsRequest {
                    drive: package.to_string(),
                    action: kt::VfsAction::Add {
                        full_path: package.to_string(),
                        entry_type: kt::AddEntryType::ZipArchive,
                    },
                })?)
                .payload(payload.clone())
                .send_and_await_response(5)??;

            // save the zip file itself in VFS for sharing with other nodes
            // call it <package>.zip
            Request::new()
                .target(Address::from_str("our@vfs:sys:uqbar")?)
                .inherit(true)
                .ipc(serde_json::to_vec(&kt::VfsRequest {
                    drive: package.to_string(),
                    action: kt::VfsAction::Add {
                        full_path: format!("/{}.zip", package.to_string()),
                        entry_type: kt::AddEntryType::NewFile,
                    },
                })?)
                .payload(payload)
                .send_and_await_response(5)??;
            Request::new()
                .target(Address::from_str("our@vfs:sys:uqbar")?)
                .ipc(serde_json::to_vec(&kt::VfsRequest {
                    drive: package.to_string(),
                    action: kt::VfsAction::GetEntry("/metadata.json".into()),
                })?)
                .send_and_await_response(5)??;
            let Some(payload) = get_payload() else {
                return Err(anyhow::anyhow!("no metadata found!"));
            };
            println!("got metadata 1");
            let metadata = String::from_utf8(payload.bytes)?;
            println!("from bytes");
            let metadata = serde_json::from_str::<kt::PackageMetadata>(&metadata)?;
            println!("parsed metadata");

            let listing_data = PackageListing {
                name: metadata.package,
                publisher: our.node.clone(),
                description: metadata.description,
                website: metadata.website,
                version: metadata.version,
                version_hash,
            };
            let package_state = PackageState {
                mirrored_from: our.node.clone(),
                listing_data,
                mirroring: *mirror,
                auto_update: true,
            };
            state.packages.insert(package.clone(), package_state);
            crate::set_state(&bincode::serialize(state)?);
            Ok(Some(Resp::NewPackageResponse(NewPackageResponse::Success)))
        }
        LocalRequest::Download {
            package,
            install_from,
        } => Ok(Some(Resp::DownloadResponse(
            match Request::new()
                .target(Address::new(install_from, our.process.clone()))
                .inherit(true)
                .ipc(serde_json::to_vec(&RemoteRequest::Download(
                    package.clone(),
                ))?)
                .send_and_await_response(5)
            {
                Ok(Ok((_source, Message::Response((resp, _context))))) => {
                    let resp = serde_json::from_slice::<Resp>(&resp.ipc)?;
                    match resp {
                        Resp::RemoteResponse(RemoteResponse::DownloadApproved) => {
                            state.requested_packages.insert(package.clone());
                            crate::set_state(&bincode::serialize(&state)?);
                            DownloadResponse::Started
                        }
                        _ => DownloadResponse::Failure,
                    }
                }
                _ => DownloadResponse::Failure,
            },
        ))),
        LocalRequest::Install(package) => {
            Request::new()
                .target(Address::from_str("our@vfs:sys:uqbar")?)
                .ipc(serde_json::to_vec(&kt::VfsRequest {
                    drive: package.to_string(),
                    action: kt::VfsAction::GetEntry("/manifest.json".into()),
                })?)
                .send_and_await_response(5)??;
            let Some(payload) = get_payload() else {
                return Err(anyhow::anyhow!("no payload"));
            };
            let manifest = String::from_utf8(payload.bytes)?;
            let manifest = serde_json::from_str::<Vec<kt::PackageManifestEntry>>(&manifest)?;
            // always grant read/write to their drive, which we created for them
            let Some(read_cap) = get_capability(
                &Address::new(&our.node, ("vfs", "sys", "uqbar")),
                &serde_json::to_string(&serde_json::json!({
                    "kind": "read",
                    "drive": package.to_string(),
                }))?,
            ) else {
                return Err(anyhow::anyhow!("app-store: no read cap"));
            };
            let Some(write_cap) = get_capability(
                &Address::new(&our.node, ("vfs", "sys", "uqbar")),
                &serde_json::to_string(&serde_json::json!({
                    "kind": "write",
                    "drive": package.to_string(),
                }))?,
            ) else {
                return Err(anyhow::anyhow!("app-store: no write cap"));
            };
            let Some(networking_cap) = get_capability(
                &Address::new(&our.node, ("kernel", "sys", "uqbar")),
                &"\"network\"".to_string(),
            ) else {
                return Err(anyhow::anyhow!("app-store: no net cap"));
            };
            // first, for each process in manifest, initialize it
            // then, once all have been initialized, grant them requested caps
            // and finally start them.
            for entry in &manifest {
                let path = if entry.process_wasm_path.starts_with("/") {
                    entry.process_wasm_path.clone()
                } else {
                    format!("/{}", entry.process_wasm_path)
                };
                let (_, hash_response) = Request::new()
                    .target(Address::from_str("our@vfs:sys:uqbar")?)
                    .ipc(serde_json::to_vec(&kt::VfsRequest {
                        drive: package.to_string(),
                        action: kt::VfsAction::GetHash(path.clone()),
                    })?)
                    .send_and_await_response(5)??;

                let Message::Response((wit::Response { ipc, .. }, _)) = hash_response else {
                    return Err(anyhow::anyhow!("bad vfs response"));
                };
                let kt::VfsResponse::GetHash(Some(hash)) = serde_json::from_slice(&ipc)? else {
                    return Err(anyhow::anyhow!("no hash in vfs"));
                };
                // build initial caps
                let mut initial_capabilities: HashSet<kt::SignedCapability> = HashSet::new();
                if entry.request_networking {
                    initial_capabilities.insert(kt::de_wit_signed_capability(networking_cap.clone()));
                }
                initial_capabilities.insert(kt::de_wit_signed_capability(read_cap.clone()));
                initial_capabilities.insert(kt::de_wit_signed_capability(write_cap.clone()));
                let process_id = format!("{}:{}", entry.process_name, package.to_string());
                let Ok(parsed_new_process_id) = ProcessId::from_str(&process_id) else {
                    return Err(anyhow::anyhow!("app-store: invalid process id!"));
                };
                // kill process if it already exists
                Request::new()
                    .target(Address::from_str("our@kernel:sys:uqbar")?)
                    .ipc(serde_json::to_vec(&kt::KernelCommand::KillProcess(
                        parsed_new_process_id.clone(),
                    ))?)
                    .send()?;

                let (_, _bytes_response) = Request::new()
                    .target(Address::from_str("our@vfs:sys:uqbar")?)
                    .ipc(serde_json::to_vec(&kt::VfsRequest {
                        drive: package.to_string(),
                        action: kt::VfsAction::GetEntry(path),
                    })?)
                    .send_and_await_response(5)??;
                Request::new()
                    .target(Address::from_str("our@kernel:sys:uqbar")?)
                    .ipc(serde_json::to_vec(&kt::KernelCommand::InitializeProcess {
                        id: parsed_new_process_id,
                        wasm_bytes_handle: hash,
                        on_panic: entry.on_panic.clone(),
                        initial_capabilities,
                        public: entry.public,
                    })?)
                    .inherit(true)
                    .send_and_await_response(5)?;
            }
            for entry in &manifest {
                let process_id = ProcessId::new(
                    Some(&entry.process_name),
                    package.package(),
                    package.publisher(),
                );
                if let Some(to_request) = &entry.request_messaging {
                    for process_name in to_request {
                        let Ok(parsed_process_id) = ProcessId::from_str(&process_name) else {
                            // TODO handle arbitrary caps here
                            continue;
                        };
                        let Some(messaging_cap) = get_capability(
                            &Address {
                                node: our.node.clone(),
                                process: parsed_process_id.clone(),
                            },
                            &"\"messaging\"".into(),
                        ) else {
                            println!("app-store: no cap for {} to give away!", process_name);
                            continue;
                        };
                        share_capability(&process_id, &messaging_cap);
                    }
                }
                if let Some(to_grant) = &entry.grant_messaging {
                    let Some(messaging_cap) = get_capability(
                        &Address {
                            node: our.node.clone(),
                            process: process_id.clone(),
                        },
                        &"\"messaging\"".into(),
                    ) else {
                        println!("app-store: no cap for {} to give away!", process_id);
                        continue;
                    };
                    for process_name in to_grant {
                        let Ok(parsed_process_id) = ProcessId::from_str(&process_name) else {
                            // TODO handle arbitrary caps here
                            continue;
                        };
                        share_capability(&parsed_process_id, &messaging_cap);
                    }
                }
                Request::new()
                    .target(Address::from_str("our@kernel:sys:uqbar")?)
                    .ipc(serde_json::to_vec(&kt::KernelCommand::RunProcess(
                        process_id,
                    ))?)
                    .send_and_await_response(5)?;
            }
            Ok(Some(Resp::InstallResponse(InstallResponse::Success)))
        }
        LocalRequest::Uninstall(_package) => {
            // TODO
            Ok(None)
        }
        LocalRequest::Delete(_package) => {
            // TODO
            Ok(None)
        }
    }
}

fn handle_remote_request(
    our: &Address,
    source: &Address,
    request: &RemoteRequest,
    state: &mut State,
) -> anyhow::Result<Option<Resp>> {
    match request {
        RemoteRequest::Download(package) => {
            let Some(package_state) = state.packages.get(&package) else {
                return Ok(Some(Resp::RemoteResponse(RemoteResponse::DownloadDenied)));
            };
            if !package_state.mirroring {
                return Ok(Some(Resp::RemoteResponse(RemoteResponse::DownloadDenied)));
            }
            // get the .zip from VFS and attach as payload to response
            let file_name = format!("/{}.zip", package.to_string());
            Request::new()
                .target(Address::from_str("our@vfs:sys:uqbar")?)
                .ipc(serde_json::to_vec(&kt::VfsRequest {
                    drive: package.to_string(),
                    action: kt::VfsAction::GetEntry(file_name.clone()),
                })?)
                .send_and_await_response(5)?;
            // transfer will inherit the payload bytes we receive from VFS
            spawn_transfer(&our, &file_name, None, &source);
            Ok(Some(Resp::RemoteResponse(RemoteResponse::DownloadApproved)))
        }
    }
}
