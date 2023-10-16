cargo_component_bindings::generate!();

use bindings::{
    component::uq_process::types::*, get_capability, get_payload, print_to_terminal, receive, Guest, send_request, send_response
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[allow(dead_code)]
mod kernel_types;
use kernel_types as kt;

#[allow(dead_code)]
mod process_lib;

struct Component;

#[derive(Debug, Serialize, Deserialize)]
pub enum AppTrackerRequest {
    New { package: String },
    Install { package: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ApptrackerResponse {
    New { package: String },
    Install { package: String },
    Error { error: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PackageManifestEntry {
    pub process_name: String,
    pub process_wasm_path: String,
    pub on_panic: kt::OnPanic,
    pub request_networking: bool,
    pub request_messaging: Vec<String>,
    pub grant_messaging: Vec<String>, // special logic for the string "all": makes process public
}

fn parse_command(our: &Address, request_string: String) -> anyhow::Result<ApptrackerResponse> {
    match serde_json::from_str(&request_string)? {
        AppTrackerRequest::New { package } => {
            let Some(payload) = get_payload() else {
                return Err(anyhow::anyhow!("no payload"));
            };

            let vfs_address = Address {
                node: our.node.clone(),
                process: ProcessId::from_str("vfs:sys:uqbar").unwrap(),
            };

            let _ = process_lib::send_and_await_response(
                &vfs_address,
                false,
                Some(
                    serde_json::to_string(&kt::VfsRequest {
                        drive: package.clone(),
                        action: kt::VfsAction::New,
                    })
                    .unwrap(),
                ),
                None,
                None,
                5,
            )?;

            // add zip bytes
            let _ = process_lib::send_and_await_response(
                &vfs_address,
                true,
                Some(
                    serde_json::to_string(&kt::VfsRequest {
                        drive: package.clone(),
                        action: kt::VfsAction::Add {
                            full_path: package.clone().into(),
                            entry_type: kt::AddEntryType::ZipArchive,
                        },
                    })
                    .unwrap(),
                ),
                None,
                Some(&payload),
                5,
            )?;
            Ok(ApptrackerResponse::New { package })
        }
        AppTrackerRequest::Install { package } => {
            let vfs_address = Address {
                node: our.node.clone(),
                process: ProcessId::from_str("vfs:sys:uqbar").unwrap(),
            };

            let _ = process_lib::send_and_await_response(
                &vfs_address,
                false,
                Some(
                    serde_json::to_string(&kt::VfsRequest {
                        drive: package.clone(),
                        action: kt::VfsAction::GetEntry("/manifest.json".into()),
                    })
                    .unwrap(),
                ),
                None,
                None,
                5,
            )?;
            let Some(payload) = get_payload() else {
                return Err(anyhow::anyhow!("no payload"));
            };
            let manifest = String::from_utf8(payload.bytes)?;
            let manifest = serde_json::from_str::<Vec<PackageManifestEntry>>(&manifest).unwrap();

            for entry in manifest {
                let path = if entry.process_wasm_path.starts_with("/") {
                    entry.process_wasm_path
                } else {
                    format!("/{}", entry.process_wasm_path)
                };


                let (_, hash_response) = process_lib::send_and_await_response(
                    &vfs_address,
                    false,
                    Some(
                        serde_json::to_string(&kt::VfsRequest {
                            drive: package.clone(),
                            action: kt::VfsAction::GetHash(path.clone()),
                        })
                        .unwrap(),
                    ),
                    None,
                    None,
                    5,
                )?;


                let Message::Response((Response { ipc: Some(ipc), .. }, _)) = hash_response else {
                    return Err(anyhow::anyhow!("bad vfs response"));
                };
                let kt::VfsResponse::GetHash(Some(hash)) = serde_json::from_str(&ipc).unwrap() else {
                    return Err(anyhow::anyhow!("no hash in vfs"));
                };

                // build initial caps
                let mut initial_capabilities: HashSet<kt::SignedCapability> = HashSet::new();
                if entry.request_networking {
                    let Some(networking_cap) = get_capability(
                        &Address {
                            node: our.node.clone(),
                            process: ProcessId::from_str("kernel:sys:uqbar").unwrap(),
                        },
                        &"\"network\"".to_string(),
                    ) else {
                        return Err(anyhow::anyhow!("app_tracker: no net cap"));
                    };
                    initial_capabilities.insert(kt::de_wit_signed_capability(networking_cap));
                }
                let Some(read_cap) = get_capability(
                    &vfs_address.clone(),
                    &serde_json::to_string(&serde_json::json!({
                        "kind": "read",
                        "drive": package,
                    })).unwrap(),
                ) else {
                    return Err(anyhow::anyhow!("app_tracker: no read cap"));
                };
                initial_capabilities.insert(kt::de_wit_signed_capability(read_cap));
                let Some(write_cap) = get_capability(
                    &vfs_address.clone(),
                    &serde_json::to_string(&serde_json::json!({
                        "kind": "write",
                        "drive": package,
                    })).unwrap(),
                ) else {
                    return Err(anyhow::anyhow!("app_tracker: no write cap"));
                };
                initial_capabilities.insert(kt::de_wit_signed_capability(write_cap));
                let mut public = false;

                for process_name in &entry.grant_messaging {
                    if process_name == "all" {
                        public = true;
                        continue;
                    }
                    let Ok(parsed_process_id) = ProcessId::from_str(&process_name) else {
                        continue;
                    };
                    let Some(messaging_cap) = get_capability(
                        &Address {
                            node: our.node.clone(),
                            process: parsed_process_id.clone(),
                        },
                        &"\"messaging\"".into()
                    ) else {
                        return Err(anyhow::anyhow!(format!("app_tracker: no cap for {}", process_name)));
                    };
                    initial_capabilities.insert(kt::de_wit_signed_capability(messaging_cap));
                }

                for process_name in &entry.request_messaging {
                    let Ok(parsed_process_id) = ProcessId::from_str(process_name) else {
                        continue;
                    };
                    let Some(messaging_cap) = get_capability(
                        &Address {
                            node: our.node.clone(),
                            process: parsed_process_id.clone(),
                        },
                        &"\"messaging\"".into()
                    ) else {
                        return Err(anyhow::anyhow!(format!("app_tracker: no cap for {}", process_name)));
                    };
                    initial_capabilities.insert(kt::de_wit_signed_capability(messaging_cap));
                }

                let process_id = format!("{}:{}", entry.process_name, package.clone());
                let Ok(parsed_new_process_id) = ProcessId::from_str(&process_id) else {
                    return Err(anyhow::anyhow!("app_tracker: invalid process id!"));
                };
                let _ = process_lib::send_request(
                    &Address {
                        node: our.node.clone(),
                        process: ProcessId::from_str("kernel:sys:uqbar").unwrap(),
                    },
                    false,
                    Some(
                        serde_json::to_string(
                            &kt::KernelCommand::KillProcess(kt::ProcessId::de_wit(parsed_new_process_id.clone()))).unwrap()),
                    None,
                    None,
                    None,
                );

                // kernel start process takes bytes as payload + wasm_bytes_handle...
                // reconsider perhaps
                let (_, _bytes_response) = process_lib::send_and_await_response(
                    &vfs_address,
                    false,
                    Some(
                        serde_json::to_string(&kt::VfsRequest {
                            drive: package.clone(),
                            action: kt::VfsAction::GetEntry(path),
                        })
                        .unwrap(),
                    ),
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
                        process: ProcessId::from_str("kernel:sys:uqbar").unwrap(),
                    },
                    false,
                    Some(
                        serde_json::to_string(&kt::KernelCommand::StartProcess {
                            id: kt::ProcessId::de_wit(parsed_new_process_id),
                            wasm_bytes_handle: hash,
                            on_panic: entry.on_panic,
                            initial_capabilities,
                            public,
                        })
                        .unwrap(),
                    ),
                    None,
                    Some(&payload),
                    5,
                )?;

            }
            Ok(ApptrackerResponse::Install { package })
        }
    }
}

impl Guest for Component {
    fn init(our: Address) {
        assert_eq!(our.process.to_string(), "app_tracker:app_tracker:uqbar");
        print_to_terminal(0, &format!("app_tracker: start"));
        loop {
            let message = match receive() {
                Ok((source, message)) => {
                    if our.node != source.node {
                        continue;
                    }
                    message
                }
                Err((error, _context)) => {
                    print_to_terminal(0, &format!("net error: {:?}!", error.kind));
                    continue;
                }
            };
            match message {
                Message::Request(Request { ipc, expects_response, metadata, .. }) => {
                    let Some(command) = ipc else {
                        continue;
                    };
                    match parse_command(&our, command) {
                        Ok(response) => {
                            if let Some(_) = expects_response {
                                let _ = send_response(
                                    &Response {
                                        ipc: Some(serde_json::to_string(&response).unwrap()),
                                        metadata,
                                    },
                                    None,
                                );
                            };
                        }
                        Err(e) => {
                            print_to_terminal(0, &format!("app_tracker: got error {}", e));
                            if let Some(_) = expects_response {
                                let error = ApptrackerResponse::Error {
                                    error: format!("{}", e),
                                };
                                let _ = send_response(
                                    &Response {
                                        ipc: Some(serde_json::to_string(&error).unwrap()),
                                        metadata,
                                    },
                                    None,
                                );
                            };
                        }
                    }
                }
                _ => continue,
            }
        }
    }
}
