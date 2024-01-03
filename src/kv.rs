use anyhow::Result;
use dashmap::DashMap;
// use rocksdb::checkpoint::Checkpoint;
use rocksdb::OptimisticTransactionDB;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::Mutex;

use crate::types::*;

pub async fn kv(
    our_node: String,
    send_to_loop: MessageSender,
    send_to_terminal: PrintSender,
    mut recv_from_loop: MessageReceiver,
    send_to_caps_oracle: CapMessageSender,
    home_directory_path: String,
) -> anyhow::Result<()> {
    let kv_path = format!("{}/kv", &home_directory_path);

    if let Err(e) = fs::create_dir_all(&kv_path).await {
        panic!("failed creating kv dir! {:?}", e);
    }

    let open_kvs: Arc<DashMap<(PackageId, String), OptimisticTransactionDB>> =
        Arc::new(DashMap::new());
    let txs: Arc<DashMap<u64, Vec<(KvAction, Option<Vec<u8>>)>>> = Arc::new(DashMap::new());

    let mut process_queues: HashMap<ProcessId, Arc<Mutex<VecDeque<KernelMessage>>>> =
        HashMap::new();

    loop {
        tokio::select! {
            Some(km) = recv_from_loop.recv() => {
                if our_node.clone() != km.source.node {
                    println!(
                        "kv: request must come from our_node={}, got: {}",
                        our_node,
                        km.source.node,
                    );
                    continue;
                }

                let queue = process_queues
                    .entry(km.source.process.clone())
                    .or_insert_with(|| Arc::new(Mutex::new(VecDeque::new())))
                    .clone();

                {
                    let mut queue_lock = queue.lock().await;
                    queue_lock.push_back(km.clone());
                }

                // clone Arcs
                let our_node = our_node.clone();
                let send_to_caps_oracle = send_to_caps_oracle.clone();
                let send_to_terminal = send_to_terminal.clone();
                let send_to_loop = send_to_loop.clone();
                let open_kvs = open_kvs.clone();
                let txs = txs.clone();
                let kv_path = kv_path.clone();

                tokio::spawn(async move {
                    let mut queue_lock = queue.lock().await;
                    if let Some(km) = queue_lock.pop_front() {
                        if let Err(e) = handle_request(
                            our_node.clone(),
                            km.clone(),
                            open_kvs.clone(),
                            txs.clone(),
                            send_to_loop.clone(),
                            send_to_terminal.clone(),
                            send_to_caps_oracle.clone(),
                            kv_path.clone(),
                        )
                        .await
                        {
                            let _ = send_to_loop
                                .send(make_error_message(our_node.clone(), &km, e))
                                .await;
                        }
                    }
                });
            }
        }
    }
}

async fn handle_request(
    our_node: String,
    km: KernelMessage,
    open_kvs: Arc<DashMap<(PackageId, String), OptimisticTransactionDB>>,
    txs: Arc<DashMap<u64, Vec<(KvAction, Option<Vec<u8>>)>>>,
    send_to_loop: MessageSender,
    send_to_terminal: PrintSender,
    send_to_caps_oracle: CapMessageSender,
    kv_path: String,
) -> Result<(), KvError> {
    let KernelMessage {
        id,
        source,
        message,
        payload,
        ..
    } = km.clone();
    let Message::Request(Request {
        ipc,
        expects_response,
        metadata,
        ..
    }) = message.clone()
    else {
        return Err(KvError::InputError {
            error: "not a request".into(),
        });
    };

    let request: KvRequest = match serde_json::from_slice(&ipc) {
        Ok(r) => r,
        Err(e) => {
            println!("kv: got invalid Request: {}", e);
            return Err(KvError::InputError {
                error: "didn't serialize to KvAction.".into(),
            });
        }
    };

    check_caps(
        our_node.clone(),
        source.clone(),
        open_kvs.clone(),
        send_to_caps_oracle.clone(),
        &request,
        kv_path.clone(),
    )
    .await?;

    let (ipc, bytes) = match &request.action {
        KvAction::Open => {
            // handled in check_caps.
            (serde_json::to_vec(&KvResponse::Ok).unwrap(), None)
        }
        KvAction::RemoveDb => {
            // handled in check_caps.
            (serde_json::to_vec(&KvResponse::Ok).unwrap(), None)
        }
        KvAction::Get { key } => {
            let db = match open_kvs.get(&(request.package_id, request.db)) {
                None => {
                    return Err(KvError::NoDb);
                }
                Some(db) => db,
            };

            match db.get(&key) {
                Ok(Some(value)) => (
                    serde_json::to_vec(&KvResponse::Get { key: key.to_vec() }).unwrap(),
                    Some(value),
                ),
                Ok(None) => {
                    return Err(KvError::KeyNotFound);
                }
                Err(e) => {
                    return Err(KvError::RocksDBError {
                        action: request.action.to_string(),
                        error: e.to_string(),
                    })
                }
            }
        }
        KvAction::BeginTx => {
            let tx_id = rand::random::<u64>();
            txs.insert(tx_id, Vec::new());
            (
                serde_json::to_vec(&KvResponse::BeginTx { tx_id }).unwrap(),
                None,
            )
        }
        KvAction::Set { key, tx_id } => {
            let db = match open_kvs.get(&(request.package_id, request.db)) {
                None => {
                    return Err(KvError::NoDb);
                }
                Some(db) => db,
            };
            let Some(payload) = payload else {
                return Err(KvError::InputError {
                    error: "no payload".into(),
                });
            };

            match tx_id {
                None => {
                    db.put(key, payload.bytes)?;
                }
                Some(tx_id) => {
                    let mut tx = match txs.get_mut(&tx_id) {
                        None => {
                            return Err(KvError::NoTx);
                        }
                        Some(tx) => tx,
                    };
                    tx.push((request.action.clone(), Some(payload.bytes)));
                }
            }

            (serde_json::to_vec(&KvResponse::Ok).unwrap(), None)
        }
        KvAction::Delete { key, tx_id } => {
            let db = match open_kvs.get(&(request.package_id, request.db)) {
                None => {
                    return Err(KvError::NoDb);
                }
                Some(db) => db,
            };
            match tx_id {
                None => {
                    db.delete(key)?;
                }
                Some(tx_id) => {
                    let mut tx = match txs.get_mut(&tx_id) {
                        None => {
                            return Err(KvError::NoTx);
                        }
                        Some(tx) => tx,
                    };
                    tx.push((request.action.clone(), None));
                }
            }
            (serde_json::to_vec(&KvResponse::Ok).unwrap(), None)
        }
        KvAction::Commit { tx_id } => {
            let db = match open_kvs.get(&(request.package_id, request.db)) {
                None => {
                    return Err(KvError::NoDb);
                }
                Some(db) => db,
            };

            let txs = match txs.remove(&tx_id).map(|(_, tx)| tx) {
                None => {
                    return Err(KvError::NoTx);
                }
                Some(tx) => tx,
            };
            let tx = db.transaction();

            for (action, payload) in txs {
                match action {
                    KvAction::Set { key, .. } => {
                        if let Some(payload) = payload {
                            tx.put(&key, &payload)?;
                        }
                    }
                    KvAction::Delete { key, .. } => {
                        tx.delete(&key)?;
                    }
                    _ => {}
                }
            }

            match tx.commit() {
                Ok(_) => (serde_json::to_vec(&KvResponse::Ok).unwrap(), None),
                Err(e) => {
                    return Err(KvError::RocksDBError {
                        action: request.action.to_string(),
                        error: e.to_string(),
                    })
                }
            }
        }
        KvAction::Backup => {
            // looping through open dbs and flushing their memtables
            for db_ref in open_kvs.iter() {
                let db = db_ref.value();
                db.flush()?;
            }
            (serde_json::to_vec(&KvResponse::Ok).unwrap(), None)
        }
    };

    if let Some(target) = km.rsvp.or_else(|| {
        expects_response.map(|_| Address {
            node: our_node.clone(),
            process: source.process.clone(),
        })
    }) {
        let response = KernelMessage {
            id,
            source: Address {
                node: our_node.clone(),
                process: KV_PROCESS_ID.clone(),
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
    } else {
        send_to_terminal
            .send(Printout {
                verbosity: 2,
                content: format!(
                    "kv: not sending response: {:?}",
                    serde_json::from_slice::<KvResponse>(&ipc)
                ),
            })
            .await
            .unwrap();
    }

    Ok(())
}

async fn check_caps(
    our_node: String,
    source: Address,
    open_kvs: Arc<DashMap<(PackageId, String), OptimisticTransactionDB>>,
    mut send_to_caps_oracle: CapMessageSender,
    request: &KvRequest,
    kv_path: String,
) -> Result<(), KvError> {
    let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
    let src_package_id = PackageId::new(source.process.package(), source.process.publisher());

    match &request.action {
        KvAction::Delete { .. }
        | KvAction::Set { .. }
        | KvAction::BeginTx
        | KvAction::Commit { .. } => {
            send_to_caps_oracle
                .send(CapMessage::Has {
                    on: source.process.clone(),
                    cap: Capability {
                        issuer: Address {
                            node: our_node.clone(),
                            process: KV_PROCESS_ID.clone(),
                        },
                        params: serde_json::to_string(&serde_json::json!({
                            "kind": "write",
                            "db": request.db.to_string(),
                        }))
                        .unwrap(),
                    },
                    responder: send_cap_bool,
                })
                .await?;
            let has_cap = recv_cap_bool.await?;
            if !has_cap {
                return Err(KvError::NoCap {
                    error: request.action.to_string(),
                });
            }
            Ok(())
        }
        KvAction::Get { .. } => {
            send_to_caps_oracle
                .send(CapMessage::Has {
                    on: source.process.clone(),
                    cap: Capability {
                        issuer: Address {
                            node: our_node.clone(),
                            process: KV_PROCESS_ID.clone(),
                        },
                        params: serde_json::to_string(&serde_json::json!({
                            "kind": "read",
                            "db": request.db.to_string(),
                        }))
                        .unwrap(),
                    },
                    responder: send_cap_bool,
                })
                .await?;
            let has_cap = recv_cap_bool.await?;
            if !has_cap {
                return Err(KvError::NoCap {
                    error: request.action.to_string(),
                });
            }
            Ok(())
        }
        KvAction::Open { .. } => {
            if src_package_id != request.package_id {
                return Err(KvError::NoCap {
                    error: request.action.to_string(),
                });
            }

            add_capability(
                "read",
                &request.db.to_string(),
                &our_node,
                &source,
                &mut send_to_caps_oracle,
            )
            .await?;
            add_capability(
                "write",
                &request.db.to_string(),
                &our_node,
                &source,
                &mut send_to_caps_oracle,
            )
            .await?;

            if open_kvs.contains_key(&(request.package_id.clone(), request.db.clone())) {
                return Ok(());
            }

            let db_path = format!(
                "{}/{}/{}",
                kv_path,
                request.package_id.to_string(),
                request.db.to_string()
            );
            fs::create_dir_all(&db_path).await?;

            let db = OptimisticTransactionDB::open_default(&db_path)?;

            open_kvs.insert((request.package_id.clone(), request.db.clone()), db);
            Ok(())
        }
        KvAction::RemoveDb { .. } => {
            if src_package_id != request.package_id {
                return Err(KvError::NoCap {
                    error: request.action.to_string(),
                });
            }

            let db_path = format!(
                "{}/{}/{}",
                kv_path,
                request.package_id.to_string(),
                request.db.to_string()
            );
            open_kvs.remove(&(request.package_id.clone(), request.db.clone()));

            fs::remove_dir_all(&db_path).await?;
            Ok(())
        }
        KvAction::Backup { .. } => Ok(()),
    }
}

async fn add_capability(
    kind: &str,
    db: &str,
    our_node: &str,
    source: &Address,
    send_to_caps_oracle: &mut CapMessageSender,
) -> Result<(), KvError> {
    let cap = Capability {
        issuer: Address {
            node: our_node.to_string(),
            process: KV_PROCESS_ID.clone(),
        },
        params: serde_json::to_string(&serde_json::json!({ "kind": kind, "db": db })).unwrap(),
    };
    let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
    send_to_caps_oracle
        .send(CapMessage::Add {
            on: source.process.clone(),
            caps: vec![cap],
            responder: send_cap_bool,
        })
        .await?;
    let _ = recv_cap_bool.await?;
    Ok(())
}

fn make_error_message(our_name: String, km: &KernelMessage, error: KvError) -> KernelMessage {
    KernelMessage {
        id: km.id,
        source: Address {
            node: our_name.clone(),
            process: KV_PROCESS_ID.clone(),
        },
        target: match &km.rsvp {
            None => km.source.clone(),
            Some(rsvp) => rsvp.clone(),
        },
        rsvp: None,
        message: Message::Response((
            Response {
                inherit: false,
                ipc: serde_json::to_vec(&KvResponse::Err { error: error }).unwrap(),
                metadata: None,
            },
            None,
        )),
        payload: None,
        signed_capabilities: None,
    }
}

impl std::fmt::Display for KvAction {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<tokio::sync::oneshot::error::RecvError> for KvError {
    fn from(err: tokio::sync::oneshot::error::RecvError) -> Self {
        KvError::NoCap {
            error: err.to_string(),
        }
    }
}

impl From<tokio::sync::mpsc::error::SendError<CapMessage>> for KvError {
    fn from(err: tokio::sync::mpsc::error::SendError<CapMessage>) -> Self {
        KvError::NoCap {
            error: err.to_string(),
        }
    }
}

impl From<std::io::Error> for KvError {
    fn from(err: std::io::Error) -> Self {
        KvError::IOError {
            error: err.to_string(),
        }
    }
}
impl From<rocksdb::Error> for KvError {
    fn from(error: rocksdb::Error) -> Self {
        KvError::RocksDBError {
            action: "".into(),
            error: error.to_string(),
        }
    }
}
