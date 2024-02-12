use anyhow::Result;
use dashmap::DashMap;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use surrealdb::engine::local::{Db, RocksDb};
use surrealdb::opt::Config;
use surrealdb::sql::Kind;
use surrealdb::Surreal;
use tokio::fs;
use tokio::sync::Mutex;

use lib::types::core::*;

pub type SurrealDBConn = Surreal<Db>;

lazy_static::lazy_static! {
    static ref READ_KEYWORDS: HashSet<String> = {
        let mut set = HashSet::new();
        // TODO: test LIVE SELECT
        let keywords = ["SELECT", "SHOW", "LIVE SELECT", "INFO", "USE"];
        for &keyword in &keywords {
            set.insert(keyword.to_string());
        }
        set
    };
    static ref WRITE_KEYWORDS: HashSet<String> = {
        let mut set = HashSet::new();
        let keywords = ["BEGIN", "CANCEL", "COMMIT", "CONTINUE", "CREATE", "DEFINE", "DELETE", "INSERT", "RELATE", "REMOVE", "UPDATE"];
        for &keyword in &keywords {
            set.insert(keyword.to_string());
        }
        set
    };
}

pub async fn gdb(
    our_node: String,
    send_to_loop: MessageSender,
    send_to_terminal: PrintSender,
    mut recv_from_loop: MessageReceiver,
    send_to_caps_oracle: CapMessageSender,
    home_directory_path: String,
) -> anyhow::Result<()> {
    let graphdb_path = format!("{}/graphdb", &home_directory_path);

    if let Err(e) = fs::create_dir_all(&graphdb_path).await {
        panic!("failed creating graphdb dir! {:?}", e);
    }

    let open_gdbs: Arc<DashMap<(PackageId, String), Mutex<SurrealDBConn>>> =
        Arc::new(DashMap::new());
    let txs: Arc<DashMap<u64, Vec<(GraphDbAction, Vec<Kind>)>>> = Arc::new(DashMap::new());

    let mut process_queues: HashMap<ProcessId, Arc<Mutex<VecDeque<KernelMessage>>>> =
        HashMap::new();

    loop {
        tokio::select! {
            Some(km) = recv_from_loop.recv() => {
                if our_node.clone() != km.source.node {
                    println!(
                        "graphdb: request must come from our_node={}, got: {}",
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
                let open_gdbs = open_gdbs.clone();

                let txs = txs.clone();
                let graphdb_path = graphdb_path.clone();

                tokio::spawn(async move {
                    let mut queue_lock = queue.lock().await;
                    if let Some(km) = queue_lock.pop_front() {
                        if let Err(e) = handle_request(
                            our_node.clone(),
                            km.clone(),
                            open_gdbs.clone(),
                            txs.clone(),
                            send_to_loop.clone(),
                            send_to_terminal.clone(),
                            send_to_caps_oracle.clone(),
                            graphdb_path.clone(),
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
    open_gdbs: Arc<DashMap<(PackageId, String), Mutex<SurrealDBConn>>>,
    _txs: Arc<DashMap<u64, Vec<(GraphDbAction, Vec<Kind>)>>>,
    send_to_loop: MessageSender,
    send_to_terminal: PrintSender,
    send_to_caps_oracle: CapMessageSender,
    graphdb_path: String,
) -> Result<(), GraphDbError> {
    let KernelMessage {
        id,
        source,
        message,
        lazy_load_blob: blob,
        ..
    } = km.clone();
    let Message::Request(Request {
        body,
        expects_response,
        metadata,
        ..
    }) = message.clone()
    else {
        return Err(GraphDbError::InputError {
            error: "not a request".into(),
        });
    };

    let request: GraphDbRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            println!("graphdb: got invalid Request: {}", e);
            return Err(GraphDbError::InputError {
                error: "didn't serialize to GraphDbAction.".into(),
            });
        }
    };

    check_caps(
        our_node.clone(),
        source.clone(),
        open_gdbs.clone(),
        send_to_caps_oracle.clone(),
        &request,
        graphdb_path.clone(),
    )
    .await?;

    let db_name = request.db.clone();

    let (body, bytes) = match &request.action {
        GraphDbAction::Open => {
            // handled in check_caps.
            (serde_json::to_vec(&GraphDbResponse::Ok).unwrap(), None)
        }
        GraphDbAction::RemoveDb => {
            // handled in check_caps.
            (serde_json::to_vec(&GraphDbResponse::Ok).unwrap(), None)
        }
        GraphDbAction::Define { resource } => {
            let db = match open_gdbs.get(&(request.package_id, request.db)) {
                None => {
                    return Err(GraphDbError::NoDb);
                }
                Some(db) => db,
            };

            let db = db.lock().await;
            db.use_ns(source.process.package()).await.unwrap();
            db.use_db(db_name).await.unwrap();

            let query = db.query(resource.clone().query());

            query.await.map_err(|err| GraphDbError::SurrealDBError {
                action: "".into(),
                error: err.to_string(),
            })?;

            (serde_json::to_vec(&GraphDbResponse::Ok).unwrap(), None)
        }
        GraphDbAction::Read { statement } => {
            let db = match open_gdbs.get(&(request.package_id, request.db)) {
                None => {
                    return Err(GraphDbError::NoDb);
                }
                Some(db) => db,
            };

            // remove escape characters
            let statement = statement.replace("\\", "");
            let first_word = statement
                .split_whitespace()
                .next()
                .map(|word| word.to_uppercase())
                .unwrap_or("".to_string());
            if !READ_KEYWORDS.contains(&first_word) {
                return Err(GraphDbError::NotAReadKeyword);
            }

            let db = db.lock().await;
            db.use_ns(source.process.package()).await.unwrap();
            db.use_db(db_name).await.unwrap();

            let mut results = match db.query(statement.clone()).await {
                Ok(response) => response,
                Err(e) => {
                    return Err(GraphDbError::SurrealDBError {
                        action: "".into(),
                        error: e.to_string(),
                    });
                }
            };

            let results_data = match results.take(0) {
                Ok(r) => match r {
                    surrealdb::sql::Value::Array(a) => Some(
                        surrealdb::sql::Value::Array(a)
                            .into_json()
                            .to_string()
                            .as_bytes()
                            .to_vec(),
                    ),
                    surrealdb::sql::Value::Object(o) => Some(
                        surrealdb::sql::Value::Object(o)
                            .into_json()
                            .to_string()
                            .as_bytes()
                            .to_vec(),
                    ),
                    _ => Some(
                        surrealdb::sql::Value::None
                            .into_json()
                            .to_string()
                            .as_bytes()
                            .to_vec(),
                    ),
                },
                Err(e) => {
                    return Err(GraphDbError::SurrealDBError {
                        action: "".into(),
                        error: e.to_string(),
                    });
                }
            };

            let serialized_data = serde_json::to_vec(&GraphDbResponse::Data).unwrap();
            (serialized_data, results_data)
        }
        GraphDbAction::Write { statement } => {
            let db = match open_gdbs.get(&(request.package_id, request.db)) {
                None => {
                    return Err(GraphDbError::NoDb);
                }
                Some(db) => db,
            };

            let first_word = statement
                .split_whitespace()
                .next()
                .map(|word| word.to_uppercase())
                .unwrap_or("".to_string());

            if !WRITE_KEYWORDS.contains(&first_word) {
                return Err(GraphDbError::NotAWriteKeyword);
            }

            let db = db.lock().await;
            db.use_ns(source.process.package()).await.unwrap();
            db.use_db(db_name).await.unwrap();

            let params = get_json_params(blob)?;

            // if no params and parmas is an empty array, just execute the statement
            let res = match &params {
                Some(p) => {
                    if p.is_array() && p.as_array().unwrap().is_empty() {
                        db.query(statement.clone()).await
                    } else {
                        db.query(statement.clone()).bind(p).await
                    }
                }
                _ => {
                    // If parameters are None or empty, execute the query without binding params
                    db.query(statement.clone()).await
                }
            };

            let _results = match res {
                Ok(response) => response,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    return Err(GraphDbError::SurrealDBError {
                        action: "".into(),
                        error: e.to_string(),
                    });
                }
            };

            // TODO: handle results
            // println!("graphdb: write results Ok", results);

            (serde_json::to_vec(&GraphDbResponse::Ok).unwrap(), None)
        }
        GraphDbAction::Backup => {
            // TODO: implement and test
            // for db_ref in open_gdbs.iter() {
            //     let db = db_ref.value();
            // }
            (serde_json::to_vec(&GraphDbResponse::Ok).unwrap(), None)
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
                process: GRAPHDB_PROCESS_ID.clone(),
            },
            target,
            rsvp: None,
            message: Message::Response((
                Response {
                    inherit: false,
                    body,
                    metadata,
                    capabilities: vec![],
                },
                None,
            )),
            lazy_load_blob: bytes.map(|bytes| LazyLoadBlob {
                mime: Some("application/octet-stream".into()),
                bytes,
            }),
        };

        let _ = send_to_loop.send(response).await;
    } else {
        send_to_terminal
            .send(Printout {
                verbosity: 2,
                content: format!(
                    "graphdb: not sending response: {:?}",
                    serde_json::from_slice::<GraphDbResponse>(&body)
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
    open_gdbs: Arc<DashMap<(PackageId, String), Mutex<SurrealDBConn>>>,
    mut send_to_caps_oracle: CapMessageSender,
    request: &GraphDbRequest,
    graphdb_path: String,
) -> Result<(), GraphDbError> {
    let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
    let src_package_id = PackageId::new(source.process.package(), source.process.publisher());

    match &request.action {
        GraphDbAction::Write { .. } => {
            send_to_caps_oracle
                .send(CapMessage::Has {
                    on: source.process.clone(),
                    cap: Capability {
                        issuer: Address {
                            node: our_node.clone(),
                            process: GRAPHDB_PROCESS_ID.clone(),
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
                return Err(GraphDbError::NoCap {
                    error: request.action.to_string(),
                });
            }
            Ok(())
        }
        GraphDbAction::Read { .. } => {
            send_to_caps_oracle
                .send(CapMessage::Has {
                    on: source.process.clone(),
                    cap: Capability {
                        issuer: Address {
                            node: our_node.clone(),
                            process: GRAPHDB_PROCESS_ID.clone(),
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
                return Err(GraphDbError::NoCap {
                    error: request.action.to_string(),
                });
            }
            Ok(())
        }
        GraphDbAction::Define { .. } => {
            send_to_caps_oracle
                .send(CapMessage::Has {
                    on: source.process.clone(),
                    cap: Capability {
                        issuer: Address {
                            node: our_node.clone(),
                            process: GRAPHDB_PROCESS_ID.clone(),
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
                return Err(GraphDbError::NoCap {
                    error: request.action.to_string(),
                });
            }
            Ok(())
        }
        GraphDbAction::Open { .. } => {
            if src_package_id != request.package_id {
                return Err(GraphDbError::NoCap {
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

            if open_gdbs.contains_key(&(request.package_id.clone(), request.db.clone())) {
                return Ok(());
            }

            fs::create_dir_all(&graphdb_path).await?;

            let db = SurrealDBConn::new::<RocksDb>((graphdb_path, Config::default()))
                .await
                .map_err(|err| GraphDbError::SurrealDBError {
                    action: "".into(),
                    error: err.to_string(),
                })
                .unwrap();

            // Define a namespace for the process
            // TODO: if it doesn't already exist
            db.query(format!("DEFINE namespace {};", source.process.package()))
                .await
                .map_err(|err| GraphDbError::SurrealDBError {
                    action: "Create".into(),
                    error: err.to_string(),
                })?;

            db.use_ns(source.process.package()).await.map_err(|err| {
                GraphDbError::SurrealDBError {
                    action: "Create".into(),
                    error: err.to_string(),
                }
            })?;

            // Create a new database for the process
            // TODO: if it doesn't already exist
            db.query(format!("DEFINE database {};", request.db))
                .await
                .map_err(|err| GraphDbError::SurrealDBError {
                    action: "Create".into(),
                    error: err.to_string(),
                })?;

            println!("\n graphdb: created/opened db: {}", request.db);

            open_gdbs.insert(
                (request.package_id.clone(), request.db.clone()),
                Mutex::new(db.clone()),
            );
            Ok(())
        }
        GraphDbAction::RemoveDb { .. } => {
            if src_package_id != request.package_id {
                return Err(GraphDbError::NoCap {
                    error: request.action.to_string(),
                });
            }

            let db_path = format!("{}/{}/{}", graphdb_path, request.package_id, request.db);
            open_gdbs.remove(&(request.package_id.clone(), request.db.clone()));

            fs::remove_dir_all(&db_path).await?;
            Ok(())
        }
        GraphDbAction::Backup => Ok(()),
    }
}

async fn add_capability(
    kind: &str,
    db: &str,
    our_node: &str,
    source: &Address,
    send_to_caps_oracle: &mut CapMessageSender,
) -> Result<(), GraphDbError> {
    let cap = Capability {
        issuer: Address {
            node: our_node.to_string(),
            process: GRAPHDB_PROCESS_ID.clone(),
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

fn make_error_message(our_name: String, km: &KernelMessage, error: GraphDbError) -> KernelMessage {
    KernelMessage {
        id: km.id,
        source: Address {
            node: our_name.clone(),
            process: GRAPHDB_PROCESS_ID.clone(),
        },
        target: match &km.rsvp {
            None => km.source.clone(),
            Some(rsvp) => rsvp.clone(),
        },
        rsvp: None,
        message: Message::Response((
            Response {
                inherit: false,
                body: serde_json::to_vec(&GraphDbResponse::Err { error }).unwrap(),
                metadata: None,
                capabilities: vec![],
            },
            None,
        )),
        lazy_load_blob: None,
    }
}

fn get_json_params(blob: Option<LazyLoadBlob>) -> Result<Option<serde_json::Value>, GraphDbError> {
    match blob {
        None => Ok(None),
        Some(blob) => match serde_json::from_slice::<serde_json::Value>(&blob.bytes) {
            Ok(params) => {
                if params.is_array() && params.as_array().unwrap().is_empty() {
                    return Ok(None);
                }
                Ok(Some(params))
            }
            Err(e) => Err(GraphDbError::InputError {
                error: format!("graphdb: gave unparsable params: {}", e),
            }),
        },
    }
}
