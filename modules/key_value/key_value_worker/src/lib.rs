use std::collections::HashMap;

use redb::ReadableTable;
use serde::{Deserialize, Serialize};

use uqbar_process_lib::{Address, create_capability, ProcessId, Response};
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
const TABLE: redb::TableDefinition<&[u8], &[u8]> = redb::TableDefinition::new("process");

fn get_payload_wrapped() -> Option<(Option<String>, Vec<u8>)> {
   match wit::get_payload() {
       None => None,
       Some(wit::Payload { mime, bytes }) => Some((mime, bytes)),
   }
}

fn send_and_await_response_wrapped(
    target_node: String,
    target_process: String,
    target_package: String,
    target_publisher: String,
    request_ipc: Vec<u8>,
    request_metadata: Option<String>,
    payload: Option<(Option<String>, Vec<u8>)>,
    timeout: u64,
) -> (Vec<u8>, Option<String>) {
    let payload = match payload {
        None => None,
        Some((mime, bytes)) => Some(wit::Payload { mime, bytes }),
    };
    let (
        _,
        wit::Message::Response((wit::Response { ipc, metadata, .. }, _)),
    ) = wit::send_and_await_response(
        &wit::Address {
            node: target_node,
            process: ProcessId::new(
                &target_process,
                &target_package,
                &target_publisher,
            ),
        },
        &wit::Request {
            inherit: false,
            expects_response: Some(timeout),
            ipc: request_ipc,
            metadata: request_metadata,
        },
        match payload {
            None => None,
            Some(ref p) => Some(p),
        },
    ).unwrap() else {
        panic!("");
    };
    (ipc, metadata)
}

fn handle_message (
    our: &wit::Address,
    db_handle: &mut Option<redb::Database>,
) -> anyhow::Result<()> {
    let (source, message) = wit::receive().unwrap();

    if our.node != source.node {
        return Err(kv::KeyValueError::RejectForeign.into());
    }

    match message {
        wit::Message::Response(_) => { unimplemented!() },
        wit::Message::Request(wit::Request { ipc, .. }) => {
            match serde_json::from_slice(&ipc)? {
                kv::KeyValueMessage::New { db } => {
                    let vfs_drive = format!("{}{}", PREFIX, db);
                    match db_handle {
                        Some(_) => {
                            return Err(kv::KeyValueError::DbAlreadyExists.into());
                        },
                        None => {
                            wit::print_to_terminal(1, "key_value_worker: Create");
                            *db_handle = Some(redb::Database::create(
                                format!("/{}.redb", db),
                                our.node.clone(),
                                vfs_drive,
                                get_payload_wrapped,
                                send_and_await_response_wrapped,
                            )?);
                            wit::print_to_terminal(1, "key_value_worker: Create done");
                        },
                    }
                },
                kv::KeyValueMessage::Write { ref key, .. } => {
                    let Some(db_handle) = db_handle else {
                        return Err(kv::KeyValueError::DbDoesNotExist.into());
                    };

                    let wit::Payload { ref bytes, .. } = wit::get_payload()
                        .ok_or(anyhow::anyhow!("couldnt get bytes for Write"))?;

                    let write_txn = db_handle.begin_write()?;
                    {
                        let mut table = write_txn.open_table(TABLE)?;
                        table.insert(&key[..], &bytes[..])?;
                    }
                    write_txn.commit()?;

                    Response::new()
                        .ipc_bytes(ipc)
                        .send()?;
                },
                kv::KeyValueMessage::Read { ref key, .. } => {
                    let Some(db_handle) = db_handle else {
                        return Err(kv::KeyValueError::DbDoesNotExist.into());
                    };

                    let read_txn = db_handle.begin_read()?;

                    let table = read_txn.open_table(TABLE)?;

                    match table.get(&key[..])? {
                        None => {
                            Response::new()
                                .ipc_bytes(ipc)
                                .send()?;
                        },
                        Some(v) => {
                            let bytes = v.value().to_vec();
                            wit::print_to_terminal(
                                1,
                                &format!(
                                    "key_value_worker: key, val: {:?}, {}",
                                    key,
                                    if bytes.len() < 100 {
                                        format!("{:?}", bytes)
                                    } else {
                                        "<elided>".into()
                                    },
                                ),
                            );
                            Response::new()
                                .ipc_bytes(ipc)
                                .payload(wit::Payload {
                                    mime: None,
                                    bytes,
                                })
                                .send()?;
                        },
                    };
                },
                kv::KeyValueMessage::Err { error } => {
                    return Err(error.into());
                }
            }

            Ok(())
        },
    }
}

struct Component;
impl Guest for Component {
    fn init(our: String) {
        wit::print_to_terminal(1, "key_value_worker: begin");

        let our = Address::from_str(&our).unwrap();
        let mut db_handle: Option<redb::Database> = None;

        let vfs_address = ProcessId::from_str("vfs:sys:uqbar").unwrap();
        create_capability(
            &vfs_address,
            &"\"messaging\"".into(),
        );

        loop {
            match handle_message(&our, &mut db_handle) {
                Ok(()) => {},
                Err(e) => {
                    wit::print_to_terminal(0, format!(
                        "key_value_worker: error: {:?}",
                        e,
                    ).as_str());
                    if let Some(e) = e.downcast_ref::<kv::KeyValueError>() {
                        Response::new()
                            .ipc_bytes(serde_json::to_vec(&e).unwrap())
                            .send()
                            .unwrap();
                    }
                    panic!("");
                },
            };
        }
    }
}
