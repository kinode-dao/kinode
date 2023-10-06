cargo_component_bindings::generate!();

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use bindings::component::uq_process::types::*;
use bindings::{get_capability, has_capability, Guest, print_to_terminal, receive, send_request, send_requests, spawn};

mod kernel_types;
use kernel_types as kt;
mod process_lib;

struct Component;

const PREFIX: &str = "key_value-";

fn make_cap(kind: &str, drive: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "kind": kind,
        "drive": drive,
    })).unwrap()
}

fn handle_message (
    our: &Address,
    drive_to_process: &mut HashMap<String, ProcessId>,
) -> anyhow::Result<()> {
    let (source, message) = receive().unwrap();
    // let (source, message) = receive()?;

    if our.node != source.node {
        return Err(anyhow::anyhow!(
            "rejecting foreign Message from {:?}",
            source,
        ));
    }

    match message {
        Message::Response(_) => { unimplemented!() },
        Message::Request(Request { inherit: _ , expects_response: _, ipc, metadata: _ }) => {
            match process_lib::parse_message_ipc(ipc.clone())? {
                kt::KeyValueMessage::New { ref drive } => {
                    //  TODO: make atomic
                    //  (1): create vfs
                    //  (2): spin up worker, granting vfs caps
                    //  (3): issue new caps
                    //  (4): persist

                    if drive_to_process.contains_key(drive) {
                        return Err(anyhow::anyhow!(
                            "rejecting New for drive that already exists: {}",
                            drive,
                        ))
                    }

                    //  (1)
                    let vfs_address = Address {
                        node: our.node.clone(),
                        process: ProcessId::Name("vfs".into()),
                    };
                    let vfs_drive = format!("{}{}", PREFIX, drive);
                    let _ = process_lib::send_and_await_response(
                        &vfs_address,
                        false,
                        Some(serde_json::to_string(&kt::VfsRequest::New {
                            drive: vfs_drive.clone(),
                        }).unwrap()),
                        None,
                        None,
                        15,
                    ).unwrap();

                    //  (2)
                    let vfs_read = get_capability(
                        &vfs_address,
                        &make_cap("read", &vfs_drive),
                    ).ok_or(anyhow::anyhow!("New failed: no vfs 'read' capability found"))?;
                    let vfs_write = get_capability(
                        &vfs_address,
                        &make_cap("write", &vfs_drive),
                    ).ok_or(anyhow::anyhow!("New failed: no vfs 'write' capability found"))?;
                    let Some(spawned_process_id) = spawn(
                        &ProcessId::Id(0),
                        "key_value",
                        "/key_value_worker.wasm",
                        &OnPanic::None,  //  TODO: notify us
                        &Capabilities::Some(vec![vfs_read, vfs_write]),
                        false, // not public
                    ) else {
                        panic!("couldn't spawn");  //  TODO
                    };

                    //  (3)
                    send_requests(&vec![
                        //  grant caps to source
                        (
                            Address {
                                node: our.node.clone(),
                                process: ProcessId::Name("kernel".into()),
                            },
                            Request {
                                inherit: false,
                                expects_response: None,
                                ipc: Some(serde_json::to_string(&kt::KernelCommand::GrantCapability {
                                    to_process: kt::de_wit_process_id(source.process.clone()),
                                    params: make_cap("read", drive),
                                }).unwrap()),
                                metadata: None,
                            },
                            None,
                            None,
                        ),
                        (
                            Address {
                                node: our.node.clone(),
                                process: ProcessId::Name("kernel".into()),
                            },
                            Request {
                                inherit: false,
                                expects_response: None,
                                ipc: Some(serde_json::to_string(&kt::KernelCommand::GrantCapability {
                                    to_process: kt::de_wit_process_id(source.process.clone()),
                                    params: make_cap("write", drive),
                                }).unwrap()),
                                metadata: None,
                            },
                            None,
                            None,
                        ),
                        (
                            Address {
                                node: our.node.clone(),
                                process: ProcessId::Name("kernel".into()),
                            },
                            Request {
                                inherit: false,
                                expects_response: None,
                                ipc: Some(serde_json::to_string(&kt::KernelCommand::GrantCapability {
                                    to_process: kt::de_wit_process_id(spawned_process_id.clone()),
                                    params: serde_json::to_string(&serde_json::json!({
                                        "messaging": kt::de_wit_process_id(our.process.clone()),
                                    })).unwrap(),
                                }).unwrap()),
                                metadata: None,
                            },
                            None,
                            None,
                        ),
                        //  initialize worker
                        (
                            Address {
                                node: our.node.clone(),
                                process: spawned_process_id.clone(),
                            },
                            Request {
                                inherit: false,
                                expects_response: None,
                                ipc,
                                metadata: None,
                            },
                            None,
                            None,
                        ),
                    ]);

                    //  (4)
                    drive_to_process.insert(drive.into(), spawned_process_id);
                    //  TODO
                },
                kt::KeyValueMessage::Write { ref drive, key: _ } => {
                    if has_capability(&make_cap("write", drive)) {
                        //  forward
                        let Some(process_id) = drive_to_process.get(drive) else {
                            //  TODO
                            return Err(anyhow::anyhow!(
                                "cannot write to non-existent drive {}",
                                drive,
                            ));
                        };
                        send_request(
                            &Address {
                                node: our.node.clone(),
                                process: process_id.clone(),
                            },
                            &Request {
                                inherit: true,
                                expects_response: None,
                                ipc,
                                metadata: None,
                            },
                            None,
                            None,
                        );
                    } else {
                        //  reject
                        //  TODO
                        return Err(anyhow::anyhow!(
                            "cannot write to drive: missing 'write' capability; {}",
                            drive,
                        ));
                    }
                },
                kt::KeyValueMessage::Read { ref drive, key: _ } => {
                    if has_capability(&make_cap("read", drive)) {
                        //  forward
                        let Some(process_id) = drive_to_process.get(drive) else {
                            //  TODO
                            return Err(anyhow::anyhow!(
                                "cannot read from non-existent drive {}",
                                drive,
                            ));
                        };
                        send_request(
                            &Address {
                                node: our.node.clone(),
                                process: process_id.clone(),
                            },
                            &Request {
                                inherit: true,
                                expects_response: None,
                                ipc,
                                metadata: None,
                            },
                            None,
                            None,
                        );
                    } else {
                        //  reject
                        //  TODO
                        return Err(anyhow::anyhow!(
                            "cannot read from drive: missing 'read' capability; {}",
                            drive,
                        ));
                    }
                },
            }

            Ok(())
        },
    }
}

impl Guest for Component {
    fn init(our: Address) {
        print_to_terminal(1, "key_value: begin");

        let mut drive_to_process: HashMap<String, ProcessId> = HashMap::new();

        loop {
            match handle_message(&our, &mut drive_to_process) {
                Ok(()) => {},
                Err(e) => {
                    //  TODO: should we send an error on failure?
                    print_to_terminal(0, format!(
                        "key_value: error: {:?}",
                        e,
                    ).as_str());
                },
            };
        }
    }
}
