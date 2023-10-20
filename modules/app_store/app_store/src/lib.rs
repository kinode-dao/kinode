cargo_component_bindings::generate!();
use bindings::{
    component::uq_process::types::*, get_capability, get_payload, print_to_terminal, receive,
    send_request, send_response, Guest,
};
use serde::{Deserialize, Serialize};
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
}

/// state of an individual package we have downloaded
#[derive(Debug, Serialize, Deserialize)]
struct PackageState {
    pub mirrored_from: NodeId,
    pub listing_data: Option<PackageListing>, // None if package is unlisted
    pub installed_version: Option<kt::PackageVersion>, // None if downloaded but not installed
    pub mirroring: bool,                      // are we serving this package to others?
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
    FTWorkerCommand(ft_worker_lib::FTWorkerCommand),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)] // untagged as a meta-type for all responses
pub enum Resp {
    // note that we do not need to ourselves handle local responses, as
    // those are given to others rather than received.
    RemoteResponse(RemoteResponse),
    FTWorkerResult(ft_worker_lib::FTWorkerResult),
}

/// Local requests take this form.
#[derive(Debug, Serialize, Deserialize)]
pub enum LocalRequest {
    /// expects a zipped package as payload: create a new package from it
    /// if requested, will return a NewPackageResponse indicating success/failure
    NewPackage {
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
    Success,
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

        // begin by granting messaging capabilities to http_bindings and terminal,
        // so that they can send us requests.
        process_lib::grant_messaging(
            &our,
            &Vec::from([
                ProcessId::from_str("http_bindings:http_bindings:uqbar").unwrap(),
                ProcessId::from_str("terminal:terminal:uqbar").unwrap(),
            ]),
        );
        print_to_terminal(0, &format!("app_store main proc: start"));

        // load in our saved state or initalize a new one if none exists
        let mut state = process_lib::get_state::<State>().unwrap_or(State {
            packages: HashMap::new(),
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
            match message {
                Message::Request(req) => {
                    let Some(ref ipc) = req.ipc else {
                        continue;
                    };
                    match &serde_json::from_str::<Req>(ipc) {
                        Ok(Req::LocalRequest(local_request)) => {
                            match handle_local_request(local_request) {
                                Ok(None) => continue,
                                Ok(Some(resp)) => {
                                    if req.expects_response.is_some() {
                                        send_response(
                                            &Response {
                                                inherit: false,
                                                ipc: Some(
                                                    serde_json::to_string(&resp).unwrap(),
                                                ),
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
                            match handle_remote_request(remote_request) {
                                Ok(None) => continue,
                                Ok(Some(resp)) => {
                                    if req.expects_response.is_some() {
                                        send_response(
                                            &Response {
                                                inherit: false,
                                                ipc: Some(
                                                    serde_json::to_string(&resp).unwrap(),
                                                ),
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
                        Ok(Req::FTWorkerCommand(ft_worker_command)) => {
                            // TODO handle ft_worker commands
                        }
                        Err(_) => {
                            continue;
                        }
                    }
                }
                Message::Response((response, context)) => {
                    let Some(ref ipc) = response.ipc else {
                        continue;
                    };
                    match &serde_json::from_str::<Resp>(ipc) {
                        Ok(Resp::RemoteResponse(remote_response)) => {
                            // TODO handle remote response
                        }
                        Ok(Resp::FTWorkerResult(ft_worker_result)) => {
                            // TODO handle ft_worker result
                        }
                        Err(_) => {
                            continue;
                        }
                    }
                    //     // only expecting NewFromRemote for apps we've requested
                    //     match serde_json::from_str(&response.ipc.unwrap_or_default()) {
                    //         Ok(AppTrackerResponse::NewFromRemote { package_id }) => {
                    //             if let Some(install_from) = state.requested_packages.remove(&package_id)
                    //             {
                    //                 if install_from == source.node {
                    //                     // auto-take zip from payload and request ourself with New
                    //                     let _ = send_request(
                    //                         &our,
                    //                         &Request {
                    //                             inherit: true, // will inherit payload!
                    //                             expects_response: None,
                    //                             ipc: Some(
                    //                                 serde_json::to_string(&AppTrackerRequest::New {
                    //                                     package: package_id,
                    //                                     mirror: true,
                    //                                 })
                    //                                 .unwrap(),
                    //                             ),
                    //                             metadata: None,
                    //                         },
                    //                         None,
                    //                         None,
                    //                     );
                    //                 } else {
                    //                     print_to_terminal(
                    //                         0,
                    //                         &format!(
                    //                             "app-store: got install response from bad source: {}",
                    //                             install_from
                    //                         ),
                    //                     );
                    //                 }
                    //             }
                    //         }
                    //         err => {
                    //             print_to_terminal(
                    //                 0,
                    //                 &format!("app-store: got unexpected response {:?}", err),
                    //             );
                    //         }
                    //     }
                }
            }
        }
    }
}

fn handle_local_request(request: &LocalRequest) -> anyhow::Result<Option<Resp>> {
    // TODO
    Ok(None)
}

fn handle_remote_request(request: &RemoteRequest) -> anyhow::Result<Option<Resp>> {
    // TODO
    Ok(None)
}

// fn parse_command(
//     our: &Address,
//     source: &Address,
//     request_string: String,
//     state: &mut AppTrackerState,
// ) -> anyhow::Result<Option<AppTrackerResponse>> {
//     match serde_json::from_str(&request_string)? {
//         // create a new package based on local payload
//         AppTrackerRequest::New { package, mirror } => {
//             if our.node != source.node {
//                 return Err(anyhow::anyhow!("new package request from non-local node"));
//             }
//             let Some(mut payload) = get_payload() else {
//                 return Err(anyhow::anyhow!("no payload"));
//             };

//             let vfs_address = Address {
//                 node: our.node.clone(),
//                 process: ProcessId::from_str("vfs:sys:uqbar")?,
//             };

//             let _ = process_lib::send_and_await_response(
//                 &vfs_address,
//                 false,
//                 Some(serde_json::to_string(&kt::VfsRequest {
//                     drive: package.to_string(),
//                     action: kt::VfsAction::New,
//                 })?),
//                 None,
//                 None,
//                 5,
//             )?;

//             // add zip bytes
//             payload.mime = Some("application/zip".to_string());
//             let _ = process_lib::send_and_await_response(
//                 &vfs_address,
//                 true,
//                 Some(serde_json::to_string(&kt::VfsRequest {
//                     drive: package.to_string(),
//                     action: kt::VfsAction::Add {
//                         full_path: package.to_string(),
//                         entry_type: kt::AddEntryType::ZipArchive,
//                     },
//                 })?),
//                 None,
//                 Some(&payload),
//                 5,
//             )?;

//             // save the zip file itself in VFS for sharing with other nodes
//             // call it <package>.zip
//             let _ = process_lib::send_and_await_response(
//                 &vfs_address,
//                 true,
//                 Some(serde_json::to_string(&kt::VfsRequest {
//                     drive: package.to_string(),
//                     action: kt::VfsAction::Add {
//                         full_path: format!("/{}.zip", package.to_string()),
//                         entry_type: kt::AddEntryType::NewFile,
//                     },
//                 })?),
//                 None,
//                 Some(&payload),
//                 5,
//             )?;

//             // if mirror, save in our state
//             if mirror {
//                 let _ = process_lib::send_and_await_response(
//                     &vfs_address,
//                     false,
//                     Some(serde_json::to_string(&kt::VfsRequest {
//                         drive: package.to_string(),
//                         action: kt::VfsAction::GetEntry("/metadata.json".into()),
//                     })?),
//                     None,
//                     None,
//                     5,
//                 )?;
//                 let Some(payload) = get_payload() else {
//                     return Err(anyhow::anyhow!("no metadata payload"));
//                 };
//                 let metadata = String::from_utf8(payload.bytes)?;
//                 let metadata = serde_json::from_str::<PackageMetadata>(&metadata)?;
//                 state
//                     .mirrored_packages
//                     .insert(PackageId::new(&metadata.package, &metadata.publisher));
//                 process_lib::set_state::<AppTrackerState>(&state);
//             }

//             Ok(Some(AppTrackerResponse::New {
//                 package: package.to_string(),
//             }))
//         }
//         // if we are the source, forward to install_from target.
//         // if we install_from, respond with package if we have it
//         AppTrackerRequest::NewFromRemote {
//             package_id,
//             install_from,
//         } => {
//             if our.node == source.node {
//                 let _ = send_request(
//                     &Address {
//                         node: install_from.clone(),
//                         process: our.process.clone(),
//                     },
//                     &Request {
//                         inherit: true,
//                         expects_response: Some(5), // TODO
//                         ipc: Some(serde_json::to_string(&AppTrackerRequest::NewFromRemote {
//                             package_id: package_id.clone(),
//                             install_from: install_from.clone(),
//                         })?),
//                         metadata: None,
//                     },
//                     None,
//                     None,
//                 );
//                 state.requested_packages.insert(package_id, install_from);
//                 process_lib::set_state::<AppTrackerState>(&state);
//                 Ok(None)
//             } else if our.node == install_from {
//                 let Some(_mirror) = state.mirrored_packages.get(&package_id) else {
//                     return Ok(Some(AppTrackerResponse::Error { error: "package not mirrored here!".into() }))
//                 };
//                 // get the .zip from VFS and attach as payload to response
//                 let vfs_address = Address {
//                     node: our.node.clone(),
//                     process: ProcessId::from_str("vfs:sys:uqbar")?,
//                 };
//                 let _ = process_lib::send_and_await_response(
//                     &vfs_address,
//                     false,
//                     Some(serde_json::to_string(&kt::VfsRequest {
//                         drive: package_id.to_string(),
//                         action: kt::VfsAction::GetEntry(format!("/{}.zip", package_id.to_string())),
//                     })?),
//                     None,
//                     None,
//                     5,
//                 )?;
//                 Ok(Some(AppTrackerResponse::NewFromRemote { package_id }))
//             } else {
//                 // TODO what to do here?
//                 Ok(None)
//             }
//         }
//         AppTrackerRequest::Install { package } => {
//             if our.node != source.node {
//                 return Err(anyhow::anyhow!("install request from non-local node"));
//             }
//             let vfs_address = Address {
//                 node: our.node.clone(),
//                 process: ProcessId::from_str("vfs:sys:uqbar")?,
//             };

//             let _ = process_lib::send_and_await_response(
//                 &vfs_address,
//                 false,
//                 Some(serde_json::to_string(&kt::VfsRequest {
//                     drive: package.to_string(),
//                     action: kt::VfsAction::GetEntry("/manifest.json".into()),
//                 })?),
//                 None,
//                 None,
//                 5,
//             )?;
//             let Some(payload) = get_payload() else {
//                 return Err(anyhow::anyhow!("no payload"));
//             };
//             let manifest = String::from_utf8(payload.bytes)?;
//             let manifest = serde_json::from_str::<Vec<PackageManifestEntry>>(&manifest)?;

//             for entry in manifest {
//                 let path = if entry.process_wasm_path.starts_with("/") {
//                     entry.process_wasm_path
//                 } else {
//                     format!("/{}", entry.process_wasm_path)
//                 };

//                 let (_, hash_response) = process_lib::send_and_await_response(
//                     &vfs_address,
//                     false,
//                     Some(serde_json::to_string(&kt::VfsRequest {
//                         drive: package.to_string(),
//                         action: kt::VfsAction::GetHash(path.clone()),
//                     })?),
//                     None,
//                     None,
//                     5,
//                 )?;

//                 let Message::Response((Response { ipc: Some(ipc), .. }, _)) = hash_response else {
//                     return Err(anyhow::anyhow!("bad vfs response"));
//                 };
//                 let kt::VfsResponse::GetHash(Some(hash)) = serde_json::from_str(&ipc)? else {
//                     return Err(anyhow::anyhow!("no hash in vfs"));
//                 };

//                 // build initial caps
//                 let mut initial_capabilities: HashSet<kt::SignedCapability> = HashSet::new();
//                 if entry.request_networking {
//                     let Some(networking_cap) = get_capability(
//                         &Address {
//                             node: our.node.clone(),
//                             process: ProcessId::from_str("kernel:sys:uqbar")?,
//                         },
//                         &"\"network\"".to_string(),
//                     ) else {
//                         return Err(anyhow::anyhow!("app-store: no net cap"));
//                     };
//                     initial_capabilities.insert(kt::de_wit_signed_capability(networking_cap));
//                 }
//                 let Some(read_cap) = get_capability(
//                     &vfs_address.clone(),
//                     &serde_json::to_string(&serde_json::json!({
//                         "kind": "read",
//                         "drive": package.to_string(),
//                     }))?,
//                 ) else {
//                     return Err(anyhow::anyhow!("app-store: no read cap"));
//                 };
//                 initial_capabilities.insert(kt::de_wit_signed_capability(read_cap));
//                 let Some(write_cap) = get_capability(
//                     &vfs_address.clone(),
//                     &serde_json::to_string(&serde_json::json!({
//                         "kind": "write",
//                         "drive": package.to_string(),
//                     }))?,
//                 ) else {
//                     return Err(anyhow::anyhow!("app-store: no write cap"));
//                 };
//                 initial_capabilities.insert(kt::de_wit_signed_capability(write_cap));

//                 for process_name in &entry.request_messaging {
//                     let Ok(parsed_process_id) = ProcessId::from_str(&process_name) else {
//                         // TODO handle arbitrary caps here
//                         continue;
//                     };
//                     let Some(messaging_cap) = get_capability(
//                         &Address {
//                             node: our.node.clone(),
//                             process: parsed_process_id.clone(),
//                         },
//                         &"\"messaging\"".into()
//                     ) else {
//                         print_to_terminal(0, &format!("app-store: no cap for {} to give away!", process_name));
//                         continue;
//                     };
//                     initial_capabilities.insert(kt::de_wit_signed_capability(messaging_cap));
//                 }

//                 let process_id = format!("{}:{}", entry.process_name, package.to_string());
//                 let Ok(parsed_new_process_id) = ProcessId::from_str(&process_id) else {
//                     return Err(anyhow::anyhow!("app-store: invalid process id!"));
//                 };
//                 let _ = process_lib::send_request(
//                     &Address {
//                         node: our.node.clone(),
//                         process: ProcessId::from_str("kernel:sys:uqbar")?,
//                     },
//                     false,
//                     Some(serde_json::to_string(&kt::KernelCommand::KillProcess(
//                         kt::ProcessId::de_wit(parsed_new_process_id.clone()),
//                     ))?),
//                     None,
//                     None,
//                     None,
//                 );

//                 // kernel start process takes bytes as payload + wasm_bytes_handle...
//                 // reconsider perhaps
//                 let (_, _bytes_response) = process_lib::send_and_await_response(
//                     &vfs_address,
//                     false,
//                     Some(serde_json::to_string(&kt::VfsRequest {
//                         drive: package.to_string(),
//                         action: kt::VfsAction::GetEntry(path),
//                     })?),
//                     None,
//                     None,
//                     5,
//                 )?;

//                 let Some(payload) = get_payload() else {
//                     return Err(anyhow::anyhow!("no wasm bytes payload."));
//                 };

//                 let _ = process_lib::send_and_await_response(
//                     &Address {
//                         node: our.node.clone(),
//                         process: ProcessId::from_str("kernel:sys:uqbar")?,
//                     },
//                     false,
//                     Some(serde_json::to_string(&kt::KernelCommand::StartProcess {
//                         id: kt::ProcessId::de_wit(parsed_new_process_id),
//                         wasm_bytes_handle: hash,
//                         on_panic: entry.on_panic,
//                         initial_capabilities,
//                         public: entry.public,
//                     })?),
//                     None,
//                     Some(&payload),
//                     5,
//                 )?;
//             }
//             Ok(Some(AppTrackerResponse::Install {
//                 package: package.to_string(),
//             }))
//         }
//     }
// }
