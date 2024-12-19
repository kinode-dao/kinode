use crate::vfs::UniqueQueue;
use dashmap::DashMap;
use lib::types::core::{
    Address, CapMessage, CapMessageSender, Capability, FdManagerRequest, KernelMessage, KvAction,
    KvError, KvRequest, KvResponse, LazyLoadBlob, Message, MessageReceiver, MessageSender,
    PackageId, PrintSender, Printout, ProcessId, Request, Response, FD_MANAGER_PROCESS_ID,
    KV_PROCESS_ID,
};
use rand::random;
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
    /// track active iterators: (package_id, db_name) -> (iterator_id -> current position)
    iterators: Arc<
        DashMap<
            (PackageId, String),
            DashMap<u64, Vec<u8>>, // Store last seen key instead of iterator
        >,
    >,
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
            iterators: Arc::new(DashMap::new()),
            fds_limit: 10,
        }
    }

    pub async fn open_db(&mut self, package_id: PackageId, db: String) -> Result<(), KvError> {
        let key = (package_id.clone(), db.clone());
        if self.open_kvs.contains_key(&key) {
            let mut access_order = self.access_order.lock().await;
            access_order.remove(&key);
            access_order.push_back(key);
            return Ok(());
        }

        if self.open_kvs.len() as u64 >= self.fds_limit {
            // close least recently used db
            let key = self.access_order.lock().await.pop_front().unwrap();
            self.remove_db(key.0, key.1).await;
        }

        #[cfg(unix)]
        let db_path = self.kv_path.join(format!("{package_id}")).join(&db);
        #[cfg(target_os = "windows")]
        let db_path = self
            .kv_path
            .join(format!(
                "{}_{}",
                package_id._package(),
                package_id._publisher()
            ))
            .join(&db);

        fs::create_dir_all(&db_path).await?;

        self.open_kvs.insert(
            key,
            OptimisticTransactionDB::open_default(&db_path).map_err(rocks_to_kv_err)?,
        );
        let mut access_order = self.access_order.lock().await;
        access_order.push_back((package_id, db));
        Ok(())
    }

    pub async fn remove_db(&mut self, package_id: PackageId, db: String) {
        self.open_kvs.remove(&(package_id.clone(), db.to_string()));
        let mut access_order = self.access_order.lock().await;
        access_order.remove(&(package_id, db));
    }

    pub async fn remove_least_recently_used_dbs(&mut self, n: u64) {
        for _ in 0..n {
            let mut lock = self.access_order.lock().await;
            let key = lock.pop_front().unwrap();
            drop(lock);
            self.remove_db(key.0, key.1).await;
        }
    }

    async fn handle_iter_start(
        &mut self,
        package_id: PackageId,
        db: String,
        prefix: Option<Vec<u8>>,
    ) -> Result<u64, KvError> {
        let db_key = (package_id.clone(), db.clone());
        let _db = self.open_kvs.get(&db_key).ok_or(KvError::NoDb)?;

        // Generate a random iterator ID and ensure it's unique
        let iterators = self
            .iterators
            .entry(db_key.clone())
            .or_insert_with(|| DashMap::new());

        let mut iterator_id = random::<u64>();
        while iterators.contains_key(&iterator_id) {
            iterator_id = random::<u64>();
        }

        // Store the starting position (prefix or empty vec for start)
        iterators.insert(iterator_id, prefix.unwrap_or_default());

        Ok(iterator_id)
    }

    async fn handle_iter_next(
        &mut self,
        package_id: PackageId,
        db: String,
        iterator_id: u64,
        count: u64,
    ) -> Result<(Vec<(Vec<u8>, Vec<u8>)>, bool), KvError> {
        let db_key = (package_id.clone(), db.clone());
        let db = self.open_kvs.get(&db_key).ok_or(KvError::NoDb)?;

        let db_iters = self.iterators.get(&db_key).ok_or(KvError::NoDb)?;
        let last_key = db_iters
            .get(&iterator_id)
            .ok_or(KvError::NoIterator)?
            .clone();

        let mut entries = Vec::new();
        let mut done = true;

        // Create a fresh iterator starting from our last position
        let mode = if last_key.is_empty() {
            rocksdb::IteratorMode::Start
        } else {
            rocksdb::IteratorMode::From(&last_key, rocksdb::Direction::Forward)
        };

        let mut iter = db.iterator(mode);
        let mut count_remaining = count;

        while let Some(item) = iter.next() {
            if count_remaining == 0 {
                done = false;
                break;
            }

            match item {
                Ok((key, value)) => {
                    let key_vec = key.to_vec();
                    if !key_vec.starts_with(&last_key) && !last_key.is_empty() {
                        // We've moved past our prefix
                        break;
                    }
                    entries.push((key_vec.clone(), value.to_vec()));
                    if let Some(mut last_key_entry) = db_iters.get_mut(&iterator_id) {
                        *last_key_entry = key_vec;
                    }
                    count_remaining -= 1;
                }
                Err(e) => {
                    return Err(KvError::RocksDBError {
                        action: "iter_next".to_string(),
                        error: e.to_string(),
                    });
                }
            }
        }

        Ok((entries, done))
    }

    async fn handle_iter_close(
        &mut self,
        package_id: PackageId,
        db: String,
        iterator_id: u64,
    ) -> Result<(), KvError> {
        let db_key = (package_id, db);
        if let Some(db_iters) = self.iterators.get_mut(&db_key) {
            db_iters.remove(&iterator_id);
            if db_iters.is_empty() {
                self.iterators.remove(&db_key);
            }
        }
        Ok(())
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

    check_caps(&source, state, send_to_caps_oracle, &request).await?;

    // always open to ensure db exists
    state
        .open_db(request.package_id.clone(), request.db.clone())
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
            let db = match state.open_kvs.get(&(request.package_id, request.db)) {
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
            state.txs.insert(tx_id, Vec::new());
            (
                serde_json::to_vec(&KvResponse::BeginTx { tx_id }).unwrap(),
                None,
            )
        }
        KvAction::Set { key, tx_id } => {
            let db = match state.open_kvs.get(&(request.package_id, request.db)) {
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
                    let mut tx = match state.txs.get_mut(tx_id) {
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
            let db = match state.open_kvs.get(&(request.package_id, request.db)) {
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
                    let mut tx = match state.txs.get_mut(tx_id) {
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
            let db = match state.open_kvs.get(&(request.package_id, request.db)) {
                None => {
                    return Err(KvError::NoDb);
                }
                Some(db) => db,
            };

            let txs = match state.txs.remove(tx_id).map(|(_, tx)| tx) {
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
            for db_ref in state.open_kvs.iter() {
                let db = db_ref.value();
                db.flush().map_err(rocks_to_kv_err)?;
            }
            (serde_json::to_vec(&KvResponse::Ok).unwrap(), None)
        }
        KvAction::IterStart { prefix } => {
            let iterator_id = state
                .handle_iter_start(
                    request.package_id.clone(),
                    request.db.clone(),
                    prefix.clone(),
                )
                .await?;
            (
                serde_json::to_vec(&KvResponse::IterStart { iterator_id }).unwrap(),
                None,
            )
        }
        KvAction::IterNext { iterator_id, count } => {
            let (entries, done) = state
                .handle_iter_next(
                    request.package_id.clone(),
                    request.db.clone(),
                    *iterator_id,
                    *count,
                )
                .await?;
            (
                serde_json::to_vec(&KvResponse::IterNext { done }).unwrap(),
                Some(serde_json::to_vec(&entries).unwrap()),
            )
        }
        KvAction::IterClose { iterator_id } => {
            state
                .handle_iter_close(request.package_id.clone(), request.db.clone(), *iterator_id)
                .await?;
            (serde_json::to_vec(&KvResponse::Ok).unwrap(), None)
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
    request: &KvRequest,
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
                    cap: Capability::new(
                        state.our.as_ref().clone(),
                        serde_json::json!({
                            "kind": "write",
                            "db": request.db.to_string(),
                        })
                        .to_string(),
                    ),
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
                    cap: Capability::new(
                        state.our.as_ref().clone(),
                        serde_json::json!({
                            "kind": "read",
                            "db": request.db.to_string(),
                        })
                        .to_string(),
                    ),
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
                &state.our,
                &source,
                send_to_caps_oracle,
            )
            .await?;
            add_capability(
                "write",
                &request.db.to_string(),
                &state.our,
                &source,
                send_to_caps_oracle,
            )
            .await?;

            if state
                .open_kvs
                .contains_key(&(request.package_id.clone(), request.db.clone()))
            {
                return Ok(());
            }

            state
                .open_db(request.package_id.clone(), request.db.clone())
                .await?;
            Ok(())
        }
        KvAction::RemoveDb { .. } => {
            if src_package_id != request.package_id {
                return Err(KvError::NoCap {
                    error: request.action.to_string(),
                });
            }

            state
                .remove_db(request.package_id.clone(), request.db.clone())
                .await;

            #[cfg(unix)]
            let db_path = state
                .kv_path
                .join(format!("{}", request.package_id))
                .join(&request.db);
            #[cfg(target_os = "windows")]
            let db_path = state
                .kv_path
                .join(format!(
                    "{}_{}",
                    request.package_id._package(),
                    request.package_id._publisher()
                ))
                .join(&request.db);

            fs::remove_dir_all(&db_path).await?;

            Ok(())
        }
        KvAction::Backup { .. } => Ok(()),
        KvAction::IterStart { .. } => Ok(()),
        KvAction::IterNext { .. } => Ok(()),
        KvAction::IterClose { .. } => Ok(()),
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
    kind: &str,
    db: &str,
    our: &Address,
    source: &Address,
    send_to_caps_oracle: &CapMessageSender,
) -> Result<(), KvError> {
    let cap = Capability {
        issuer: our.clone(),
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
