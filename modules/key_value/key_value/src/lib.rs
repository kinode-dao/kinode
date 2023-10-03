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

fn make_cap(kind: &str, identifier: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "kind": kind,
        "identifier": identifier,
    })).unwrap()
}

fn handle_message (
    our: &Address,
    identifier_to_process: &mut HashMap<String, ProcessId>,
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
                kt::KeyValueMessage::New { ref identifier } => {
                    //  TODO: make atomic
                    //  (1): create vfs
                    //  (2): spin up worker, granting vfs caps
                    //  (3): issue new caps
                    //  (4): persist

                    if identifier_to_process.contains_key(identifier) {
                        return Err(anyhow::anyhow!(
                            "rejecting New for identifier that already exists: {}",
                            identifier,
                        ))
                    }

                    //  (1)
                    let vfs_address = Address {
                        node: our.node.clone(),
                        process: ProcessId::Name("vfs".into()),
                    };
                    let vfs_identifier = format!("{}{}", PREFIX, identifier);
                    let _ = process_lib::send_and_await_response(
                        &vfs_address,
                        false,
                        Some(serde_json::to_string(&kt::VfsRequest::New {
                            identifier: vfs_identifier.clone(),
                        }).unwrap()),
                        None,
                        None,
                        15,
                    ).unwrap();

                    //  (2)
                    let vfs_read = get_capability(
                        &vfs_address,
                        &make_cap("read", &vfs_identifier),
                    ).ok_or(anyhow::anyhow!("New failed: no vfs 'read' capability found"))?;
                    let vfs_write = get_capability(
                        &vfs_address,
                        &make_cap("write", &vfs_identifier),
                    ).ok_or(anyhow::anyhow!("New failed: no vfs 'write' capability found"))?;
                    let Some(spawned_process_id) = spawn(
                        &ProcessId::Id(0),
                        "",  //  TODO
                        &OnPanic::None,  //  TODO: notify us
                        &Capabilities::Some(vec![vfs_read, vfs_write]),
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
                                    params: make_cap("read", identifier),
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
                                    params: make_cap("write", identifier),
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
                    identifier_to_process.insert(identifier.into(), spawned_process_id);
                    //  TODO
                },
                kt::KeyValueMessage::Write { ref identifier, key: _ } => {
                    if has_capability(&make_cap("write", identifier)) {
                        //  forward
                        let Some(process_id) = identifier_to_process.get(identifier) else {
                            //  TODO
                            return Err(anyhow::anyhow!(
                                "cannot write to non-existent identifier {}",
                                identifier,
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
                            "cannot write to identifier: missing 'write' capability; {}",
                            identifier,
                        ));
                    }
                },
                kt::KeyValueMessage::Read { ref identifier, key: _ } => {
                    if has_capability(&make_cap("read", identifier)) {
                        //  forward
                        let Some(process_id) = identifier_to_process.get(identifier) else {
                            //  TODO
                            return Err(anyhow::anyhow!(
                                "cannot read from non-existent identifier {}",
                                identifier,
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
                            "cannot read from identifier: missing 'read' capability; {}",
                            identifier,
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

        let mut identifier_to_process: HashMap<String, ProcessId> = HashMap::new();

        loop {
            match handle_message(&our, &mut identifier_to_process) {
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
