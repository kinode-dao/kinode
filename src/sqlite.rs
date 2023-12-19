use anyhow::Result;
use dashmap::DashMap;
use rusqlite::types::{FromSql, FromSqlError, ToSql, ValueRef};
use rusqlite::Connection;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::Mutex;

use crate::types::*;

pub async fn sqlite(
    our_node: String,
    send_to_loop: MessageSender,
    send_to_terminal: PrintSender,
    mut recv_from_loop: MessageReceiver,
    send_to_caps_oracle: CapMessageSender,
    home_directory_path: String,
) -> anyhow::Result<()> {
    let sqlite_path = format!("{}/sqlite", &home_directory_path);

    if let Err(e) = fs::create_dir_all(&sqlite_path).await {
        panic!("failed creating sqlite dir! {:?}", e);
    }

    let open_dbs: Arc<DashMap<(PackageId, String), Mutex<Connection>>> = Arc::new(DashMap::new());
    let txs: Arc<DashMap<u64, Vec<(String, Vec<SqlValue>)>>> = Arc::new(DashMap::new());

    let mut process_queues: HashMap<ProcessId, Arc<Mutex<VecDeque<KernelMessage>>>> =
        HashMap::new();

    loop {
        tokio::select! {
            Some(km) = recv_from_loop.recv() => {
                if our_node.clone() != km.source.node {
                    println!(
                        "sqlite: request must come from our_node={}, got: {}",
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
                let open_dbs = open_dbs.clone();
                let txs = txs.clone();
                let sqlite_path = sqlite_path.clone();

                tokio::spawn(async move {
                    let mut queue_lock = queue.lock().await;
                    if let Some(km) = queue_lock.pop_front() {
                        if let Err(e) = handle_request(
                            our_node.clone(),
                            km.clone(),
                            open_dbs.clone(),
                            txs.clone(),
                            send_to_loop.clone(),
                            send_to_terminal.clone(),
                            send_to_caps_oracle.clone(),
                            sqlite_path.clone(),
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
    open_dbs: Arc<DashMap<(PackageId, String), Mutex<Connection>>>,
    txs: Arc<DashMap<u64, Vec<(String, Vec<SqlValue>)>>>,
    send_to_loop: MessageSender,
    send_to_terminal: PrintSender,
    send_to_caps_oracle: CapMessageSender,
    sqlite_path: String,
) -> Result<(), SqliteError> {
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
        return Err(SqliteError::InputError {
            error: "not a request".into(),
        });
    };

    let request: SqliteRequest = match serde_json::from_slice(&ipc) {
        Ok(r) => r,
        Err(e) => {
            println!("sqlite: got invalid Request: {}", e);
            return Err(SqliteError::InputError {
                error: "didn't serialize to SqliteRequest.".into(),
            });
        }
    };

    check_caps(
        our_node.clone(),
        source.clone(),
        open_dbs.clone(),
        send_to_caps_oracle.clone(),
        &request,
        sqlite_path.clone(),
    )
    .await?;

    let (ipc, bytes) = match request.action {
        SqliteAction::New => {
            // handled in check_caps
            //
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

            let parameters = get_json_params(payload)?;

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
                            SqlValue::Blob(blob) => serde_json::Value::String(base64::encode(blob)), // or another representation if you prefer
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

            let parameters = get_json_params(payload)?;

            match tx_id {
                Some(tx_id) => {
                    txs.entry(tx_id)
                        .or_insert_with(Vec::new)
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
            // execute WAL flush.
            //
            (serde_json::to_vec(&SqliteResponse::Ok).unwrap(), None)
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
                process: SQLITE_PROCESS_ID.clone(),
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
                    "sqlite: not sending response: {:?}",
                    serde_json::from_slice::<SqliteResponse>(&ipc)
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
    open_dbs: Arc<DashMap<(PackageId, String), Mutex<Connection>>>,
    mut send_to_caps_oracle: CapMessageSender,
    request: &SqliteRequest,
    sqlite_path: String,
) -> Result<(), SqliteError> {
    let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
    let src_package_id = PackageId::new(source.process.package(), source.process.publisher());

    match &request.action {
        SqliteAction::Write { .. } | SqliteAction::BeginTx | SqliteAction::Commit { .. } => {
            send_to_caps_oracle
                .send(CapMessage::Has {
                    on: source.process.clone(),
                    cap: Capability {
                        issuer: Address {
                            node: our_node.clone(),
                            process: SQLITE_PROCESS_ID.clone(),
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
                    cap: Capability {
                        issuer: Address {
                            node: our_node.clone(),
                            process: SQLITE_PROCESS_ID.clone(),
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
                return Err(SqliteError::NoCap {
                    error: request.action.to_string(),
                });
            }
            Ok(())
        }
        SqliteAction::New => {
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

            let db_path = format!("{}{}", sqlite_path, request.db.to_string());

            fs::create_dir_all(&db_path).await?;

            let db = Connection::open(&db_path)?;
            db.execute("PRAGMA journal_mode=WAL;", [])?;

            open_dbs.insert((request.package_id.clone(), request.db.clone()), Mutex::new(db));
            Ok(())
        }
        SqliteAction::Backup => {
            if source.process != *STATE_PROCESS_ID {
                return Err(SqliteError::NoCap {
                    error: request.action.to_string(),
                });
            }
            Ok(())
        }
    }
}

async fn add_capability(
    kind: &str,
    db: &str,
    our_node: &str,
    source: &Address,
    send_to_caps_oracle: &mut CapMessageSender,
) -> Result<(), SqliteError> {
    let cap = Capability {
        issuer: Address {
            node: our_node.to_string(),
            process: SQLITE_PROCESS_ID.clone(),
        },
        params: serde_json::to_string(&serde_json::json!({ "kind": kind, "db": db })).unwrap(),
    };
    let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
    send_to_caps_oracle
        .send(CapMessage::Add {
            on: source.process.clone(),
            cap,
            responder: send_cap_bool,
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
            match base64::decode(&s) {
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

fn get_json_params(payload: Option<Payload>) -> Result<Vec<SqlValue>, SqliteError> {
    match payload {
        None => Ok(vec![]),
        Some(payload) => match serde_json::from_slice::<serde_json::Value>(&payload.bytes) {
            Ok(serde_json::Value::Array(vec)) => vec
                .iter()
                .map(|value| json_to_sqlite(value))
                .collect::<Result<Vec<_>, _>>(),
            _ => Err(SqliteError::InvalidParameters),
        },
    }
}

fn make_error_message(our_name: String, km: &KernelMessage, error: SqliteError) -> KernelMessage {
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
                ipc: serde_json::to_vec(&SqliteResponse::Err { error: error }).unwrap(),
                metadata: None,
            },
            None,
        )),
        payload: None,
        signed_capabilities: None,
    }
}
impl ToSql for SqlValue {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput> {
        match self {
            SqlValue::Integer(i) => i.to_sql(),
            SqlValue::Real(f) => f.to_sql(),
            SqlValue::Text(ref s) => s.to_sql(),
            SqlValue::Blob(ref b) => b.to_sql(),
            SqlValue::Boolean(b) => b.to_sql(),
            SqlValue::Null => Ok(rusqlite::types::ToSqlOutput::Owned(
                rusqlite::types::Value::Null,
            )),
        }
    }
}

impl FromSql for SqlValue {
    fn column_result(value: ValueRef<'_>) -> Result<Self, FromSqlError> {
        match value {
            ValueRef::Integer(i) => Ok(SqlValue::Integer(i)),
            ValueRef::Real(f) => Ok(SqlValue::Real(f)),
            ValueRef::Text(t) => {
                let text_str = std::str::from_utf8(t).map_err(|_| FromSqlError::InvalidType)?;
                Ok(SqlValue::Text(text_str.to_string()))
            }
            ValueRef::Blob(b) => Ok(SqlValue::Blob(b.to_vec())),
            _ => Err(FromSqlError::InvalidType),
        }
    }
}

impl std::fmt::Display for SqliteAction {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<std::io::Error> for SqliteError {
    fn from(err: std::io::Error) -> Self {
        SqliteError::IOError {
            error: err.to_string(),
        }
    }
}

impl From<rusqlite::Error> for SqliteError {
    fn from(err: rusqlite::Error) -> Self {
        SqliteError::RusqliteError {
            error: err.to_string(),
        }
    }
}

impl From<tokio::sync::oneshot::error::RecvError> for SqliteError {
    fn from(err: tokio::sync::oneshot::error::RecvError) -> Self {
        SqliteError::NoCap {
            error: err.to_string(),
        }
    }
}

impl From<tokio::sync::mpsc::error::SendError<CapMessage>> for SqliteError {
    fn from(err: tokio::sync::mpsc::error::SendError<CapMessage>) -> Self {
        SqliteError::NoCap {
            error: err.to_string(),
        }
    }
}
