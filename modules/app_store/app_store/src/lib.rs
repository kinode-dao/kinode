cargo_component_bindings::generate!();
use bindings::{
    component::uq_process::types::*, get_capability, get_payload, print_to_terminal, receive,
    send_request, send_response, Guest,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};
use std::collections::{HashMap, HashSet};

#[allow(dead_code)]
mod kernel_types;
use kernel_types as kt;
use kernel_types::{PackageManifestEntry, PackageMetadata, PackageVersion};

#[allow(dead_code)]
mod process_lib;
use process_lib::PackageId;

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
    pub requested_packages: HashMap<PackageId, NodeId>,
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
    pub version: PackageVersion,
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

//
// app store init()
//

impl Guest for Component {
    fn init(our: Address) {
        assert_eq!(our.process, "main:app_store:uqbar");

        // begin by granting messaging capabilities to http_server and terminal,
        // so that they can send us requests.
        process_lib::grant_messaging(
            &our,
            &Vec::from([
                ProcessId::from_str("http_server:sys:uqbar").unwrap(),
                ProcessId::from_str("terminal:terminal:uqbar").unwrap(),
            ]),
        );
        print_to_terminal(0, &format!("app_store main proc: start"));

        // load in our saved state or initalize a new one if none exists
        let mut state = process_lib::get_state::<State>().unwrap_or(State {
            packages: HashMap::new(),
            requested_packages: HashMap::new(),
        });

        // active the main messaging loop: handle requests and responses
        loop {
            let (source, message) = match receive() {
                Ok((source, message)) => (source, message),
                Err((error, _context)) => {
                    // TODO handle net errors more usefully based on their context
                    print_to_terminal(0, &format!("net error: {:?}", error.kind));
                    continue;
                }
            };
            print_to_terminal(
                0,
                &format!("app-store: got message from {}: {:?}", source.to_string(), message),
            );
            match message {
                Message::Request(req) => {
                    let Some(ref ipc) = req.ipc else {
                        continue;
                    };
                    match &serde_json::from_str::<Req>(ipc) {
                        Ok(Req::LocalRequest(local_request)) => {
                            match handle_local_request(&our, &source, local_request, &mut state) {
                                Ok(None) => continue,
                                Ok(Some(resp)) => {
                                    if req.expects_response.is_some() {
                                        send_response(
                                            &Response {
                                                inherit: false,
                                                ipc: Some(serde_json::to_string(&resp).unwrap()),
                                                metadata: None,
                                            },
                                            None,
                                        );
                                    }
                                }
                                Err(err) => {
                                    print_to_terminal(
                                        0,
                                        &format!("app-store: local request error: {:?}", err),
                                    );
                                }
                            }
                        }
                        Ok(Req::RemoteRequest(remote_request)) => {
                            match handle_remote_request(&our, &source, remote_request, &mut state) {
                                Ok(None) => continue,
                                Ok(Some(resp)) => {
                                    if req.expects_response.is_some() {
                                        send_response(
                                            &Response {
                                                inherit: false,
                                                ipc: Some(serde_json::to_string(&resp).unwrap()),
                                                metadata: None,
                                            },
                                            None,
                                        );
                                    }
                                }
                                Err(err) => {
                                    print_to_terminal(
                                        0,
                                        &format!("app-store: remote request error: {:?}", err),
                                    );
                                }
                            }
                        }
                        Ok(Req::FTWorkerCommand(_)) => {
                            spawn_receive_transfer(&our, ipc);
                        }
                        e => {
                            print_to_terminal(
                                0,
                                &format!("app store bad request: {}, error {:?}", ipc, e),
                            );
                            continue;
                        }
                    }
                }
                Message::Response((response, context)) => {
                    let Some(ref ipc) = response.ipc else {
                        continue;
                    };
                    match &serde_json::from_str::<Resp>(ipc) {
                        Ok(Resp::RemoteResponse(remote_response)) => match remote_response {
                            RemoteResponse::DownloadApproved => {
                                print_to_terminal(
                                    0,
                                    "app store: download approved, should be starting",
                                );
                            }
                            RemoteResponse::DownloadDenied => {
                                print_to_terminal(
                                    0,
                                    "app store: could not download package from that node!",
                                );
                            }
                        },
                        Ok(Resp::FTWorkerResult(ft_worker_result)) => {
                            let Ok(context) = serde_json::from_str::<FileTransferContext>(&context.unwrap_or_default()) else {
                                print_to_terminal(0, "file_transfer: got weird local request");
                                continue;
                            };
                            match ft_worker_result {
                                FTWorkerResult::SendSuccess => {
                                    print_to_terminal(
                                        0,
                                        &format!(
                                            "file_transfer: successfully shared app {} in {:.4}s",
                                            context.file_name,
                                            std::time::SystemTime::now()
                                                .duration_since(context.start_time)
                                                .unwrap()
                                                .as_secs_f64(),
                                        ),
                                    );
                                }
                                FTWorkerResult::ReceiveSuccess(name) => {
                                    // do with file what you'd like here
                                    print_to_terminal(
                                        0,
                                        &format!("file_transfer: successfully received {:?}", name,),
                                    );
                                    // remove .zip from name
                                    let package_id =
                                        match PackageId::from_str(name.trim_end_matches(".zip")) {
                                            Ok(package_id) => package_id,
                                            Err(_) => {
                                                print_to_terminal(
                                                    0,
                                                    &format!(
                                                        "app store: bad package filename: {}",
                                                        name
                                                    ),
                                                );
                                                continue;
                                            }
                                        };
                                    if let Some(install_from) =
                                        state.requested_packages.remove(&package_id)
                                    {
                                        if install_from == source.node {
                                            // auto-take zip from payload and request ourself with New
                                            let _ = send_request(
                                                &our,
                                                &Request {
                                                    inherit: true, // will inherit payload!
                                                    expects_response: None,
                                                    ipc: Some(
                                                        serde_json::to_string(&Req::LocalRequest(
                                                            LocalRequest::NewPackage {
                                                                package: package_id,
                                                                mirror: true,
                                                            },
                                                        ))
                                                        .unwrap(),
                                                    ),
                                                    metadata: None,
                                                },
                                                None,
                                                None,
                                            );
                                        } else {
                                            print_to_terminal(
                                                0,
                                                &format!(
                                            "app-store: got install response from bad source: {}",
                                            install_from
                                        ),
                                            );
                                        }
                                    }
                                }
                                FTWorkerResult::Err(e) => {
                                    print_to_terminal(
                                        0,
                                        &format!("app store file transfer: error {:?}", e),
                                    );
                                }
                            }
                        }
                        e => {
                            print_to_terminal(
                                0,
                                &format!("app store bad response: {}, error {:?}", ipc, e),
                            );
                            continue;
                        }
                    }
                }
            }
        }
    }
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
            let vfs_address = Address {
                node: our.node.clone(),
                process: ProcessId::from_str("vfs:sys:uqbar")?,
            };

            // produce the version hash for this new package
            let mut hasher = sha2::Sha256::new();
            hasher.update(&payload.bytes);
            let version_hash = format!("{:x}", hasher.finalize());

            let _ = process_lib::send_and_await_response(
                &vfs_address,
                false,
                Some(serde_json::to_string(&kt::VfsRequest {
                    drive: package.to_string(),
                    action: kt::VfsAction::New,
                })?),
                None,
                None,
                5,
            )?;

            // add zip bytes
            payload.mime = Some("application/zip".to_string());
            let _ = process_lib::send_and_await_response(
                &vfs_address,
                true,
                Some(serde_json::to_string(&kt::VfsRequest {
                    drive: package.to_string(),
                    action: kt::VfsAction::Add {
                        full_path: package.to_string(),
                        entry_type: kt::AddEntryType::ZipArchive,
                    },
                })?),
                None,
                Some(&payload),
                5,
            )?;

            // save the zip file itself in VFS for sharing with other nodes
            // call it <package>.zip
            let _ = process_lib::send_and_await_response(
                &vfs_address,
                true,
                Some(serde_json::to_string(&kt::VfsRequest {
                    drive: package.to_string(),
                    action: kt::VfsAction::Add {
                        full_path: format!("/{}.zip", package.to_string()),
                        entry_type: kt::AddEntryType::NewFile,
                    },
                })?),
                None,
                Some(&payload),
                5,
            )?;

            let _ = process_lib::send_and_await_response(
                &vfs_address,
                false,
                Some(serde_json::to_string(&kt::VfsRequest {
                    drive: package.to_string(),
                    action: kt::VfsAction::GetEntry("/metadata.json".into()),
                })?),
                None,
                None,
                5,
            )?;
            let Some(payload) = get_payload() else {
                return Err(anyhow::anyhow!("no metadata payload"));
            };
            let metadata = String::from_utf8(payload.bytes)?;
            let metadata = serde_json::from_str::<PackageMetadata>(&metadata)?;

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
            process_lib::set_state::<State>(&state);
            Ok(Some(Resp::NewPackageResponse(NewPackageResponse::Success)))
        }
        LocalRequest::Download {
            package,
            install_from,
        } => Ok(Some(Resp::DownloadResponse(
            match process_lib::send_and_await_response(
                &Address {
                    node: install_from.clone(),
                    process: our.process.clone(),
                },
                true,
                Some(serde_json::to_string(&RemoteRequest::Download(
                    package.clone(),
                ))?),
                None,
                None,
                5,
            ) {
                Ok((_source, Message::Response((resp, _context)))) => {
                    let Some(ipc) = resp.ipc else {
                            return Err(anyhow::anyhow!("no ipc in response"))
                        };
                    let resp = serde_json::from_str::<Resp>(&ipc)?;
                    match resp {
                        Resp::RemoteResponse(RemoteResponse::DownloadApproved) => {
                            state
                                .requested_packages
                                .insert(package.clone(), install_from.to_string());
                            process_lib::set_state::<State>(&state);
                            DownloadResponse::Started
                        }
                        _ => DownloadResponse::Failure,
                    }
                }
                _ => DownloadResponse::Failure,
            },
        ))),
        LocalRequest::Install(package) => {
            let vfs_address = Address {
                node: our.node.clone(),
                process: ProcessId::from_str("vfs:sys:uqbar")?,
            };
            let _ = process_lib::send_and_await_response(
                &vfs_address,
                false,
                Some(serde_json::to_string(&kt::VfsRequest {
                    drive: package.to_string(),
                    action: kt::VfsAction::GetEntry("/manifest.json".into()),
                })?),
                None,
                None,
                5,
            )?;
            let Some(payload) = get_payload() else {
                return Err(anyhow::anyhow!("no payload"));
            };
            let manifest = String::from_utf8(payload.bytes)?;
            let manifest = serde_json::from_str::<Vec<PackageManifestEntry>>(&manifest)?;
            for entry in manifest {
                let path = if entry.process_wasm_path.starts_with("/") {
                    entry.process_wasm_path
                } else {
                    format!("/{}", entry.process_wasm_path)
                };

                let (_, hash_response) = process_lib::send_and_await_response(
                    &vfs_address,
                    false,
                    Some(serde_json::to_string(&kt::VfsRequest {
                        drive: package.to_string(),
                        action: kt::VfsAction::GetHash(path.clone()),
                    })?),
                    None,
                    None,
                    5,
                )?;

                let Message::Response((Response { ipc: Some(ipc), .. }, _)) = hash_response else {
                    return Err(anyhow::anyhow!("bad vfs response"));
                };
                let kt::VfsResponse::GetHash(Some(hash)) = serde_json::from_str(&ipc)? else {
                    return Err(anyhow::anyhow!("no hash in vfs"));
                };

                // build initial caps
                let mut initial_capabilities: HashSet<kt::SignedCapability> = HashSet::new();
                if entry.request_networking {
                    let Some(networking_cap) = get_capability(
                        &Address {
                            node: our.node.clone(),
                            process: ProcessId::from_str("kernel:sys:uqbar")?,
                        },
                        &"\"network\"".to_string(),
                    ) else {
                        return Err(anyhow::anyhow!("app-store: no net cap"));
                    };
                    initial_capabilities.insert(kt::de_wit_signed_capability(networking_cap));
                }
                let Some(read_cap) = get_capability(
                    &vfs_address.clone(),
                    &serde_json::to_string(&serde_json::json!({
                        "kind": "read",
                        "drive": package.to_string(),
                    }))?,
                ) else {
                    return Err(anyhow::anyhow!("app-store: no read cap"));
                };
                initial_capabilities.insert(kt::de_wit_signed_capability(read_cap));
                let Some(write_cap) = get_capability(
                    &vfs_address.clone(),
                    &serde_json::to_string(&serde_json::json!({
                        "kind": "write",
                        "drive": package.to_string(),
                    }))?,
                ) else {
                    return Err(anyhow::anyhow!("app-store: no write cap"));
                };
                initial_capabilities.insert(kt::de_wit_signed_capability(write_cap));

                for process_name in &entry.request_messaging {
                    let Ok(parsed_process_id) = ProcessId::from_str(&process_name) else {
                        // TODO handle arbitrary caps here
                        continue;
                    };
                    let Some(messaging_cap) = get_capability(
                        &Address {
                            node: our.node.clone(),
                            process: parsed_process_id.clone(),
                        },
                        &"\"messaging\"".into()
                    ) else {
                        print_to_terminal(0, &format!("app-store: no cap for {} to give away!", process_name));
                        continue;
                    };
                    initial_capabilities.insert(kt::de_wit_signed_capability(messaging_cap));
                }

                let process_id = format!("{}:{}", entry.process_name, package.to_string());
                let Ok(parsed_new_process_id) = ProcessId::from_str(&process_id) else {
                    return Err(anyhow::anyhow!("app-store: invalid process id!"));
                };
                let _ = process_lib::send_request(
                    &Address {
                        node: our.node.clone(),
                        process: ProcessId::from_str("kernel:sys:uqbar")?,
                    },
                    false,
                    Some(serde_json::to_string(&kt::KernelCommand::KillProcess(
                        kt::ProcessId::de_wit(parsed_new_process_id.clone()),
                    ))?),
                    None,
                    None,
                    None,
                );

                // kernel start process takes bytes as payload + wasm_bytes_handle...
                // reconsider perhaps
                let (_, _bytes_response) = process_lib::send_and_await_response(
                    &vfs_address,
                    false,
                    Some(serde_json::to_string(&kt::VfsRequest {
                        drive: package.to_string(),
                        action: kt::VfsAction::GetEntry(path),
                    })?),
                    None,
                    None,
                    5,
                )?;

                let Some(payload) = get_payload() else {
                    return Err(anyhow::anyhow!("no wasm bytes payload."));
                };

                let _ = process_lib::send_and_await_response(
                    &Address {
                        node: our.node.clone(),
                        process: ProcessId::from_str("kernel:sys:uqbar")?,
                    },
                    false,
                    Some(serde_json::to_string(&kt::KernelCommand::StartProcess {
                        id: kt::ProcessId::de_wit(parsed_new_process_id),
                        wasm_bytes_handle: hash,
                        on_panic: entry.on_panic,
                        initial_capabilities,
                        public: entry.public,
                    })?),
                    None,
                    Some(&payload),
                    5,
                )?;
            }
            Ok(Some(Resp::InstallResponse(InstallResponse::Success)))
        }
        LocalRequest::Uninstall(package) => {
            // TODO
            Ok(None)
        }
        LocalRequest::Delete(package) => {
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
            print_to_terminal(0, &format!("app store: got download request for {:?}", package));
            print_to_terminal(0, &format!("app store: state: {:?}", state));
            let Some(package_state) = state.packages.get(&package) else {
                return Ok(Some(Resp::RemoteResponse(RemoteResponse::DownloadDenied)))
            };
            if !package_state.mirroring {
                return Ok(Some(Resp::RemoteResponse(RemoteResponse::DownloadDenied)));
            }
            // get the .zip from VFS and attach as payload to response
            let vfs_address = Address {
                node: our.node.clone(),
                process: ProcessId::from_str("vfs:sys:uqbar")?,
            };
            let file_name = format!("/{}.zip", package.to_string());
            let _ = process_lib::send_and_await_response(
                &vfs_address,
                false,
                Some(serde_json::to_string(&kt::VfsRequest {
                    drive: package.to_string(),
                    action: kt::VfsAction::GetEntry(file_name.clone()),
                })?),
                None,
                None,
                5,
            )?;
            // transfer will inherit the payload bytes we receive from VFS
            spawn_transfer(&our, &file_name, None, &source);
            Ok(Some(Resp::RemoteResponse(RemoteResponse::DownloadApproved)))
        }
    }
}
