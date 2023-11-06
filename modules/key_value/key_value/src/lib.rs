use std::collections::HashMap;

// use serde::{Deserialize, Serialize};

use uqbar_process_lib::{Address, ProcessId, Request, Response};
use uqbar_process_lib::kernel_types as kt;
use uqbar_process_lib::uqbar::process::standard as wit;

wit_bindgen::generate!({
    path: "../../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

mod key_value_types;
use key_value_types as kv;

const PREFIX: &str = "key_value-";

type DbToProcess = HashMap<String, ProcessId>;

fn make_vfs_cap(kind: &str, drive: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "kind": kind,
        "drive": drive,
    }))
    .unwrap()
}

fn make_db_cap(kind: &str, db: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "kind": kind,
        "db": db,
    }))
    .unwrap()
}

fn forward_if_have_cap(
    our: &Address,
    operation_type: &str,
    db: &str,
    ipc: Vec<u8>,
    db_to_process: &mut DbToProcess,
) -> anyhow::Result<()> {
    if wit::has_capability(&make_db_cap(operation_type, db)) {
        //  forward
        let Some(process_id) = db_to_process.get(db) else {
            return Err(kv::KeyValueError::DbDoesNotExist.into());
        };
        Request::new()
            .target(wit::Address {
                node: our.node.clone(),
                process: process_id.clone(),
            })?
            // .target(Address::new(our.node.clone(), process_id.clone()))?
            .inherit(true)
            .ipc_bytes(ipc)
            .send()?;
        return Ok(());
    } else {
        //  reject
        return Err(kv::KeyValueError::NoCap.into());
    }
}

fn handle_message(our: &Address, db_to_process: &mut DbToProcess) -> anyhow::Result<()> {
    let (source, message) = wit::receive().unwrap();

    if our.node != source.node {
        return Err(kv::KeyValueError::RejectForeign.into());
    }

    match message {
        wit::Message::Response(_) => {
            return Err(kv::KeyValueError::UnexpectedResponse.into());
        }
        wit::Message::Request(wit::Request { ipc, .. }) => {
            match serde_json::from_slice(&ipc)? {
                kv::KeyValueMessage::New { ref db } => {
                    //  TODO: make atomic
                    //  (1): create vfs drive
                    //  (2): spin up worker, granting vfs caps
                    //  (3): issue new caps
                    //  (4): persist

                    if db_to_process.contains_key(db) {
                        return Err(kv::KeyValueError::DbAlreadyExists.into());
                    }

                    //  (1)
                    let vfs_address = Address {
                        node: our.node.clone(),
                        process: ProcessId::new("vfs", "sys", "uqbar"),
                    };
                    let vfs_drive = format!("{}{}", PREFIX, db);
                    let _ = Request::new()
                        .target(vfs_address.clone())?
                        .ipc_bytes(serde_json::to_vec(&kt::VfsRequest {
                            drive: vfs_drive.clone(),
                            action: kt::VfsAction::New,
                        })?)
                        .expects_response(15)
                        .send_and_await_response()??;

                    //  (2)
                    let vfs_read = wit::get_capability(&vfs_address, &make_vfs_cap("read", &vfs_drive))
                        .ok_or(anyhow::anyhow!(
                            "New failed: no vfs 'read' capability found"
                        ))?;
                    let vfs_write =
                        wit::get_capability(&vfs_address, &make_vfs_cap("write", &vfs_drive)).ok_or(
                            anyhow::anyhow!("New failed: no vfs 'write' capability found"),
                        )?;
                    let spawned_process_id = match wit::spawn(
                        None,
                        "/key_value_worker.wasm",
                        &wit::OnPanic::None, //  TODO: notify us
                        &wit::Capabilities::Some(vec![vfs_read, vfs_write]),
                        false, // not public
                    ) {
                        Ok(spawned_process_id) => spawned_process_id,
                        Err(e) => {
                            wit::print_to_terminal(0, &format!("couldn't spawn: {}", e));
                            panic!("couldn't spawn"); //  TODO
                        }
                    };
                    //  grant caps
                    wit::create_capability(&source.process, &make_db_cap("read", db));
                    wit::create_capability(&source.process, &make_db_cap("write", db));
                    //  initialize worker
                    Request::new()
                        .target(wit::Address {
                            node: our.node.clone(),
                            process: spawned_process_id.clone(),
                        })?
                        .ipc_bytes(ipc.clone())
                        .send()?;

                    //  (4)
                    db_to_process.insert(db.into(), spawned_process_id);
                    //  TODO: persistence?

                    Response::new()
                        .ipc_bytes(ipc)
                        .send()?;
                }
                kv::KeyValueMessage::Write { ref db, .. } => {
                    forward_if_have_cap(our, "write", db, ipc, db_to_process)?;
                }
                kv::KeyValueMessage::Read { ref db, .. } => {
                    forward_if_have_cap(our, "read", db, ipc, db_to_process)?;
                }
                kv::KeyValueMessage::Err { error } => {
                    return Err(error.into());
                }
            }

            Ok(())
        }
    }
}

struct Component;
impl Guest for Component {
    fn init(our: String) {
        wit::print_to_terminal(0, "key_value: begin");

        let our = Address::from_str(&our).unwrap();
        let mut db_to_process: DbToProcess = HashMap::new();

        loop {
            match handle_message(&our, &mut db_to_process) {
                Ok(()) => {}
                Err(e) => {
                    wit::print_to_terminal(0, format!("key_value: error: {:?}", e,).as_str());
                    if let Some(e) = e.downcast_ref::<kv::KeyValueError>() {
                        Response::new()
                            .ipc_bytes(serde_json::to_vec(&e).unwrap())
                            .send()
                            .unwrap();
                    }
                }
            };
        }
    }
}
