use crate::vfs::UniqueQueue;
use dashmap::DashMap;
use lib::types::core::{
    Address, CapMessage, CapMessageSender, Capability, FdManagerRequest, KernelMessage, KvAction,
    KvCapabilityKind, KvCapabilityParams, KvError, KvRequest, KvResponse, LazyLoadBlob, Message,
    MessageReceiver, MessageSender, PackageId, PrintSender, Printout, ProcessId, Request, Response,
    FD_MANAGER_PROCESS_ID, KV_PROCESS_ID,
};
use rocksdb::OptimisticTransactionDB;
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::Arc,
};
use tokio::{fs, sync::Mutex};

#[derive(Clone)]
struct KvState {
    our: Arc<Address>,
    kv_path: Arc<PathBuf>,
    send_to_loop: MessageSender,
    send_to_terminal: PrintSender,
    open_kvs: Arc<DashMap<(PackageId, String), OptimisticTransactionDB>>,
    /// access order of dbs, used to cull if we hit the fds limit
    access_order: Arc<Mutex<UniqueQueue<(PackageId, String)>>>,
    txs: Arc<DashMap<u64, Vec<(KvAction, Option<Vec<u8>>)>>>,
    fds_limit: u64,
}

impl KvState {
    pub fn new(
        our: Address,
        send_to_terminal: PrintSender,
        send_to_loop: MessageSender,
        home_directory_path: PathBuf,
    ) -> Self {
        Self {
            our: Arc::new(our),
            kv_path: Arc::new(home_directory_path.join("kv")),
            send_to_loop,
            send_to_terminal,
            open_kvs: Arc::new(DashMap::new()),
            access_order: Arc::new(Mutex::new(UniqueQueue::new())),
            txs: Arc::new(DashMap::new()),
            fds_limit: 10,
        }
    }

    pub async fn open_db(&mut self, key: &(PackageId, String)) -> Result<(), KvError> {
        if self.open_kvs.contains_key(key) {
            let mut access_order = self.access_order.lock().await;
            access_order.remove(key);
            access_order.push_back(key.clone());
            return Ok(());
        }

        if self.open_kvs.len() as u64 >= self.fds_limit {
            // close least recently used db
            let to_close = self.access_order.lock().await.pop_front().unwrap();
            self.remove_db(&to_close).await;
        }

        #[cfg(unix)]
        let db_path = self.kv_path.join(format!("{}", key.0)).join(&key.1);
        #[cfg(target_os = "windows")]
        let db_path = self
            .kv_path
            .join(format!("{}_{}", key.0._package(), key.0._publisher()))
            .join(&key.1);

        fs::create_dir_all(&db_path).await?;

        self.open_kvs.insert(
            key.clone(),
            OptimisticTransactionDB::open_default(&db_path).map_err(rocks_to_kv_err)?,
        );
        let mut access_order = self.access_order.lock().await;
        access_order.push_back(key.clone());
        Ok(())
    }

    pub async fn remove_db(&mut self, key: &(PackageId, String)) {
        self.open_kvs.remove(key);
        let mut access_order = self.access_order.lock().await;
        access_order.remove(key);
    }

    pub async fn remove_least_recently_used_dbs(&mut self, n: u64) {
        for _ in 0..n {
            let mut lock = self.access_order.lock().await;
            let key = lock.pop_front().unwrap();
            drop(lock);
            self.remove_db(&key).await;
        }
    }
}

pub async fn kv(
    our_node: Arc<String>,
    send_to_loop: MessageSender,
    send_to_terminal: PrintSender,
    mut recv_from_loop: MessageReceiver,
    send_to_caps_oracle: CapMessageSender,
    home_directory_path: PathBuf,
) -> anyhow::Result<()> {
    let our = Address::new(our_node.as_str(), KV_PROCESS_ID.clone());

    crate::fd_manager::send_fd_manager_request_fds_limit(&our, &send_to_loop).await;

    let mut state = KvState::new(our, send_to_terminal, send_to_loop, home_directory_path);

    if let Err(e) = fs::create_dir_all(&*state.kv_path).await {
        panic!("failed creating kv dir! {e:?}");
    }

    let process_queues: HashMap<ProcessId, Arc<Mutex<VecDeque<KernelMessage>>>> = HashMap::new();

    while let Some(km) = recv_from_loop.recv().await {
        if state.our.node != km.source.node {
            Printout::new(
                1,
                KV_PROCESS_ID.clone(),
                format!(
                    "kv: got request from {}, but requests must come from our node {}",
                    km.source.node, state.our.node,
                ),
            )
            .send(&state.send_to_terminal)
            .await;
            continue;
        }

        if km.source.process == *FD_MANAGER_PROCESS_ID {
            if let Err(e) = handle_fd_request(km, &mut state).await {
                Printout::new(
                    1,
                    KV_PROCESS_ID.clone(),
                    format!("kv: got request from fd-manager that errored: {e:?}"),
                )
                .send(&state.send_to_terminal)
                .await;
            };
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
        let mut state = state.clone();
        let send_to_caps_oracle = send_to_caps_oracle.clone();

        tokio::spawn(async move {
            let mut queue_lock = queue.lock().await;
            if let Some(km) = queue_lock.pop_front() {
                let (km_id, km_rsvp) =
                    (km.id.clone(), km.rsvp.clone().unwrap_or(km.source.clone()));

                if let Err(e) = handle_request(km, &mut state, &send_to_caps_oracle).await {
                    Printout::new(1, KV_PROCESS_ID.clone(), format!("kv: {e}"))
                        .send(&state.send_to_terminal)
                        .await;
                    KernelMessage::builder()
                        .id(km_id)
                        .source(state.our.as_ref().clone())
                        .target(km_rsvp)
                        .message(Message::Response((
                            Response {
                                inherit: false,
                                body: serde_json::to_vec(&KvResponse::Err(e)).unwrap(),
                                metadata: None,
                                capabilities: vec![],
                            },
                            None,
                        )))
                        .build()
                        .unwrap()
                        .send(&state.send_to_loop)
                        .await;
                }
            }
        });
    }
    Ok(())
}

async fn handle_request(
    km: KernelMessage,
    state: &mut KvState,
    send_to_caps_oracle: &CapMessageSender,
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
        // we got a response -- safe to ignore
        return Ok(());
    };

    let request: KvRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            println!("kv: got invalid request: {e}");
            return Err(KvError::MalformedRequest);
        }
    };

    let db_key = (request.package_id, request.db);

    check_caps(
        &source,
        state,
        send_to_caps_oracle,
        &request.action,
        &db_key,
    )
    .await?;

    // always open to ensure db exists
    state.open_db(&db_key).await?;

    let (body, bytes) = match request.action {
        KvAction::Open => {
            // handled in check_caps.
            (serde_json::to_vec(&KvResponse::Ok).unwrap(), None)
        }
        KvAction::RemoveDb => {
            // handled in check_caps.
            (serde_json::to_vec(&KvResponse::Ok).unwrap(), None)
        }
        KvAction::Get(key) => {
            let db = match state.open_kvs.get(&db_key) {
                None => {
                    return Err(KvError::NoDb(db_key.0, db_key.1));
                }
                Some(db) => db,
            };

            match db.get(&key) {
                Ok(Some(value)) => (
                    serde_json::to_vec(&KvResponse::Get(key)).unwrap(),
                    Some(value),
                ),
                Ok(None) => {
                    return Err(KvError::KeyNotFound);
                }
                Err(e) => {
                    return Err(rocks_to_kv_err(e));
                }
            }
        }
        KvAction::BeginTx => {
            let tx_id = rand::random::<u64>();
            state.txs.insert(tx_id, Vec::new());
            (
                serde_json::to_vec(&KvResponse::BeginTx { tx_id }).unwrap(),
                None,
            )
        }
        KvAction::Set { ref key, tx_id } => {
            let db = match state.open_kvs.get(&db_key) {
                None => {
                    return Err(KvError::NoDb(db_key.0, db_key.1));
                }
                Some(db) => db,
            };
            let Some(blob) = blob else {
                return Err(KvError::MalformedRequest);
            };

            match tx_id {
                None => {
                    db.put(key, blob.bytes).map_err(rocks_to_kv_err)?;
                }
                Some(tx_id) => {
                    let mut tx = match state.txs.get_mut(&tx_id) {
                        None => {
                            return Err(KvError::NoTx(tx_id));
                        }
                        Some(tx) => tx,
                    };
                    tx.push((request.action, Some(blob.bytes)));
                }
            }

            (serde_json::to_vec(&KvResponse::Ok).unwrap(), None)
        }
        KvAction::Delete { ref key, tx_id } => {
            let db = match state.open_kvs.get(&db_key) {
                None => {
                    return Err(KvError::NoDb(db_key.0, db_key.1));
                }
                Some(db) => db,
            };
            match tx_id {
                None => {
                    db.delete(key).map_err(rocks_to_kv_err)?;
                }
                Some(tx_id) => {
                    let mut tx = match state.txs.get_mut(&tx_id) {
                        None => {
                            return Err(KvError::NoTx(tx_id));
                        }
                        Some(tx) => tx,
                    };
                    tx.push((request.action, None));
                }
            }
            (serde_json::to_vec(&KvResponse::Ok).unwrap(), None)
        }
        KvAction::Commit { tx_id } => {
            let db = match state.open_kvs.get(&db_key) {
                None => {
                    return Err(KvError::NoDb(db_key.0, db_key.1));
                }
                Some(db) => db,
            };

            let txs = match state.txs.remove(&tx_id).map(|(_, tx)| tx) {
                None => {
                    return Err(KvError::NoTx(tx_id));
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
                    return Err(rocks_to_kv_err(e));
                }
            }
        }
    };

    if let Some(target) = km.rsvp.or_else(|| expects_response.map(|_| source)) {
        KernelMessage::builder()
            .id(id)
            .source(state.our.as_ref().clone())
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
            .send(&state.send_to_loop)
            .await;
    }

    Ok(())
}

async fn check_caps(
    source: &Address,
    state: &mut KvState,
    send_to_caps_oracle: &CapMessageSender,
    action: &KvAction,
    db_key: &(PackageId, String),
) -> Result<(), KvError> {
    let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
    let src_package_id = PackageId::new(source.process.package(), source.process.publisher());

    match &action {
        KvAction::Delete { .. }
        | KvAction::Set { .. }
        | KvAction::BeginTx
        | KvAction::Commit { .. } => {
            let Ok(()) = send_to_caps_oracle
                .send(CapMessage::Has {
                    on: source.process.clone(),
                    cap: Capability::new(
                        state.our.as_ref().clone(),
                        serde_json::to_string(&KvCapabilityParams {
                            kind: KvCapabilityKind::Write,
                            db_key: db_key.clone(),
                        })
                        .unwrap(),
                    ),
                    responder: send_cap_bool,
                })
                .await
            else {
                return Err(KvError::NoWriteCap);
            };
            let Ok(true) = recv_cap_bool.await else {
                return Err(KvError::NoWriteCap);
            };
            Ok(())
        }
        KvAction::Get { .. } => {
            let Ok(()) = send_to_caps_oracle
                .send(CapMessage::Has {
                    on: source.process.clone(),
                    cap: Capability::new(
                        state.our.as_ref().clone(),
                        serde_json::to_string(&KvCapabilityParams {
                            kind: KvCapabilityKind::Read,
                            db_key: db_key.clone(),
                        })
                        .unwrap(),
                    ),
                    responder: send_cap_bool,
                })
                .await
            else {
                return Err(KvError::NoReadCap);
            };
            let Ok(true) = recv_cap_bool.await else {
                return Err(KvError::NoReadCap);
            };
            Ok(())
        }
        KvAction::Open { .. } => {
            if src_package_id != db_key.0 {
                return Err(KvError::MismatchingPackageId);
            }

            add_capability(
                KvCapabilityKind::Read,
                &db_key,
                &state.our,
                &source,
                send_to_caps_oracle,
            )
            .await?;
            add_capability(
                KvCapabilityKind::Write,
                &db_key,
                &state.our,
                &source,
                send_to_caps_oracle,
            )
            .await?;

            if state.open_kvs.contains_key(&db_key) {
                return Ok(());
            }

            state.open_db(&db_key).await?;
            Ok(())
        }
        KvAction::RemoveDb { .. } => {
            if src_package_id != db_key.0 {
                return Err(KvError::MismatchingPackageId);
            }

            state.remove_db(&db_key).await;

            #[cfg(unix)]
            let db_path = state.kv_path.join(format!("{}", db_key.0)).join(&db_key.1);
            #[cfg(target_os = "windows")]
            let db_path = state
                .kv_path
                .join(format!("{}_{}", db_key.0._package(), db_key.0._publisher()))
                .join(&db_key.1);

            fs::remove_dir_all(&db_path).await?;

            Ok(())
        }
    }
}

async fn handle_fd_request(km: KernelMessage, state: &mut KvState) -> anyhow::Result<()> {
    let Message::Request(Request { body, .. }) = km.message else {
        return Err(anyhow::anyhow!("not a request"));
    };

    let request: FdManagerRequest = serde_json::from_slice(&body)?;

    match request {
        FdManagerRequest::FdsLimit(new_fds_limit) => {
            state.fds_limit = new_fds_limit;
            if state.open_kvs.len() as u64 >= state.fds_limit {
                crate::fd_manager::send_fd_manager_hit_fds_limit(&state.our, &state.send_to_loop)
                    .await;
                state
                    .remove_least_recently_used_dbs(state.open_kvs.len() as u64 - state.fds_limit)
                    .await;
            }
        }
        _ => {
            return Err(anyhow::anyhow!("non-Cull FdManagerRequest"));
        }
    }

    Ok(())
}

async fn add_capability(
    kind: KvCapabilityKind,
    db_key: &(PackageId, String),
    our: &Address,
    source: &Address,
    send_to_caps_oracle: &CapMessageSender,
) -> Result<(), KvError> {
    let cap = Capability {
        issuer: our.clone(),
        params: serde_json::to_string(&KvCapabilityParams {
            kind,
            db_key: db_key.clone(),
        })
        .unwrap(),
    };
    let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
    let Ok(()) = send_to_caps_oracle
        .send(CapMessage::Add {
            on: source.process.clone(),
            caps: vec![cap],
            responder: Some(send_cap_bool),
        })
        .await
    else {
        return Err(KvError::AddCapFailed);
    };
    let Ok(_) = recv_cap_bool.await else {
        return Err(KvError::AddCapFailed);
    };
    Ok(())
}

fn rocks_to_kv_err(error: rocksdb::Error) -> KvError {
    KvError::RocksDBError(error.to_string())
}
