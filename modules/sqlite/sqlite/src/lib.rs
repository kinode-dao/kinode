use std::collections::{HashMap, HashSet};

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

mod sqlite_types;
use sqlite_types as sq;

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
    ipc: Vec<u8>,
    db_to_process: &mut DbToProcess,
) -> anyhow::Result<()> {
    if wit::has_capability(&make_db_cap(operation_type, db)) {
        //  forward
        let Some(process_id) = db_to_process.get(db) else {
            return Err(sq::SqliteError::DbDoesNotExist.into());
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
        return Err(sq::SqliteError::NoCap.into());
    }
}

fn handle_message (
    our: &Address,
    db_to_process: &mut DbToProcess,
    read_keywords: &HashSet<String>,
    write_keywords: &HashSet<String>,
) -> anyhow::Result<()> {
    let (source, message) = wit::receive().unwrap();

    if our.node != source.node {
        return Err(sq::SqliteError::RejectForeign.into());
    }

    match message {
        wit::Message::Response(_) => {
            return Err(sq::SqliteError::UnexpectedResponse.into());
        },
        wit::Message::Request(wit::Request { ipc, .. }) => {
            match serde_json::from_slice(&ipc)? {
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
                        process: ProcessId::new("vfs", "sys", "uqbar"),
                    };
                    let vfs_drive = format!("{}{}", PREFIX, db);
                    let _ = Request::new()
                        .target(vfs_address.clone())?
                        .ipc_bytes(serde_json::to_vec(&kt::VfsRequest {
                            drive: vfs_drive.clone(),
                            action: kt::VfsAction::New,
                        })?)
                        .send_and_await_response(15)??;

                    //  (2)
                    let vfs_read = wit::get_capability(
                        &vfs_address,
                        &make_vfs_cap("read", &vfs_drive),
                    ).ok_or(anyhow::anyhow!("New failed: no vfs 'read' capability found"))?;
                    let vfs_write = wit::get_capability(
                        &vfs_address,
                        &make_vfs_cap("write", &vfs_drive),
                    ).ok_or(anyhow::anyhow!("New failed: no vfs 'write' capability found"))?;
                    let spawned_process_id = match wit::spawn(
                        None,
                        "/sqlite_worker.wasm",
                        &wit::OnPanic::None,  //  TODO: notify us
                        &wit::Capabilities::Some(vec![vfs_read, vfs_write]),
                        false, // not public
                    ) {
                        Ok(spawned_process_id) => spawned_process_id,
                        Err(e) => {
                            wit::print_to_terminal(0, &format!("couldn't spawn: {}", e));
                            panic!("couldn't spawn");  //  TODO
                        },
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
                },
                sq::SqliteMessage::Write { ref db, ref statement, ref tx_id } => {
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
                sq::SqliteMessage::Commit { ref db, ref tx_id } => {
                    forward_if_have_cap(our, "write", db, ipc, db_to_process)?;
                },
            }

            Ok(())
        },
    }
}

struct Component;
impl Guest for Component {
    fn init(our: String) {
        wit::print_to_terminal(0, "sqlite: begin");

        let our = Address::from_str(&our).unwrap();
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
                    wit::print_to_terminal(0, format!(
                        "sqlite: error: {:?}",
                        e,
                    ).as_str());
                    if let Some(e) = e.downcast_ref::<sq::SqliteError>() {
                        Response::new()
                            .ipc_bytes(serde_json::to_vec(&e).unwrap())
                            .send()
                            .unwrap();
                    }
                },
            };
        }
    }
}
