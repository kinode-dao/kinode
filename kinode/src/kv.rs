use dashmap::DashMap;
use lib::types::core::{
    Address, CapMessage, CapMessageSender, Capability, KernelMessage, KvAction, KvError, KvRequest,
    KvResponse, LazyLoadBlob, Message, MessageReceiver, MessageSender, PackageId, PrintSender,
    Printout, ProcessId, Request, Response, KV_PROCESS_ID,
};
use rocksdb::OptimisticTransactionDB;
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::Arc,
};
use tokio::{fs, sync::Mutex};

pub async fn kv(
    our_node: Arc<String>,
    send_to_loop: MessageSender,
    send_to_terminal: PrintSender,
    mut recv_from_loop: MessageReceiver,
    send_to_caps_oracle: CapMessageSender,
    home_directory_path: PathBuf,
) -> anyhow::Result<()> {
    let kv_path = Arc::new(home_directory_path.join("kv"));
    if let Err(e) = fs::create_dir_all(&*kv_path).await {
        panic!("failed creating kv dir! {e:?}");
    }

    let open_kvs: Arc<DashMap<(PackageId, String), OptimisticTransactionDB>> =
        Arc::new(DashMap::new());
    let txs: Arc<DashMap<u64, Vec<(KvAction, Option<Vec<u8>>)>>> = Arc::new(DashMap::new());

    let process_queues: HashMap<ProcessId, Arc<Mutex<VecDeque<KernelMessage>>>> = HashMap::new();

    while let Some(km) = recv_from_loop.recv().await {
        if *our_node != km.source.node {
            Printout::new(
                1,
                format!(
                    "kv: got request from {}, but requests must come from our node {our_node}",
                    km.source.node
                ),
            )
            .send(&send_to_terminal)
            .await;
            continue;
        }

        let queue = process_queues
            .get(&km.source.process)
            .cloned()
            .unwrap_or_else(|| Arc::new(Mutex::new(VecDeque::new())));

        {
            let mut queue_lock = queue.lock().await;
            queue_lock.push_back(km);
        }

        // clone Arcs
        let our_node = our_node.clone();
        let send_to_loop = send_to_loop.clone();
        let send_to_terminal = send_to_terminal.clone();
        let send_to_caps_oracle = send_to_caps_oracle.clone();
        let open_kvs = open_kvs.clone();
        let txs = txs.clone();
        let kv_path = kv_path.clone();

        tokio::spawn(async move {
            let mut queue_lock = queue.lock().await;
            if let Some(km) = queue_lock.pop_front() {
                let (km_id, km_rsvp) =
                    (km.id.clone(), km.rsvp.clone().unwrap_or(km.source.clone()));

                if let Err(e) = handle_request(
                    &our_node,
                    km,
                    open_kvs,
                    txs,
                    &send_to_loop,
                    &send_to_caps_oracle,
                    &kv_path,
                )
                .await
                {
                    Printout::new(1, format!("kv: {e}"))
                        .send(&send_to_terminal)
                        .await;
                    KernelMessage::builder()
                        .id(km_id)
                        .source((our_node.as_str(), KV_PROCESS_ID.clone()))
                        .target(km_rsvp)
                        .message(Message::Response((
                            Response {
                                inherit: false,
                                body: serde_json::to_vec(&KvResponse::Err { error: e }).unwrap(),
                                metadata: None,
                                capabilities: vec![],
                            },
                            None,
                        )))
                        .build()
                        .unwrap()
                        .send(&send_to_loop)
                        .await;
                }
            }
        });
    }
    Ok(())
}

async fn handle_request(
    our_node: &str,
    km: KernelMessage,
    open_kvs: Arc<DashMap<(PackageId, String), OptimisticTransactionDB>>,
    txs: Arc<DashMap<u64, Vec<(KvAction, Option<Vec<u8>>)>>>,
    send_to_loop: &MessageSender,
    send_to_caps_oracle: &CapMessageSender,
    kv_path: &PathBuf,
) -> Result<(), KvError> {
    let KernelMessage {
        id,
        source,
        message,
        lazy_load_blob: blob,
        ..
    } = km;
    let Message::Request(Request {
        body,
        expects_response,
        metadata,
        ..
    }) = message
    else {
        return Err(KvError::InputError {
            error: "not a request".into(),
        });
    };

    let request: KvRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            println!("kv: got invalid Request: {}", e);
            return Err(KvError::InputError {
                error: "didn't serialize to KvAction.".into(),
            });
        }
    };

    check_caps(
        our_node,
        &source,
        &open_kvs,
        send_to_caps_oracle,
        &request,
        kv_path,
    )
    .await?;

    let (body, bytes) = match &request.action {
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

            match db.get(key) {
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
            let Some(blob) = blob else {
                return Err(KvError::InputError {
                    error: "no blob".into(),
                });
            };

            match tx_id {
                None => {
                    db.put(key, blob.bytes).map_err(rocks_to_kv_err)?;
                }
                Some(tx_id) => {
                    let mut tx = match txs.get_mut(tx_id) {
                        None => {
                            return Err(KvError::NoTx);
                        }
                        Some(tx) => tx,
                    };
                    tx.push((request.action.clone(), Some(blob.bytes)));
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
                    db.delete(key).map_err(rocks_to_kv_err)?;
                }
                Some(tx_id) => {
                    let mut tx = match txs.get_mut(tx_id) {
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

            let txs = match txs.remove(tx_id).map(|(_, tx)| tx) {
                None => {
                    return Err(KvError::NoTx);
                }
                Some(tx) => tx,
            };
            let tx = db.transaction();

            for (action, blob) in txs {
                match action {
                    KvAction::Set { key, .. } => {
                        if let Some(blob) = blob {
                            tx.put(&key, &blob).map_err(rocks_to_kv_err)?;
                        }
                    }
                    KvAction::Delete { key, .. } => {
                        tx.delete(&key).map_err(rocks_to_kv_err)?;
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
                db.flush().map_err(rocks_to_kv_err)?;
            }
            (serde_json::to_vec(&KvResponse::Ok).unwrap(), None)
        }
    };

    if let Some(target) = km.rsvp.or_else(|| expects_response.map(|_| source)) {
        KernelMessage::builder()
            .id(id)
            .source((our_node, KV_PROCESS_ID.clone()))
            .target(target)
            .message(Message::Response((
                Response {
                    inherit: false,
                    body,
                    metadata,
                    capabilities: vec![],
                },
                None,
            )))
            .lazy_load_blob(bytes.map(|bytes| LazyLoadBlob {
                mime: Some("application/octet-stream".into()),
                bytes,
            }))
            .build()
            .unwrap()
            .send(send_to_loop)
            .await;
    }

    Ok(())
}

async fn check_caps(
    our_node: &str,
    source: &Address,
    open_kvs: &Arc<DashMap<(PackageId, String), OptimisticTransactionDB>>,
    send_to_caps_oracle: &CapMessageSender,
    request: &KvRequest,
    kv_path: &PathBuf,
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
                            node: our_node.to_string(),
                            process: KV_PROCESS_ID.clone(),
                        },
                        params: serde_json::json!({
                            "kind": "write",
                            "db": request.db.to_string(),
                        })
                        .to_string(),
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
                            node: our_node.to_string(),
                            process: KV_PROCESS_ID.clone(),
                        },
                        params: serde_json::json!({
                            "kind": "read",
                            "db": request.db.to_string(),
                        })
                        .to_string(),
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
                send_to_caps_oracle,
            )
            .await?;
            add_capability(
                "write",
                &request.db.to_string(),
                &our_node,
                &source,
                send_to_caps_oracle,
            )
            .await?;

            if open_kvs.contains_key(&(request.package_id.clone(), request.db.clone())) {
                return Ok(());
            }

            let db_path = kv_path
                .join(format!("{}", request.package_id))
                .join(&request.db);
            fs::create_dir_all(&db_path).await?;

            let db = OptimisticTransactionDB::open_default(&db_path).map_err(rocks_to_kv_err)?;

            open_kvs.insert((request.package_id.clone(), request.db.clone()), db);
            Ok(())
        }
        KvAction::RemoveDb { .. } => {
            if src_package_id != request.package_id {
                return Err(KvError::NoCap {
                    error: request.action.to_string(),
                });
            }

            let db_path = kv_path
                .join(format!("{}", request.package_id))
                .join(&request.db);
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
    send_to_caps_oracle: &CapMessageSender,
) -> Result<(), KvError> {
    let cap = Capability {
        issuer: Address {
            node: our_node.to_string(),
            process: KV_PROCESS_ID.clone(),
        },
        params: serde_json::json!({ "kind": kind, "db": db }).to_string(),
    };
    let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
    send_to_caps_oracle
        .send(CapMessage::Add {
            on: source.process.clone(),
            caps: vec![cap],
            responder: Some(send_cap_bool),
        })
        .await?;
    let _ = recv_cap_bool.await?;
    Ok(())
}

fn rocks_to_kv_err(error: rocksdb::Error) -> KvError {
    KvError::RocksDBError {
        action: "".into(),
        error: error.to_string(),
    }
}
