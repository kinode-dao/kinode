use anyhow::Result;
use rocksdb::checkpoint::Checkpoint;
use rocksdb::{Options, DB};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::fs;

use crate::types::*;

pub async fn load_state(
    our_name: String,
    home_directory_path: String,
    runtime_extensions: Vec<(ProcessId, MessageSender, bool)>,
) -> Result<(ProcessMap, DB, Vec<KernelMessage>), StateError> {
    let state_path = format!("{}/kernel", &home_directory_path);

    if let Err(e) = fs::create_dir_all(&state_path).await {
        panic!("failed creating kernel state dir! {:?}", e);
    }

    // more granular kernel_state in column families

    // let mut options = Option::default().unwrap();
    // options.create_if_missing(true);
    //let db = DB::open_default(&state_directory_path_str).unwrap();
    let mut opts = Options::default();
    opts.create_if_missing(true);
    // let cf_name = "kernel_state";
    // let cf_descriptor = ColumnFamilyDescriptor::new(cf_name, Options::default());
    let mut db = DB::open_default(state_path).unwrap();
    let mut process_map: ProcessMap = HashMap::new();

    let vfs_messages = vec![];
    println!("booted process map: {:?}", process_map);
    Ok((process_map, db, vfs_messages))
}

pub async fn state_sender(
    our_name: String,
    send_to_loop: MessageSender,
    send_to_terminal: PrintSender,
    mut recv_state: MessageReceiver,
    db: DB,
    home_directory_path: String,
    // mut recv_kill: Receiver<()>,
    // send_kill_confirm: Sender<()>,
) -> Result<(), anyhow::Error> {
    let db = Arc::new(db);
    //  into main loop

    loop {
        tokio::select! {
            Some(km) = recv_state.recv() => {
                if our_name != km.source.node {
                    println!(
                        "fs: request must come from our_name={}, got: {}",
                        our_name, &km,
                    );
                    continue;
                }
                let db_clone = db.clone();
                let send_to_loop = send_to_loop.clone();
                let send_to_terminal = send_to_terminal.clone();
                let our_name = our_name.clone();
                let home_directory_path = home_directory_path.clone();

                tokio::spawn(async move {
                    if let Err(e) = handle_request(
                            our_name.clone(),
                            km.clone(),
                            db_clone,
                            send_to_loop.clone(),
                            send_to_terminal,
                            home_directory_path,
                        )
                        .await
                        {
                            let _ = send_to_loop
                                .send(make_error_message(our_name.clone(), &km, e))
                                .await;
                        }
                });
            }
        }
    }
}

async fn handle_request(
    our_name: String,
    kernel_message: KernelMessage,
    db: Arc<DB>,
    send_to_loop: MessageSender,
    _send_to_terminal: PrintSender,
    home_directory_path: String,
) -> Result<(), StateError> {
    let KernelMessage {
        id,
        source,
        rsvp,
        message,
        payload,
        ..
    } = kernel_message;
    let Message::Request(Request {
        expects_response,
        ipc,
        metadata, // for kernel
        ..
    }) = message
    else {
        return Err(StateError::BadRequest {
            error: "not a request".into(),
        });
    };

    let action: StateAction = match serde_json::from_slice(&ipc) {
        Ok(r) => r,
        Err(e) => {
            return Err(StateError::BadJson {
                error: format!("parse into StateAction failed: {:?}", e),
            })
        }
    };

    let (ipc, bytes) = match action {
        StateAction::SetState(process_id) => {
            let key = process_id.to_hash();
            // TODO consistency with to_stirngs
            let Some(ref payload) = payload else {
                return Err(StateError::BadBytes {
                    action: "SetState".into(),
                });
            };

            db.put(key, &payload.bytes)?;
            (serde_json::to_vec(&StateResponse::SetState).unwrap(), None)
        }
        StateAction::GetState(process_id) => {
            let key = process_id.to_hash();
            match db.get(key) {
                Ok(Some(value)) => {
                    println!("found value");
                    (
                        serde_json::to_vec(&StateResponse::GetState).unwrap(),
                        Some(value),
                    )
                }
                Ok(None) => {
                    println!("nothing found");
                    return Err(StateError::NotFound {
                        process_id: process_id.clone(),
                    });
                }
                Err(e) => {
                    println!("get state error: {:?}", e);
                    return Err(StateError::RocksDBError {
                        action: "GetState".into(),
                        error: e.to_string(),
                    });
                }
            }
        }
        StateAction::DeleteState(process_id) => {
            // handle DeleteState action
            println!("got deleteState");
            let key = process_id.to_hash();
            match db.delete(key) {
                Ok(_) => {
                    println!("delete state success");
                    (
                        serde_json::to_vec(&StateResponse::DeleteState).unwrap(),
                        None,
                    )
                }
                Err(e) => {
                    println!("delete state error: {:?}", e);
                    return Err(StateError::RocksDBError {
                        action: "DeleteState".into(),
                        error: e.to_string(),
                    });
                }
            }
        }
        StateAction::Backup => {
            // handle Backup action
            println!("got backup");
            let checkpoint_dir = format!("{}/kernel/checkpoint", &home_directory_path);

            if Path::new(&checkpoint_dir).exists() {
                let _ = fs::remove_dir_all(&checkpoint_dir).await;
            }
            let checkpoint = Checkpoint::new(&db).unwrap();
            checkpoint.create_checkpoint(&checkpoint_dir).unwrap();
            (serde_json::to_vec(&StateResponse::Backup).unwrap(), None)
        }
    };

    if let Some(target) = rsvp.or_else(|| {
        expects_response.map(|_| Address {
            node: our_name.clone(),
            process: source.process.clone(),
        })
    }) {
        let response = KernelMessage {
            id,
            source: Address {
                node: our_name.clone(),
                process: STATE_PROCESS_ID.clone(),
            },
            target,
            rsvp: None,
            message: Message::Response((
                Response {
                    inherit: false,
                    ipc,
                    metadata,
                },
                None,
            )),
            payload: bytes.map(|bytes| Payload {
                mime: Some("application/octet-stream".into()),
                bytes,
            }),
            signed_capabilities: None,
        };

        let _ = send_to_loop.send(response).await;
    };

    Ok(())
}

fn make_error_message(our_name: String, km: &KernelMessage, error: StateError) -> KernelMessage {
    KernelMessage {
        id: km.id,
        source: Address {
            node: our_name.clone(),
            process: STATE_PROCESS_ID.clone(),
        },
        target: match &km.rsvp {
            None => km.source.clone(),
            Some(rsvp) => rsvp.clone(),
        },
        rsvp: None,
        message: Message::Response((
            Response {
                inherit: false,
                ipc: serde_json::to_vec(&StateResponse::Err(error)).unwrap(),
                metadata: None,
            },
            None,
        )),
        payload: None,
        signed_capabilities: None,
    }
}

impl From<std::io::Error> for VfsError {
    fn from(err: std::io::Error) -> Self {
        VfsError::IOError {
            error: err.to_string(),
            path: "".to_string(),
        } // replace with appropriate VfsError variant and fields
    }
}
impl From<rocksdb::Error> for StateError {
    fn from(error: rocksdb::Error) -> Self {
        StateError::RocksDBError {
            action: "ass".into(),
            error: error.to_string(),
        }
    }
}
