cargo_component_bindings::generate!();

use std::collections::{HashMap, HashSet};

use bindings::component::uq_process::types::*;
use bindings::{create_capability, get_capability, Guest, has_capability, print_to_terminal, receive, send_request, send_response, spawn};

mod kernel_types;
use kernel_types as kt;
mod sqlite_types;
use sqlite_types as sq;
mod process_lib;

struct Component;

const PREFIX: &str = "sqlite-";

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
            return Err(sq::SqliteError::DbDoesNotExist.into());
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
        return Err(sq::SqliteError::NoCap.into());
    }
}

fn handle_message (
    our: &Address,
    db_to_process: &mut DbToProcess,
    read_keywords: &HashSet<String>,
    write_keywords: &HashSet<String>,
) -> anyhow::Result<()> {
    let (source, message) = receive().unwrap();
    // let (source, message) = receive()?;

    if our.node != source.node {
        return Err(sq::SqliteError::RejectForeign.into());
    }

    match message {
        Message::Response(_) => {
            return Err(sq::SqliteError::UnexpectedResponse.into());
        },
        Message::Request(Request { ipc, .. }) => {
            match process_lib::parse_message_ipc(ipc.clone())? {
                sq::SqliteMessage::New { ref db } => {
                    //  TODO: make atomic
                    //  (1): create vfs drive
                    //  (2): spin up worker, granting vfs caps
                    //  (3): issue new caps
                    //  (4): persist

                    if db_to_process.contains_key(db) {
                        return Err(sq::SqliteError::DbAlreadyExists.into());
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
                        "/sqlite_worker.wasm",
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
                            ipc: ipc.clone(),
                            metadata: None,
                        },
                        None,
                        None,
                    );

                    //  (4)
                    db_to_process.insert(db.into(), spawned_process_id);
                    //  TODO: persistence?

                    send_response(
                        &Response {
                            inherit: false,
                            ipc,
                            metadata: None,
                        },
                        None,
                    );
                },
                sq::SqliteMessage::Write { ref db, ref statement } => {
                    let first_word = statement
                        .split_whitespace()
                        .next()
                        .map(|word| word.to_uppercase())
                        .unwrap_or("".to_string());
                    if !write_keywords.contains(&first_word) {
                        return Err(sq::SqliteError::NotAWriteKeyword.into())
                    }
                    forward_if_have_cap(our, "write", db, ipc, db_to_process)?;
                },
                sq::SqliteMessage::Read { ref db, ref query } => {
                    let first_word = query
                        .split_whitespace()
                        .next()
                        .map(|word| word.to_uppercase())
                        .unwrap_or("".to_string());
                    if !read_keywords.contains(&first_word) {
                        return Err(sq::SqliteError::NotAReadKeyword.into())
                    }
                    forward_if_have_cap(our, "read", db, ipc, db_to_process)?;
                },
            }

            Ok(())
        },
    }
}

impl Guest for Component {
    fn init(our: Address) {
        print_to_terminal(0, "sqlite: begin");

        let mut db_to_process: DbToProcess = HashMap::new();
        let read_keywords: HashSet<String> = [
            "ANALYZE",
            "ATTACH",
            "BEGIN",
            "EXPLAIN",
            "PRAGMA",
            "SELECT",
            "VALUES",
            "WITH",
        ]
            .iter()
            .map(|x| x.to_string())
            .collect();
        let write_keywords: HashSet<String> = [
            "ALTER",
            "ANALYZE",
            "COMMIT",
            "CREATE",
            "DELETE",
            "DETACH",
            "DROP",
            "END",
            "INSERT",
            "REINDEX",
            "RELEASE",
            "RENAME",
            "REPLACE",
            "ROLLBACK",
            "SAVEPOINT",
            "UPDATE",
            "VACUUM",
        ]
            .iter()
            .map(|x| x.to_string())
            .collect();

        loop {
            match handle_message(&our, &mut db_to_process, &read_keywords, &write_keywords) {
                Ok(()) => {},
                Err(e) => {
                    print_to_terminal(0, format!(
                        "sqlite: error: {:?}",
                        e,
                    ).as_str());
                    if let Some(e) = e.downcast_ref::<sq::SqliteError>() {
                        send_response(
                            &Response {
                                inherit: false,
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
