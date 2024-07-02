use base64::{engine::general_purpose::STANDARD as base64_standard, Engine};
use dashmap::DashMap;
use lib::types::core::{
    Address, CapMessage, CapMessageSender, Capability, KernelMessage, LazyLoadBlob, Message,
    MessageReceiver, MessageSender, PackageId, PrintSender, Printout, ProcessId, Request, Response,
    SqlValue, SqliteAction, SqliteError, SqliteRequest, SqliteResponse, SQLITE_PROCESS_ID,
};
use rusqlite::Connection;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};
use tokio::{fs, sync::Mutex};

lazy_static::lazy_static! {
    static ref READ_KEYWORDS: HashSet<&'static str> =
        HashSet::from(["ANALYZE", "ATTACH", "BEGIN", "EXPLAIN", "PRAGMA", "SELECT", "VALUES", "WITH"]);

    static ref WRITE_KEYWORDS: HashSet<&'static str> =
        HashSet::from(["ALTER", "ANALYZE", "COMMIT", "CREATE", "DELETE", "DETACH", "DROP", "END", "INSERT", "REINDEX", "RELEASE", "RENAME", "REPLACE", "ROLLBACK", "SAVEPOINT", "UPDATE", "VACUUM"]);
}

pub async fn sqlite(
    our_node: Arc<String>,
    send_to_loop: MessageSender,
    send_to_terminal: PrintSender,
    mut recv_from_loop: MessageReceiver,
    send_to_caps_oracle: CapMessageSender,
    home_directory_path: String,
) -> anyhow::Result<()> {
    let sqlite_path = Arc::new(format!("{home_directory_path}/sqlite"));
    if let Err(e) = fs::create_dir_all(&*sqlite_path).await {
        panic!("failed creating sqlite dir! {e:?}");
    }

    let open_dbs: Arc<DashMap<(PackageId, String), Mutex<Connection>>> = Arc::new(DashMap::new());
    let txs: Arc<DashMap<u64, Vec<(String, Vec<SqlValue>)>>> = Arc::new(DashMap::new());

    let process_queues: HashMap<ProcessId, Arc<Mutex<VecDeque<KernelMessage>>>> = HashMap::new();

    while let Some(km) = recv_from_loop.recv().await {
        if *our_node != km.source.node {
            Printout::new(
                1,
                format!(
                    "sqlite: got request from {}, but requests must come from our node {our_node}",
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
        let open_dbs = open_dbs.clone();
        let txs = txs.clone();
        let sqlite_path = sqlite_path.clone();

        tokio::spawn(async move {
            let mut queue_lock = queue.lock().await;
            if let Some(km) = queue_lock.pop_front() {
                let (km_id, km_rsvp) =
                    (km.id.clone(), km.rsvp.clone().unwrap_or(km.source.clone()));

                if let Err(e) = handle_request(
                    &our_node,
                    km,
                    open_dbs,
                    txs,
                    &send_to_loop,
                    &send_to_caps_oracle,
                    &sqlite_path,
                )
                .await
                {
                    Printout::new(1, format!("sqlite: {e}"))
                        .send(&send_to_terminal)
                        .await;
                    KernelMessage::builder()
                        .id(km_id)
                        .source((our_node.as_str(), SQLITE_PROCESS_ID.clone()))
                        .target(km_rsvp)
                        .message(Message::Response((
                            Response {
                                inherit: false,
                                body: serde_json::to_vec(&SqliteResponse::Err { error: e })
                                    .unwrap(),
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
    open_dbs: Arc<DashMap<(PackageId, String), Mutex<Connection>>>,
    txs: Arc<DashMap<u64, Vec<(String, Vec<SqlValue>)>>>,
    send_to_loop: &MessageSender,
    send_to_caps_oracle: &CapMessageSender,
    sqlite_path: &str,
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
        return Err(SqliteError::InputError {
            error: "not a request".into(),
        });
    };

    let request: SqliteRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            println!("sqlite: got invalid Request: {}", e);
            return Err(SqliteError::InputError {
                error: "didn't serialize to SqliteRequest.".into(),
            });
        }
    };

    check_caps(
        our_node,
        &source,
        &open_dbs,
        send_to_caps_oracle,
        &request,
        sqlite_path,
    )
    .await?;

    let (body, bytes) = match request.action {
        SqliteAction::Open => {
            // handled in check_caps
            (serde_json::to_vec(&SqliteResponse::Ok).unwrap(), None)
        }
        SqliteAction::RemoveDb => {
            // handled in check_caps
            (serde_json::to_vec(&SqliteResponse::Ok).unwrap(), None)
        }
        SqliteAction::Read { query } => {
            let db = match open_dbs.get(&(request.package_id, request.db)) {
                Some(db) => db,
                None => {
                    return Err(SqliteError::NoDb);
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
                        let value: SqlValue = row.get(i)?;
                        let value_json = match value {
                            SqlValue::Integer(int) => serde_json::Value::Number(int.into()),
                            SqlValue::Real(real) => serde_json::Value::Number(
                                serde_json::Number::from_f64(real).unwrap(),
                            ),
                            SqlValue::Text(text) => serde_json::Value::String(text),
                            SqlValue::Blob(blob) => {
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
            let db = match open_dbs.get(&(request.package_id, request.db)) {
                Some(db) => db,
                None => {
                    return Err(SqliteError::NoDb);
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
                    txs.entry(tx_id)
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
            txs.insert(tx_id, Vec::new());

            (
                serde_json::to_vec(&SqliteResponse::BeginTx { tx_id }).unwrap(),
                None,
            )
        }
        SqliteAction::Commit { tx_id } => {
            let db = match open_dbs.get(&(request.package_id, request.db)) {
                Some(db) => db,
                None => {
                    return Err(SqliteError::NoDb);
                }
            };
            let mut db = db.lock().await;

            let txs = match txs.remove(&tx_id).map(|(_, tx)| tx) {
                None => {
                    return Err(SqliteError::NoTx);
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
        SqliteAction::Backup => {
            for db_ref in open_dbs.iter() {
                let db = db_ref.value().lock().await;
                let result: rusqlite::Result<()> = db
                    .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
                    .map(|_| ());
                if let Err(e) = result {
                    return Err(SqliteError::RusqliteError {
                        error: e.to_string(),
                    });
                }
            }
            (serde_json::to_vec(&SqliteResponse::Ok).unwrap(), None)
        }
    };

    if let Some(target) = km.rsvp.or_else(|| expects_response.map(|_| source)) {
        KernelMessage::builder()
            .id(id)
            .source((our_node, SQLITE_PROCESS_ID.clone()))
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
    open_dbs: &Arc<DashMap<(PackageId, String), Mutex<Connection>>>,
    send_to_caps_oracle: &CapMessageSender,
    request: &SqliteRequest,
    sqlite_path: &str,
) -> Result<(), SqliteError> {
    let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
    let src_package_id = PackageId::new(source.process.package(), source.process.publisher());

    match &request.action {
        SqliteAction::Write { .. } | SqliteAction::BeginTx | SqliteAction::Commit { .. } => {
            send_to_caps_oracle
                .send(CapMessage::Has {
                    on: source.process.clone(),
                    cap: Capability::new(
                        (our_node, SQLITE_PROCESS_ID.clone()),
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
                return Err(SqliteError::NoCap {
                    error: request.action.to_string(),
                });
            }
            Ok(())
        }
        SqliteAction::Read { .. } => {
            send_to_caps_oracle
                .send(CapMessage::Has {
                    on: source.process.clone(),
                    cap: Capability::new(
                        (our_node, SQLITE_PROCESS_ID.clone()),
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
                return Err(SqliteError::NoCap {
                    error: request.action.to_string(),
                });
            }
            Ok(())
        }
        SqliteAction::Open => {
            if src_package_id != request.package_id {
                return Err(SqliteError::NoCap {
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

            if open_dbs.contains_key(&(request.package_id.clone(), request.db.clone())) {
                return Ok(());
            }

            let db_path = format!("{}/{}/{}", sqlite_path, request.package_id, request.db);
            fs::create_dir_all(&db_path).await?;

            let db_file_path = format!("{}/{}.db", db_path, request.db);

            let db = Connection::open(db_file_path)?;
            let _ = db.execute("PRAGMA journal_mode=WAL", []);

            open_dbs.insert(
                (request.package_id.clone(), request.db.clone()),
                Mutex::new(db),
            );
            Ok(())
        }
        SqliteAction::RemoveDb => {
            if src_package_id != request.package_id {
                return Err(SqliteError::NoCap {
                    error: request.action.to_string(),
                });
            }

            let db_path = format!("{}/{}/{}", sqlite_path, request.package_id, request.db);
            open_dbs.remove(&(request.package_id.clone(), request.db.clone()));

            fs::remove_dir_all(&db_path).await?;
            Ok(())
        }
        SqliteAction::Backup => {
            // flushing WALs for backup
            Ok(())
        }
    }
}

async fn add_capability(
    kind: &str,
    db: &str,
    our_node: &str,
    source: &Address,
    send_to_caps_oracle: &CapMessageSender,
) -> Result<(), SqliteError> {
    let cap = Capability {
        issuer: Address {
            node: our_node.to_string(),
            process: SQLITE_PROCESS_ID.clone(),
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
