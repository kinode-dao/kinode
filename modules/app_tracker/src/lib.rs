cargo_component_bindings::generate!();

use bindings::{
    component::uq_process::types::*, get_capability, get_payload, print_to_terminal, receive, Guest,
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
struct ManifestEntry {
    id: String, // need to parse into ProcessId
    path: String,
    on_panic: kt::OnPanic,
    networking: bool,
    process_caps: Vec<String>,
}

// TODO: error handle
fn parse_command(our: &Address, request_string: String) -> anyhow::Result<()> {
    match serde_json::from_str(&request_string)? {
        AppTrackerRequest::New { package } => {
            //  TODO: should we check if package already exists before creating?

            let Some(payload) = get_payload() else {
                panic!("");
            };

            let vfs_address = Address {
                node: our.node.clone(),
                process: ProcessId::from_str("vfs:sys:uqbar").unwrap(),
            };
            // make vfs package
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
                            full_path: "".into(), // TODO
                            entry_type: kt::AddEntryType::ZipArchive,
                        },
                    })
                    .unwrap(),
                ),
                None,
                Some(&payload),
                5,
            )?;
            Ok(())
        }
        AppTrackerRequest::Install { package } => {
            let vfs_address = Address {
                node: our.node.clone(),
                process: ProcessId::from_str("vfs:sys:uqbar").unwrap(),
            };
            // get manifest
            let _ = process_lib::send_and_await_response(
                &vfs_address,
                false,
                Some(
                    serde_json::to_string(&kt::VfsRequest {
                        drive: package.clone(),
                        action: kt::VfsAction::GetEntry("/.manifest".into()),
                    })
                    .unwrap(),
                ),
                None,
                None,
                5,
            )?;
            let Some(payload) = get_payload() else {
                panic!("");
            };
            let manifest = String::from_utf8(payload.bytes)?;
            let manifest = serde_json::from_str::<Vec<ManifestEntry>>(&manifest).unwrap();

            for entry in manifest {
                let path = if entry.path.starts_with("/") {
                    entry.path
                } else {
                    format!("/{}", entry.path)
                };

                let (_, hash_response) = process_lib::send_and_await_response(
                    &vfs_address,
                    false,
                    Some(
                        serde_json::to_string(&kt::VfsRequest {
                            drive: package.clone(),
                            action: kt::VfsAction::GetHash(path),
                        })
                        .unwrap(),
                    ),
                    None,
                    None,
                    5,
                )?;
                let Message::Response((Response { ipc: Some(ipc), .. }, _)) = hash_response else {
                    panic!("baz");
                };
                let kt::VfsResponse::GetHash(Some(hash)) = serde_json::from_str(&ipc).unwrap() else {
                    panic!("aaa");
                };

                // build initial caps
                let mut initial_capabilities: HashSet<kt::SignedCapability> = HashSet::new();
                if entry.networking {
                    let Some(networking_cap) = get_capability(
                        &Address {
                            node: our.node.clone(),
                            process: ProcessId::from_str("kernel:sys:uqbar").unwrap(),
                        },
                        &"\"network\"".to_string(),
                    ) else {
                        panic!("app_tracker: no net cap");
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
                    panic!("app_tracker: no read cap");
                };
                initial_capabilities.insert(kt::de_wit_signed_capability(read_cap));
                let Some(write_cap) = get_capability(
                    &vfs_address.clone(),
                    &serde_json::to_string(&serde_json::json!({
                        "kind": "write",
                        "drive": package,
                    })).unwrap(),
                ) else {
                    panic!("app_tracker: no write cap");
                };
                initial_capabilities.insert(kt::de_wit_signed_capability(write_cap));
                let mut public = false;
                for process_name in entry.process_caps {
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
                        &serde_json::to_string(&serde_json::json!({
                            "messaging": kt::ProcessId::de_wit(parsed_process_id),
                        })).unwrap(),
                    ) else {
                        panic!("app_tracker: no cap");
                    };
                    initial_capabilities.insert(kt::de_wit_signed_capability(messaging_cap));
                }

                let Ok(parsed_new_process_id) = ProcessId::from_str(&entry.id) else {
                    panic!("app_tracker: invalid process id");
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
                    None,
                    5,
                )?;
            }
            Ok(())
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
                Message::Request(Request { ipc, .. }) => {
                    let Some(command) = ipc else {
                        continue;
                    };
                    match parse_command(&our, command) {
                        Ok(_) => {}
                        Err(e) => {
                            print_to_terminal(0, &format!("app_tracker: got error {}", e));
                        }
                    }
                }
                _ => continue,
            }
        }
    }
}
