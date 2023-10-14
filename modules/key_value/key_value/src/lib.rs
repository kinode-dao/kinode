cargo_component_bindings::generate!();

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use bindings::component::uq_process::types::*;
use bindings::{create_capability, get_capability, has_capability, Guest, print_to_terminal, receive, send_request, send_response, spawn};

mod kernel_types;
use kernel_types as kt;
mod key_value_types;
use key_value_types as kv;
mod process_lib;

struct Component;

const PREFIX: &str = "key_value-";

type DbToProcess = HashMap<String, ProcessId>;

fn make_vfs_cap(kind: &str, drive: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "kind": kind,
        "drive": drive,
    })).unwrap()
}

fn make_db_cap(kind: &str, db: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "kind": kind,
        "db": db,
    })).unwrap()
}

fn forward_if_have_cap(
    our: &Address,
    operation_type: &str,
    // operation_type: OperationType,
    db: &str,
    ipc: Option<String>,
    db_to_process: &mut DbToProcess,
) -> anyhow::Result<()> {
    if has_capability(&make_db_cap(operation_type, db)) {
        //  forward
        let Some(process_id) = db_to_process.get(db) else {
            return Err(kv::KeyValueError::DbDoesNotExist.into());
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
        return Ok(());
    } else {
        //  reject
        return Err(kv::KeyValueError::NoCap.into());
    }
}

fn handle_message (
    our: &Address,
    db_to_process: &mut DbToProcess,
) -> anyhow::Result<()> {
    let (source, message) = receive().unwrap();
    // let (source, message) = receive()?;

    if our.node != source.node {
        return Err(kv::KeyValueError::RejectForeign.into());
    }

    match message {
        Message::Response(r) => {
            return Err(kv::KeyValueError::UnexpectedResponse.into());
        },
        Message::Request(Request { ipc, .. }) => {
            match process_lib::parse_message_ipc(ipc.clone())? {
                kv::KeyValueMessage::New { ref db } => {
                    //  TODO: make atomic
                    //  (1): create vfs
                    //  (2): spin up worker, granting vfs caps
                    //  (3): issue new caps
                    //  (4): persist

                    if db_to_process.contains_key(db) {
                        return Err(kv::KeyValueError::DbAlreadyExists.into());
                    }

                    //  (1)
                    let vfs_address = Address {
                        node: our.node.clone(),
                        process: kt::ProcessId::new("vfs", "sys", "uqbar").en_wit(),
                    };
                    let vfs_drive = format!("{}{}", PREFIX, db);
                    let _ = process_lib::send_and_await_response(
                        &vfs_address,
                        false,
                        Some(serde_json::to_string(&kt::VfsRequest {
                            drive: vfs_drive.clone(),
                            action: kt::VfsAction::New,
                        }).unwrap()),
                        None,
                        None,
                        15,
                    ).unwrap();

                    //  (2)
                    let vfs_read = get_capability(
                        &vfs_address,
                        &make_vfs_cap("read", &vfs_drive),
                    ).ok_or(anyhow::anyhow!("New failed: no vfs 'read' capability found"))?;
                    let vfs_write = get_capability(
                        &vfs_address,
                        &make_vfs_cap("write", &vfs_drive),
                    ).ok_or(anyhow::anyhow!("New failed: no vfs 'write' capability found"))?;
                    let spawned_process_id = match spawn(
                        None,
                        "/key_value_worker.wasm",
                        &OnPanic::None,  //  TODO: notify us
                        &Capabilities::Some(vec![vfs_read, vfs_write]),
                        false, // not public
                    ) {
                        Ok(spawned_process_id) => spawned_process_id,
                        Err(e) => {
                            print_to_terminal(0, &format!("couldn't spawn: {}", e));
                            panic!("couldn't spawn");  //  TODO
                        },
                    };
                    //  grant caps
                    create_capability(&source.process, &make_db_cap("read", db));
                    create_capability(&source.process, &make_db_cap("write", db));
                    //  initialize worker
                    send_request(
                        &Address {
                            node: our.node.clone(),
                            process: spawned_process_id.clone(),
                        },
                        &Request {
                            inherit: false,
                            expects_response: None,
                            ipc,
                            metadata: None,
                        },
                        None,
                        None,
                    );

                    //  (4)
                    db_to_process.insert(db.into(), spawned_process_id);
                    //  TODO
                },
                kv::KeyValueMessage::Write { ref db, .. } => {
                    forward_if_have_cap(our, "write", db, ipc, db_to_process)?;
                },
                kv::KeyValueMessage::Read { ref db, .. } => {
                    forward_if_have_cap(our, "read", db, ipc, db_to_process)?;
                },
                kv::KeyValueMessage::Err { error } => {
                    return Err(error.into());
                }
            }

            Ok(())
        },
    }
}

impl Guest for Component {
    fn init(our: Address) {
        print_to_terminal(0, "key_value: begin");

        let mut db_to_process: HashMap<String, ProcessId> = HashMap::new();

        loop {
            match handle_message(&our, &mut db_to_process) {
                Ok(()) => {},
                Err(e) => {
                    print_to_terminal(0, format!(
                        "key_value: error: {:?}",
                        e,
                    ).as_str());
                    if let Some(e) = e.downcast_ref::<kv::KeyValueError>() {
                        send_response(
                            &Response {
                                ipc: Some(serde_json::to_string(&e).unwrap()),
                                metadata: None,
                            },
                            None,
                        );
                    }
                },
            };
        }
    }
}
