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

#[allow(dead_code)]
mod process_lib;
use process_lib::PackageId;

mod transfer_lib;

struct Component;

// #[derive(Serialize, Deserialize)]
// struct AppState {
//     // TODO this should mirror onchain listing
//     pub name: String,
//     pub owner: NodeId,
//     pub desc: String,
//     pub website: Option<String>,
//     pub versions: Vec<(u32, String)>, // TODO
// }

#[derive(Serialize, Deserialize)]
struct AppTrackerState {
    pub mirrored_packages: HashSet<PackageId>,
    pub requested_packages: HashMap<PackageId, NodeId>, // who we're expecting it from
}

#[derive(Debug, Serialize, Deserialize)]
pub enum AppTrackerRequest {
    New {
        package: PackageId,
        mirror: bool,
    },
    NewFromRemote {
        package_id: PackageId,
        install_from: NodeId,
    },
    Install {
        package: PackageId,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum AppTrackerResponse {
    New { package: String },
    NewFromRemote { package_id: PackageId },
    Install { package: String },
    Error { error: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PackageMetadata {
    pub package: String,
    pub publisher: String,
    pub desc: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PackageManifestEntry {
    pub process_name: String,
    pub process_wasm_path: String,
    pub on_panic: kt::OnPanic,
    pub request_networking: bool,
    pub request_messaging: Vec<String>,
    pub public: bool,
}

fn parse_command(
    our: &Address,
    source: &Address,
    request_string: String,
    state: &mut AppTrackerState,
) -> anyhow::Result<Option<AppTrackerResponse>> {
    match serde_json::from_str(&request_string)? {
        // create a new package based on local payload
        AppTrackerRequest::New { package, mirror } => {
            if our.node != source.node {
                return Err(anyhow::anyhow!("new package request from non-local node"));
            }
            let Some(mut payload) = get_payload() else {
                return Err(anyhow::anyhow!("no payload"));
            };

            let vfs_address = Address {
                node: our.node.clone(),
                process: ProcessId::from_str("vfs:sys:uqbar")?,
            };

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

            // if mirror, save in our state
            if mirror {
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
                state
                    .mirrored_packages
                    .insert(PackageId::new(&metadata.package, &metadata.publisher));
                process_lib::set_state::<AppTrackerState>(&state);
            }

            Ok(Some(AppTrackerResponse::New { package: package.to_string() }))
        }
        // if we are the source, forward to install_from target.
        // if we install_from, respond with package if we have it
        AppTrackerRequest::NewFromRemote {
            package_id,
            install_from,
        } => {
            if our.node == source.node {
                let _ = send_request(
                    &Address {
                        node: install_from.clone(),
                        process: our.process.clone(),
                    },
                    &Request {
                        inherit: true,
                        expects_response: Some(5), // TODO
                        ipc: Some(serde_json::to_string(&AppTrackerRequest::NewFromRemote {
                            package_id: package_id.clone(),
                            install_from: install_from.clone(),
                        })?),
                        metadata: None,
                    },
                    None,
                    None,
                );
                state.requested_packages.insert(package_id, install_from);
                process_lib::set_state::<AppTrackerState>(&state);
                Ok(None)
            } else if our.node == install_from {
                print_to_terminal(0, &format!("app-store: got new from remote for {}", package_id.to_string()));
                print_to_terminal(0, &format!("{:?}", state.mirrored_packages));
                let Some(_mirror) = state.mirrored_packages.get(&package_id) else {
                    return Ok(Some(AppTrackerResponse::Error { error: "package not mirrored here!".into() }))
                };
                // get the .zip from VFS and attach as payload to response
                let vfs_address = Address {
                    node: our.node.clone(),
                    process: ProcessId::from_str("vfs:sys:uqbar")?,
                };
                let _ = process_lib::send_and_await_response(
                    &vfs_address,
                    false,
                    Some(serde_json::to_string(&kt::VfsRequest {
                        drive: package_id.to_string(),
                        action: kt::VfsAction::GetEntry(format!("/{}.zip", package_id.to_string())),
                    })?),
                    None,
                    None,
                    5,
                )?;
                Ok(Some(AppTrackerResponse::NewFromRemote { package_id }))
            } else {
                // TODO what to do here?
                Ok(None)
            }
        }
        AppTrackerRequest::Install { package } => {
            if our.node != source.node {
                return Err(anyhow::anyhow!("install request from non-local node"));
            }
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
            Ok(Some(AppTrackerResponse::Install { package: package.to_string() }))
        }
    }
}

impl Guest for Component {
    fn init(our: Address) {
        assert_eq!(our.process.to_string(), "main:app_store:uqbar");

        // grant messaging caps to http_bindings and terminal
        let Some(our_messaging_cap) = bindings::get_capability(
            &our,
            &"\"messaging\"".into()
        ) else {
            panic!("missing self-messaging cap!")
        };
        bindings::share_capability(
            &ProcessId::from_str("http_bindings:http_bindings:uqbar").unwrap(),
            &our_messaging_cap,
        );
        bindings::share_capability(
            &ProcessId::from_str("terminal:terminal:uqbar").unwrap(),
            &our_messaging_cap,
        );

        print_to_terminal(0, &format!("app_store main proc: start"));

        let mut state = process_lib::get_state::<AppTrackerState>().unwrap_or(AppTrackerState {
            mirrored_packages: HashSet::new(),
            requested_packages: HashMap::new(),
        });

        loop {
            let (source, message) = match receive() {
                Ok((source, message)) => (source, message),
                Err((error, _context)) => {
                    print_to_terminal(0, &format!("net error: {:?}", error.kind));
                    continue;
                }
            };
            match message {
                Message::Request(Request {
                    ipc,
                    expects_response,
                    metadata,
                    ..
                }) => {
                    let Some(command) = ipc else {
                        continue;
                    };
                    match parse_command(&our, &source, command, &mut state) {
                        Ok(response) => {
                            if let Some(_) = expects_response {
                                print_to_terminal(0, &format!("app-store: sending response {:?}", response));
                                let _ = send_response(
                                    &Response {
                                        inherit: true,
                                        ipc: Some(serde_json::to_string(&response).unwrap()),
                                        metadata,
                                    },
                                    None, // payload will be attached here if created in parse_command
                                );
                            };
                        }
                        Err(e) => {
                            print_to_terminal(0, &format!("app-store: got error {}", e));
                            if let Some(_) = expects_response {
                                let error = AppTrackerResponse::Error {
                                    error: format!("{}", e),
                                };
                                let _ = send_response(
                                    &Response {
                                        inherit: false,
                                        ipc: Some(serde_json::to_string(&error).unwrap()),
                                        metadata,
                                    },
                                    None,
                                );
                            };
                        }
                    }
                }
                Message::Response((response, _)) => {
                    print_to_terminal(0, &format!("app-store: got response {:?}", response));
                    // only expecting NewFromRemote for apps we've requested
                    match serde_json::from_str(&response.ipc.unwrap_or_default()) {
                        Ok(AppTrackerResponse::NewFromRemote { package_id }) => {
                            if let Some(install_from) = state.requested_packages.remove(&package_id)
                            {
                                if install_from == source.node {
                                    print_to_terminal(0, "got install");
                                    // auto-take zip from payload and request ourself with New
                                    let _ = send_request(
                                        &our,
                                        &Request {
                                            inherit: true, // will inherit payload!
                                            expects_response: None,
                                            ipc: Some(
                                                serde_json::to_string(&AppTrackerRequest::New {
                                                    package: package_id,
                                                    mirror: true,
                                                })
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
                        err => {
                            print_to_terminal(
                                0,
                                &format!("app-store: got unexpected response {:?}", err),
                            );
                        }
                    }
                }
            }
        }
    }
}
