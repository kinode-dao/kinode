use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::{HashMap, HashSet};
use uqbar_process_lib::kernel_types as kt;
use uqbar_process_lib::{
    await_message, get_capability, get_payload, get_typed_state, println, grant_capabilities,
    set_state, Address, Message, NodeId, PackageId, ProcessId, Request, Response, Capability,
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
        // TODO couldnt you put this in the manifest? or does the manifest not apply to the app_store?
        grant_capabilities(
            &ProcessId::new(Some("http_server"), "sys", "uqbar"),
            &vec![Capability {
                    issuer: our.clone(),
                    params: "\"messaging\"".to_string(),
            }]
        );
        grant_capabilities(
            &ProcessId::new(Some("terminal"), "terminal", "uqbar"),
            &vec![Capability {
                    issuer: our.clone(),
                    params: "\"messaging\"".to_string(),
            }]
        );
        grant_capabilities(
            &ProcessId::new(Some("vfs"), "sys", "uqbar"),
            &vec![Capability {
                    issuer: our.clone(),
                    params: "\"messaging\"".to_string(),
            }]
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
            match await_message() {
                Err(send_error) => {
                    println!("{our}: got network error: {send_error:?}");
                    continue;
                }
                Ok(message) => match handle_message(&our, &mut state, &message) {
                    Ok(()) => {}
                    Err(e) => println!("app-store: error handling message: {:?}", e),
                },
            }
        }
    }
}

fn handle_message(our: &Address, mut state: &mut State, message: &Message) -> anyhow::Result<()> {
    match message {
        Message::Request {
            source,
            expects_response,
            ipc,
            ..
        } => {
            match &serde_json::from_slice::<Req>(&ipc) {
                Ok(Req::LocalRequest(local_request)) => {
                    match handle_local_request(&our, &source, local_request, &mut state) {
                        Ok(None) => return Ok(()),
                        Ok(Some(resp)) => {
                            if expects_response.is_some() {
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
                            if expects_response.is_some() {
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
                    spawn_receive_transfer(&our, &ipc);
                }
                e => {
                    return Err(anyhow::anyhow!(
                        "app store bad request: {:?}, error {:?}",
                        ipc,
                        e
                    ))
                }
            }
        }
        Message::Response { ipc, context, .. } => match &serde_json::from_slice::<Resp>(&ipc) {
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
        },
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
            let drive = format!("/{}/pkg", package);

            Request::new()
                .target(Address::from_str("our@vfs:sys:uqbar")?)
                .ipc(serde_json::to_vec(&kt::VfsRequest {
                    path: drive.clone(),
                    action: kt::VfsAction::CreateDrive,
                })?)
                .send_and_await_response(5)?
                .unwrap();

            // produce the version hash for this new package
            let mut hasher = sha2::Sha256::new();
            hasher.update(&payload.bytes);
            let version_hash = format!("{:x}", hasher.finalize());

            // add zip bytes
            payload.mime = Some("application/zip".to_string());
            let response = Request::new()
                .target(Address::from_str("our@vfs:sys:uqbar")?)
                .ipc(serde_json::to_vec(&kt::VfsRequest {
                    path: drive.clone(),
                    action: kt::VfsAction::AddZip,
                })?)
                .payload(payload.clone())
                .send_and_await_response(5)?.unwrap();
            let Message::Response { ipc: ref vfs_ipc, .. } = response else {
                panic!("app_store: send_and_await_response must return Response");
            };
            let vfs_ipc = serde_json::from_slice::<serde_json::Value>(vfs_ipc)?;
            if vfs_ipc == serde_json::json!({"Err": "NoCap"}) {
                return Err(anyhow::anyhow!("cannot add NewPackage: do not have capability to access vfs"));
            }

            // save the zip file itself in VFS for sharing with other nodes
            // call it <package>.zip
            let zip_path = format!("{}/{}.zip", drive.clone(), package);
            Request::new()
                .target(Address::from_str("our@vfs:sys:uqbar")?)
                .inherit(true)
                .ipc(serde_json::to_vec(&kt::VfsRequest {
                    path: zip_path,
                    action: kt::VfsAction::ReWrite,
                })?)
                .payload(payload)
                .send_and_await_response(5)?
                .unwrap();
            let metadata_path = format!("{}/metadata.json", drive.clone());

            Request::new()
                .target(Address::from_str("our@vfs:sys:uqbar")?)
                .ipc(serde_json::to_vec(&kt::VfsRequest {
                    path: metadata_path,
                    action: kt::VfsAction::Read,
                })?)
                .send_and_await_response(5)?
                .unwrap();
            let Some(payload) = get_payload() else {
                return Err(anyhow::anyhow!("no metadata found!"));
            };

            let metadata = String::from_utf8(payload.bytes)?;
            let metadata = serde_json::from_str::<kt::PackageMetadata>(&metadata)?;

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
                Ok(Ok(Message::Response { ipc, .. })) => {
                    let resp = serde_json::from_slice::<Resp>(&ipc)?;
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
            let drive_path = format!("/{}/pkg", package);
            Request::new()
                .target(Address::from_str("our@vfs:sys:uqbar")?)
                .ipc(serde_json::to_vec(&kt::VfsRequest {
                    path: format!("{}/manifest.json", drive_path),
                    action: kt::VfsAction::Read,
                })?)
                .send_and_await_response(5)?
                .unwrap();
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
                    "drive": drive_path,
                }))?,
            ) else {
                return Err(anyhow::anyhow!("app-store: no read cap"));
            };
            let Some(write_cap) = get_capability(
                &Address::new(&our.node, ("vfs", "sys", "uqbar")),
                &serde_json::to_string(&serde_json::json!({
                    "kind": "write",
                    "drive": drive_path,
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
                let wasm_path = if entry.process_wasm_path.starts_with("/") {
                    entry.process_wasm_path.clone()
                } else {
                    format!("/{}", entry.process_wasm_path)
                };
                let wasm_path = format!("{}{}", drive_path, wasm_path);
                // build initial caps
                let mut initial_capabilities: HashSet<kt::Capability> = HashSet::new();
                if entry.request_networking {
                    initial_capabilities
                        .insert(kt::de_wit_capability(networking_cap.clone()));
                }
                initial_capabilities.insert(kt::de_wit_capability(read_cap.clone()));
                initial_capabilities.insert(kt::de_wit_capability(write_cap.clone()));

                if let Some(to_request) = &entry.request_messaging {
                    for value in to_request {
                        let mut capability = None;
                        match value {
                            serde_json::Value::String(process_name) => {
                                if let Ok(parsed_process_id) = ProcessId::from_str(process_name) {
                                    capability = get_capability(
                                        &Address {
                                            node: our.node.clone(),
                                            process: parsed_process_id.clone(),
                                        },
                                        "\"messaging\"".into(),
                                    );
                                }
                            }
                            serde_json::Value::Object(map) => {
                                if let Some(process_name) = map.get("process") {
                                    if let Ok(parsed_process_id) =
                                        ProcessId::from_str(&process_name.to_string())
                                    {
                                        if let Some(params) = map.get("params") {
                                            capability = get_capability(
                                                &Address {
                                                    node: our.node.clone(),
                                                    process: parsed_process_id.clone(),
                                                },
                                                &params.to_string(),
                                            );
                                        }
                                    }
                                }
                            }
                            _ => {
                                continue;
                            }
                        }
                        if let Some(cap) = capability {
                            initial_capabilities.insert(kt::de_wit_capability(cap));
                            // share_capability(&process_id, &cap);
                        } else {
                            println!("app-store: no cap: {}, for {} to request!", value.to_string(), package);
                        }
                    }
                }

                let process_id = format!("{}:{}", entry.process_name, package);
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

                let _bytes_response = Request::new()
                    .target(Address::from_str("our@vfs:sys:uqbar")?)
                    .ipc(serde_json::to_vec(&kt::VfsRequest {
                        path: wasm_path.clone(),
                        action: kt::VfsAction::Read,
                    })?)
                    .send_and_await_response(5)?
                    .unwrap();
                Request::new()
                    .target(Address::from_str("our@kernel:sys:uqbar")?)
                    .ipc(serde_json::to_vec(&kt::KernelCommand::InitializeProcess {
                        id: parsed_new_process_id,
                        wasm_bytes_handle: wasm_path,
                        on_exit: entry.on_exit.clone(),
                        initial_capabilities,
                        public: entry.public,
                    })?)
                    .inherit(true)
                    .send_and_await_response(5)?
                    .unwrap();
            }
            for entry in &manifest {
                let process_id = ProcessId::new(
                    Some(&entry.process_name),
                    package.package(),
                    package.publisher(),
                );
                if let Some(to_grant) = &entry.grant_messaging {
                    for value in to_grant {
                        let mut capability = None;
                        let mut to_process = None;
                        match value {
                            serde_json::Value::String(process_name) => {
                                if let Ok(parsed_process_id) = ProcessId::from_str(process_name) {
                                    capability = Some(Capability {
                                        issuer: Address {
                                            node: our.node.clone(),
                                            process: process_id.clone(),
                                        },
                                        params: "\"messaging\"".into(),
                                    }) ;
                                    to_process = Some(parsed_process_id);
                                }
                            }
                            serde_json::Value::Object(map) => {
                                if let Some(process_name) = map.get("process") {
                                    if let Ok(parsed_process_id) =
                                        ProcessId::from_str(&process_name.to_string())
                                    {
                                        if let Some(params) = map.get("params") {
                                            capability = Some(Capability {
                                                issuer: Address {
                                                    node: our.node.clone(),
                                                    process: process_id.clone(),
                                                },
                                                params: params.to_string(),
                                            });
                                            to_process = Some(parsed_process_id);
                                        }
                                    }
                                }
                            }
                            _ => {
                                continue;
                            }
                        }
                        // TODO: how do I give app_store the root capability?
                        grant_capabilities(&to_process.unwrap(), &vec![capability.unwrap()]);
                    }
                }
                Request::new()
                    .target(Address::from_str("our@kernel:sys:uqbar")?)
                    .ipc(serde_json::to_vec(&kt::KernelCommand::RunProcess(
                        process_id,
                    ))?)
                    .send_and_await_response(5)?
                    .unwrap();
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
            let drive_name = format!("/{}/pkg", package);
            let file_path = format!("/{}.zip", drive_name);
            Request::new()
                .target(Address::from_str("our@vfs:sys:uqbar")?)
                .ipc(serde_json::to_vec(&kt::VfsRequest {
                    path: file_path,
                    action: kt::VfsAction::Read,
                })?)
                .send_and_await_response(5)?
                .unwrap();
            // transfer will inherit the payload bytes we receive from VFS
            let file_name = format!("/{}.zip", package);
            spawn_transfer(&our, &file_name, None, &source);
            Ok(Some(Resp::RemoteResponse(RemoteResponse::DownloadApproved)))
        }
    }
}
