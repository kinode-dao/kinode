use crate::vfs::UniqueQueue;
use base64::{engine::general_purpose::STANDARD as base64_standard, Engine};
use dashmap::DashMap;
use lib::types::core::{
    Address, CapMessage, CapMessageSender, Capability, FdManagerRequest, KernelMessage,
    LazyLoadBlob, Message, MessageReceiver, MessageSender, PackageId, PrintSender, Printout,
    ProcessId, Request, Response, SqlValue, SqliteAction, SqliteCapabilityKind,
    SqliteCapabilityParams, SqliteError, SqliteRequest, SqliteResponse, FD_MANAGER_PROCESS_ID,
    SQLITE_PROCESS_ID,
};
use rusqlite::Connection;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::Arc,
};
use tokio::{fs, sync::Mutex};

lazy_static::lazy_static! {
    static ref READ_KEYWORDS: HashSet<&'static str> =
        HashSet::from(["ANALYZE", "ATTACH", "BEGIN", "EXPLAIN", "PRAGMA", "SELECT", "VALUES", "WITH"]);

    static ref WRITE_KEYWORDS: HashSet<&'static str> =
        HashSet::from(["ALTER", "ANALYZE", "COMMIT", "CREATE", "DELETE", "DETACH", "DROP", "END", "INSERT", "REINDEX", "RELEASE", "RENAME", "REPLACE", "ROLLBACK", "SAVEPOINT", "UPDATE", "VACUUM"]);
}

#[derive(Clone)]
struct SqliteState {
    our: Arc<Address>,
    sqlite_path: Arc<PathBuf>,
    send_to_loop: MessageSender,
    send_to_terminal: PrintSender,
    open_dbs: Arc<DashMap<(PackageId, String), Mutex<Connection>>>,
    access_order: Arc<Mutex<UniqueQueue<(PackageId, String)>>>,
    txs: Arc<DashMap<u64, Vec<(String, Vec<SqlValue>)>>>,
    fds_limit: u64,
}

impl SqliteState {
    pub fn new(
        our: Address,
        send_to_terminal: PrintSender,
        send_to_loop: MessageSender,
        home_directory_path: PathBuf,
    ) -> Self {
        Self {
            our: Arc::new(our),
            sqlite_path: Arc::new(home_directory_path.join("sqlite")),
            send_to_loop,
            send_to_terminal,
            open_dbs: Arc::new(DashMap::new()),
            access_order: Arc::new(Mutex::new(UniqueQueue::new())),
            txs: Arc::new(DashMap::new()),
            fds_limit: 10,
        }
    }

    pub async fn open_db(&mut self, key: &(PackageId, String)) -> Result<(), SqliteError> {
        if self.open_dbs.contains_key(key) {
            let mut access_order = self.access_order.lock().await;
            access_order.remove(key);
            access_order.push_back(key.clone());
            return Ok(());
        }

        if self.open_dbs.len() as u64 >= self.fds_limit {
            // close least recently used db
            let to_close = self.access_order.lock().await.pop_front().unwrap();
            self.remove_db(&to_close).await;
        }

        #[cfg(unix)]
        let db_path = self.sqlite_path.join(format!("{}", key.0)).join(&key.1);
        #[cfg(target_os = "windows")]
        let db_path = self
            .sqlite_path
            .join(format!("{}_{}", key.0._package(), key.0._publisher()))
            .join(&key.1);

        fs::create_dir_all(&db_path).await?;

        let db_file_path = db_path.join(format!("{}.db", key.1));

        let db_conn = Connection::open(db_file_path)?;
        let _: String = db_conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;

        self.open_dbs.insert(key.clone(), Mutex::new(db_conn));

        let mut access_order = self.access_order.lock().await;
        access_order.push_back(key.clone());
        Ok(())
    }

    pub async fn remove_db(&mut self, key: &(PackageId, String)) {
        self.open_dbs.remove(key);
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

pub async fn sqlite(
    our_node: Arc<String>,
    send_to_loop: MessageSender,
    send_to_terminal: PrintSender,
    mut recv_from_loop: MessageReceiver,
    send_to_caps_oracle: CapMessageSender,
    home_directory_path: PathBuf,
) -> anyhow::Result<()> {
    let our = Address::new(our_node.as_str(), SQLITE_PROCESS_ID.clone());

    crate::fd_manager::send_fd_manager_request_fds_limit(&our, &send_to_loop).await;

    let mut state = SqliteState::new(our, send_to_terminal, send_to_loop, home_directory_path);

    if let Err(e) = fs::create_dir_all(&*state.sqlite_path).await {
        panic!("failed creating sqlite dir! {e:?}");
    }

    let process_queues: HashMap<ProcessId, Arc<Mutex<VecDeque<KernelMessage>>>> = HashMap::new();

    while let Some(km) = recv_from_loop.recv().await {
        if state.our.node != km.source.node {
            Printout::new(
                1,
                SQLITE_PROCESS_ID.clone(),
                format!(
                    "sqlite: got request from {}, but requests must come from our node {}",
                    km.source.node, state.our.node
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
                    SQLITE_PROCESS_ID.clone(),
                    format!("sqlite: got request from fd-manager that errored: {e:?}"),
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
                let (km_id, km_rsvp) = (km.id, km.rsvp.clone().unwrap_or(km.source.clone()));

                if let Err(e) = handle_request(km, &mut state, &send_to_caps_oracle).await {
                    Printout::new(1, SQLITE_PROCESS_ID.clone(), format!("sqlite: {e}"))
                        .send(&state.send_to_terminal)
                        .await;
                    KernelMessage::builder()
                        .id(km_id)
                        .source(state.our.as_ref().clone())
                        .target(km_rsvp)
                        .message(Message::Response((
                            Response {
                                inherit: false,
                                body: serde_json::to_vec(&SqliteResponse::Err(e)).unwrap(),
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
    state: &mut SqliteState,
    send_to_caps_oracle: &CapMessageSender,
) -> Result<(), SqliteError> {
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

    let request: SqliteRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            println!("sqlite: got invalid request: {e}");
            return Err(SqliteError::MalformedRequest);
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
        SqliteAction::Open => {
            // handled in check_caps
            (serde_json::to_vec(&SqliteResponse::Ok).unwrap(), None)
        }
        SqliteAction::RemoveDb => {
            // handled in check_caps
            (serde_json::to_vec(&SqliteResponse::Ok).unwrap(), None)
        }
        SqliteAction::Query(query) => {
            let db = match state.open_dbs.get(&db_key) {
                Some(db) => db,
                None => {
                    return Err(SqliteError::NoDb(db_key.0, db_key.1));
                }
            };
            let db = db.lock().await;
            let first_word = query
                .split_whitespace()
                .next()
                .map(|word| word.to_uppercase())
                .unwrap_or("".to_string());
            if !READ_KEYWORDS.contains(first_word.as_str()) {
                return Err(SqliteError::NotAReadKeyword);
            }

            let parameters = get_json_params(blob)?;

            let mut statement = db.prepare(&query)?;
            let column_names: Vec<String> = statement
                .column_names()
                .iter()
                .map(|c| c.to_string())
                .collect();

            let results: Vec<HashMap<String, serde_json::Value>> = statement
                .query_map(rusqlite::params_from_iter(parameters.iter()), |row| {
                    let mut map = HashMap::new();
                    for (i, column_name) in column_names.iter().enumerate() {
                        let value: Option<SqlValue> = row.get(i)?;
                        let value_json = match value {
                            Some(SqlValue::Integer(int)) => serde_json::Value::Number(int.into()),
                            Some(SqlValue::Real(real)) => serde_json::Value::Number(
                                serde_json::Number::from_f64(real).unwrap(),
                            ),
                            Some(SqlValue::Text(text)) => serde_json::Value::String(text),
                            Some(SqlValue::Blob(blob)) => {
                                serde_json::Value::String(base64_standard.encode(blob))
                            } // or another representation if you prefer
                            _ => serde_json::Value::Null,
                        };
                        map.insert(column_name.clone(), value_json);
                    }
                    Ok(map)
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let results = serde_json::json!(results).to_string();
            let results_bytes = results.as_bytes().to_vec();

            (
                serde_json::to_vec(&SqliteResponse::Read).unwrap(),
                Some(results_bytes),
            )
        }
        SqliteAction::Write { statement, tx_id } => {
            let db = match state.open_dbs.get(&db_key) {
                Some(db) => db,
                None => {
                    return Err(SqliteError::NoDb(db_key.0, db_key.1));
                }
            };
            let db = db.lock().await;

            let first_word = statement
                .split_whitespace()
                .next()
                .map(|word| word.to_uppercase())
                .unwrap_or("".to_string());

            if !WRITE_KEYWORDS.contains(first_word.as_str()) {
                return Err(SqliteError::NotAWriteKeyword);
            }

            let parameters = get_json_params(blob)?;

            match tx_id {
                Some(tx_id) => {
                    state
                        .txs
                        .entry(tx_id)
                        .or_default()
                        .push((statement.clone(), parameters));
                }
                None => {
                    let mut stmt = db.prepare(&statement)?;
                    stmt.execute(rusqlite::params_from_iter(parameters.iter()))?;
                }
            };
            (serde_json::to_vec(&SqliteResponse::Ok).unwrap(), None)
        }
        SqliteAction::BeginTx => {
            let tx_id = rand::random::<u64>();
            state.txs.insert(tx_id, Vec::new());

            (
                serde_json::to_vec(&SqliteResponse::BeginTx { tx_id }).unwrap(),
                None,
            )
        }
        SqliteAction::Commit { tx_id } => {
            let db = match state.open_dbs.get(&db_key) {
                Some(db) => db,
                None => {
                    return Err(SqliteError::NoDb(db_key.0, db_key.1));
                }
            };
            let mut db = db.lock().await;

            let txs = match state.txs.remove(&tx_id).map(|(_, tx)| tx) {
                None => {
                    return Err(SqliteError::NoTx(tx_id));
                }
                Some(tx) => tx,
            };

            let tx = db.transaction()?;
            for (query, params) in txs {
                tx.execute(&query, rusqlite::params_from_iter(params.iter()))?;
            }

            tx.commit()?;
            (serde_json::to_vec(&SqliteResponse::Ok).unwrap(), None)
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
    state: &mut SqliteState,
    send_to_caps_oracle: &CapMessageSender,
    action: &SqliteAction,
    db_key: &(PackageId, String),
) -> Result<(), SqliteError> {
    let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
    let src_package_id = PackageId::new(source.process.package(), source.process.publisher());

    match action {
        SqliteAction::Write { .. } | SqliteAction::BeginTx | SqliteAction::Commit { .. } => {
            let Ok(()) = send_to_caps_oracle
                .send(CapMessage::Has {
                    on: source.process.clone(),
                    cap: Capability::new(
                        state.our.as_ref().clone(),
                        serde_json::to_string(&SqliteCapabilityParams {
                            kind: SqliteCapabilityKind::Write,
                            db_key: db_key.clone(),
                        })
                        .unwrap(),
                    ),
                    responder: send_cap_bool,
                })
                .await
            else {
                return Err(SqliteError::AddCapFailed);
            };
            let Ok(_) = recv_cap_bool.await else {
                return Err(SqliteError::AddCapFailed);
            };
            Ok(())
        }
        SqliteAction::Query { .. } => {
            let Ok(()) = send_to_caps_oracle
                .send(CapMessage::Has {
                    on: source.process.clone(),
                    cap: Capability::new(
                        state.our.as_ref().clone(),
                        serde_json::to_string(&SqliteCapabilityParams {
                            kind: SqliteCapabilityKind::Read,
                            db_key: db_key.clone(),
                        })
                        .unwrap(),
                    ),
                    responder: send_cap_bool,
                })
                .await
            else {
                return Err(SqliteError::AddCapFailed);
            };
            let Ok(_) = recv_cap_bool.await else {
                return Err(SqliteError::AddCapFailed);
            };
            Ok(())
        }
        SqliteAction::Open => {
            if src_package_id != db_key.0 {
                return Err(SqliteError::MismatchingPackageId);
            }

            add_capability(
                SqliteCapabilityKind::Read,
                db_key,
                &state.our,
                &source,
                send_to_caps_oracle,
            )
            .await?;
            add_capability(
                SqliteCapabilityKind::Write,
                db_key,
                &state.our,
                &source,
                send_to_caps_oracle,
            )
            .await?;

            if state.open_dbs.contains_key(db_key) {
                return Ok(());
            }

            state.open_db(db_key).await?;
            Ok(())
        }
        SqliteAction::RemoveDb => {
            if src_package_id != db_key.0 {
                return Err(SqliteError::MismatchingPackageId);
            }

            state.remove_db(db_key).await;

            #[cfg(unix)]
            let db_path = state
                .sqlite_path
                .join(format!("{}", db_key.0))
                .join(&db_key.1);
            #[cfg(target_os = "windows")]
            let db_path = state
                .sqlite_path
                .join(format!("{}_{}", db_key.0._package(), db_key.0._publisher()))
                .join(&db_key.1);

            fs::remove_dir_all(&db_path).await?;

            Ok(())
        }
    }
}

async fn handle_fd_request(km: KernelMessage, state: &mut SqliteState) -> anyhow::Result<()> {
    let Message::Request(Request { body, .. }) = km.message else {
        return Err(anyhow::anyhow!("not a request"));
    };

    match serde_json::from_slice(&body)? {
        FdManagerRequest::FdsLimit(new_fds_limit) => {
            state.fds_limit = new_fds_limit;
            if state.open_dbs.len() as u64 >= state.fds_limit {
                crate::fd_manager::send_fd_manager_hit_fds_limit(&state.our, &state.send_to_loop)
                    .await;
                state
                    .remove_least_recently_used_dbs(state.open_dbs.len() as u64 - state.fds_limit)
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
    kind: SqliteCapabilityKind,
    db_key: &(PackageId, String),
    our: &Address,
    source: &Address,
    send_to_caps_oracle: &CapMessageSender,
) -> Result<(), SqliteError> {
    let cap = Capability {
        issuer: our.clone(),
        params: serde_json::to_string(&SqliteCapabilityParams {
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
        return Err(SqliteError::AddCapFailed);
    };
    let Ok(_) = recv_cap_bool.await else {
        return Err(SqliteError::AddCapFailed);
    };
    Ok(())
}

fn json_to_sqlite(value: &serde_json::Value) -> Result<SqlValue, SqliteError> {
    match value {
        serde_json::Value::Number(n) => {
            if let Some(int_val) = n.as_i64() {
                Ok(SqlValue::Integer(int_val))
            } else if let Some(float_val) = n.as_f64() {
                Ok(SqlValue::Real(float_val))
            } else {
                Err(SqliteError::InvalidParameters)
            }
        }
        serde_json::Value::String(s) => {
            match base64_standard.decode(s) {
                Ok(decoded_bytes) => {
                    // convert to SQLite Blob if it's a valid base64 string
                    Ok(SqlValue::Blob(decoded_bytes))
                }
                Err(_) => {
                    // if it's not base64, just use the string itself
                    Ok(SqlValue::Text(s.clone()))
                }
            }
        }
        serde_json::Value::Bool(b) => Ok(SqlValue::Boolean(*b)),
        serde_json::Value::Null => Ok(SqlValue::Null),
        _ => Err(SqliteError::InvalidParameters),
    }
}

fn get_json_params(blob: Option<LazyLoadBlob>) -> Result<Vec<SqlValue>, SqliteError> {
    match blob {
        None => Ok(vec![]),
        Some(blob) => match serde_json::from_slice::<serde_json::Value>(&blob.bytes) {
            Ok(serde_json::Value::Array(vec)) => vec
                .iter()
                .map(json_to_sqlite)
                .collect::<Result<Vec<_>, _>>(),
            _ => Err(SqliteError::InvalidParameters),
        },
    }
}
