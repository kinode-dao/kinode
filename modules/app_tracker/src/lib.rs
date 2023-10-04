cargo_component_bindings::generate!();

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use bindings::{component::uq_process::types::*, get_capability, get_payload, Guest, print_to_terminal, receive};

mod kernel_types;
use kernel_types as kt;
mod process_lib;

struct Component;

#[derive(Debug, Serialize, Deserialize)]
pub enum AppTrackerRequest {
    New { package: String },
    Install { package: String },
}

#[derive(Debug, Serialize, Deserialize)]
struct ManifestEntry {
    name: String,
    path: String,
    on_panic: kt::OnPanic,
    networking: bool,
    process_caps: Vec<String>,
}

// TODO: error handle
fn parse_command(our: &Address, request_string: String) -> anyhow::Result<()>{
    match serde_json::from_str(&request_string)? {
        AppTrackerRequest::New { package } => {
            //  TODO: should we check if package already exists before creating?

            let Some(payload) = get_payload() else {
                panic!("");
            };

            let vfs_address = Address {
                node: our.node.clone(),
                process: ProcessId::Name("vfs".into()),
            };
            // make vfs package
            let _ = process_lib::send_and_await_response(
                &vfs_address,
                false,
                Some(serde_json::to_string(&kt::VfsRequest::New {
                    identifier: package.clone(),
                }).unwrap()),
                None,
                None,
                5,
            )?;

            // add zip bytes
            let _ = process_lib::send_and_await_response(
                &vfs_address,
                true,
                Some(serde_json::to_string(&kt::VfsRequest::Add {
                    identifier: package.clone(),
                    full_path: "".into(),  // TODO
                    entry_type: kt::AddEntryType::ZipArchive,
                }).unwrap()),
                None,
                Some(&payload),
                5,
            )?;
            Ok(())
        }
        AppTrackerRequest::Install { package } => {
            let vfs_address = Address {
                node: our.node.clone(),
                process: ProcessId::Name("vfs".into()),
            };
            // get manifest
            let _ = process_lib::send_and_await_response(
                &vfs_address,
                false,
                Some(serde_json::to_string(&kt::VfsRequest::GetEntry {
                    identifier: package.clone(),
                    full_path: "/.manifest".into(),
                }).unwrap()),
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
                let path =
                    if entry.path.starts_with("/") {
                        entry.path
                    } else {
                        format!("/{}", entry.path)
                    };

                let (_, hash_response) = process_lib::send_and_await_response(
                    &vfs_address,
                    false,
                    Some(serde_json::to_string(&kt::VfsRequest::GetHash {
                        identifier: package.clone(),
                        full_path: path,
                    }).unwrap()),
                    None,
                    None,
                    5,
                )?;
                let Message::Response((Response { ipc: Some(ipc), .. }, _)) = hash_response else {
                    panic!("baz");
                };
                let kt::VfsResponse::GetHash { hash, .. } = serde_json::from_str(&ipc).unwrap() else {
                    panic!("aaa");
                };

                // build initial caps
                let mut initial_capabilities: HashSet<kt::SignedCapability> = HashSet::new();
                if entry.networking {
                    let Some(networking_cap) = get_capability(
                        &Address {
                            node: our.node.clone(),
                            process: ProcessId::Name("kernel".into()),
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
                        "identifier": package,
                    })).unwrap(),
                ) else {
                    panic!("app_tracker: no read cap");
                };
                initial_capabilities.insert(kt::de_wit_signed_capability(read_cap));
                let Some(write_cap) = get_capability(
                    &vfs_address.clone(),
                    &serde_json::to_string(&serde_json::json!({
                        "kind": "write",
                        "identifier": package,
                    })).unwrap(),
                ) else {
                    panic!("app_tracker: no write cap");
                };
                initial_capabilities.insert(kt::de_wit_signed_capability(write_cap));
                for process_name in entry.process_caps {
                    let Some(messaging_cap) = get_capability(
                        &Address {
                            node: our.node.clone(),
                            process: ProcessId::Name(process_name.clone()),
                        },
                        &serde_json::to_string(&serde_json::json!({
                            "messaging": kt::ProcessId::Name(process_name.into()),
                        })).unwrap(),
                    ) else {
                        panic!("app_tracker: no cap");
                    };
                    initial_capabilities.insert(kt::de_wit_signed_capability(messaging_cap));
                }


                let _ = process_lib::send_and_await_response(
                    &Address {
                        node: our.node.clone(),
                        process: ProcessId::Name("kernel".into()),
                    },
                    false,
                    Some(serde_json::to_string(&kt::KernelCommand::StartProcess {
                        name: Some(entry.name),
                        wasm_bytes_handle: hash,
                        on_panic: entry.on_panic,
                        initial_capabilities,
                    }).unwrap()),
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
        assert_eq!(our.process, ProcessId::Name("app_tracker".into()));
        print_to_terminal(0, &format!("app_tracker: running"));
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
                Message::Request(Request {
                    ipc,
                    ..
                }) => {
                    let Some(command) = ipc else {
                        continue;
                    };
                    match parse_command(&our, command) {
                        Ok(_) => {},
                        Err(e) => {
                            print_to_terminal(0, &format!("app_tracker: got error {}", e));
                        }
                    }
                }
                _ => continue
            }
        }
    }
}
