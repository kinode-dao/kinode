use crate::http::types::*;
use crate::http::utils::*;
use crate::register;
use crate::types::*;
use anyhow::Result;
use dashmap::DashMap;
use futures::{SinkExt, StreamExt};
use route_recognizer::Router;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use warp::http::{header::HeaderValue, StatusCode};
use warp::ws::{WebSocket, Ws};
use warp::{Filter, Reply};

const HTTP_SELF_IMPOSED_TIMEOUT: u64 = 15;

/// mapping from a given HTTP request (assigned an ID) to the oneshot
/// channel that will get a response from the app that handles the request,
/// and a string which contains the path that the request was made to.
type HttpResponseSenders = Arc<DashMap<u64, (String, HttpSender)>>;
type HttpSender = tokio::sync::oneshot::Sender<(HttpResponse, Vec<u8>)>;

/// mapping from an open websocket connection to a channel that will ingest
/// WebSocketPush messages from the app that handles the connection, and
/// send them to the connection.
type WebSocketSenders = Arc<DashMap<u64, (ProcessId, WebSocketSender)>>;
type WebSocketSender = tokio::sync::mpsc::Sender<warp::ws::Message>;

type PathBindings = Arc<RwLock<Router<BoundPath>>>;

struct BoundPath {
    pub app: ProcessId,
    pub authenticated: bool,
    pub local_only: bool,
    pub static_content: Option<Payload>, // TODO store in filesystem and cache
}

/// HTTP server: a runtime module that handles HTTP requests at a given port.
/// The server accepts bindings-requests from apps. These can be used in two ways:
///
/// 1. The app can bind to a path and receive all subsequent requests in the form
/// of an [`HttpRequest`] to that path.
/// They will be responsible for generating HTTP responses in the form of an
/// [`HttpResponse`] to those requests.
///
/// 2. The app can bind static content to a path. The server will handle all subsequent
/// requests, serving that static content. It will only respond to `GET` requests.
///
///
/// In addition to binding on paths, the HTTP server can receive incoming WebSocket connections
/// and pass them to a targeted app. The server will handle encrypting and decrypting messages
/// over these connections.
pub async fn http_server(
    our_name: String,
    our_port: u16,
    jwt_secret_bytes: Vec<u8>,
    mut recv_in_server: MessageReceiver,
    send_to_loop: MessageSender,
    print_tx: PrintSender,
) -> Result<()> {
    let our_name = Arc::new(our_name);
    let jwt_secret_bytes = Arc::new(jwt_secret_bytes);
    let http_response_senders: HttpResponseSenders = Arc::new(DashMap::new());
    let ws_senders: WebSocketSenders = Arc::new(DashMap::new());

    // Add RPC path
    let mut bindings_map: Router<BoundPath> = Router::new();
    let rpc_bound_path = BoundPath {
        app: ProcessId::from_str("rpc:sys:uqbar").unwrap(),
        authenticated: false,
        local_only: true,
        static_content: None,
    };
    bindings_map.add("/rpc:sys:uqbar/message", rpc_bound_path);

    let path_bindings: PathBindings = Arc::new(RwLock::new(bindings_map));

    tokio::spawn(serve(
        our_name.clone(),
        our_port,
        http_response_senders.clone(),
        path_bindings.clone(),
        ws_senders.clone(),
        jwt_secret_bytes.clone(),
        send_to_loop.clone(),
        print_tx.clone(),
    ));

    while let Some(km) = recv_in_server.recv().await {
        // we *can* move this into a dedicated task, but it's not necessary
        handle_app_message(
            km,
            http_response_senders.clone(),
            path_bindings.clone(),
            ws_senders.clone(),
            jwt_secret_bytes.clone(),
            send_to_loop.clone(),
            print_tx.clone(),
        )
        .await;
    }
    Err(anyhow::anyhow!("http_server: http_server loop exited"))
}

/// The 'server' part. Listens on a port assigned by runtime, and handles
/// all HTTP requests on it. Also allows incoming websocket connections.
async fn serve(
    our: Arc<String>,
    our_port: u16,
    http_response_senders: HttpResponseSenders,
    path_bindings: PathBindings,
    ws_senders: WebSocketSenders,
    jwt_secret_bytes: Arc<Vec<u8>>,
    send_to_loop: MessageSender,
    print_tx: PrintSender,
) {
    let _ = print_tx
        .send(Printout {
            verbosity: 0,
            content: format!("http_server: running on port {}", our_port),
        })
        .await;

    // Filter to receive websockets
    let cloned_msg_tx = send_to_loop.clone();
    let cloned_our = our.clone();
    let cloned_jwt_secret_bytes = jwt_secret_bytes.clone();
    let ws_route = warp::path::end()
        .and(warp::ws())
        .and(warp::any().map(move || cloned_our.clone()))
        .and(warp::any().map(move || cloned_jwt_secret_bytes.clone()))
        .and(warp::any().map(move || ws_senders.clone()))
        .and(warp::any().map(move || cloned_msg_tx.clone()))
        .map(
            |ws_connection: Ws,
             our: Arc<String>,
             jwt_secret_bytes: Arc<Vec<u8>>,
             ws_senders: WebSocketSenders,
             send_to_loop: MessageSender| {
                ws_connection.on_upgrade(move |ws: WebSocket| async move {
                    maintain_websocket(ws, our, jwt_secret_bytes, ws_senders, send_to_loop).await
                })
            },
        );
    // Filter to receive HTTP requests
    let filter = warp::filters::method::method()
        .and(warp::addr::remote())
        .and(warp::path::full())
        .and(warp::filters::header::headers_cloned())
        .and(warp::filters::body::bytes())
        .and(warp::any().map(move || our.clone()))
        .and(warp::any().map(move || http_response_senders.clone()))
        .and(warp::any().map(move || path_bindings.clone()))
        .and(warp::any().map(move || jwt_secret_bytes.clone()))
        .and(warp::any().map(move || send_to_loop.clone()))
        .and_then(http_handler);

    let filter_with_ws = ws_route.or(filter);
    warp::serve(filter_with_ws)
        .run(([0, 0, 0, 0], our_port))
        .await;
}

async fn http_handler(
    method: warp::http::Method,
    socket_addr: Option<SocketAddr>,
    path: warp::path::FullPath,
    headers: warp::http::HeaderMap,
    body: warp::hyper::body::Bytes,
    our: Arc<String>,
    http_response_senders: HttpResponseSenders,
    path_bindings: PathBindings,
    jwt_secret_bytes: Arc<Vec<u8>>,
    send_to_loop: MessageSender,
) -> Result<impl warp::Reply, warp::Rejection> {
    // TODO this is all so dirty. Figure out what actually matters.

    // trim trailing "/"
    let original_path = normalize_path(path.as_str());
    let id: u64 = rand::random();
    let serialized_headers = serialize_headers(&headers);
    let path_bindings = path_bindings.read().await;

    let Ok(route) = path_bindings.recognize(&original_path) else {
        return Ok(warp::reply::with_status(vec![], StatusCode::NOT_FOUND).into_response());
    };
    let bound_path = route.handler();

    if bound_path.authenticated {
        let auth_token = serialized_headers
            .get("cookie")
            .cloned()
            .unwrap_or_default();
        if !auth_cookie_valid(&our, &auth_token, &jwt_secret_bytes) {
            return Ok(warp::reply::with_status(vec![], StatusCode::UNAUTHORIZED).into_response());
        }
    }

    let is_local = socket_addr
        .map(|addr| addr.ip().is_loopback())
        .unwrap_or(false);

    if bound_path.local_only && !is_local {
        return Ok(warp::reply::with_status(vec![], StatusCode::FORBIDDEN).into_response());
    }

    // if path has static content, serve it
    if let Some(static_content) = &bound_path.static_content {
        return Ok(warp::http::Response::builder()
            .status(StatusCode::OK)
            .header(
                "Content-Type",
                static_content
                    .mime
                    .as_ref()
                    .unwrap_or(&"text/plain".to_string()),
            )
            .body(static_content.bytes.clone())
            .into_response());
    }

    // RPC functionality: if path is /rpc:sys:uqbar/message,
    // we extract message from base64 encoded bytes in data
    // and send it to the correct app.
    let message = if bound_path.app == "rpc:sys:uqbar" {
        match handle_rpc_message(our, id, body).await {
            Ok(message) => message,
            Err(e) => {
                return Ok(warp::reply::with_status(vec![], e).into_response());
            }
        }
    } else {
        // otherwise, make a message to the correct app
        KernelMessage {
            id,
            source: Address {
                node: our.to_string(),
                process: HTTP_SERVER_PROCESS_ID.clone(),
            },
            target: Address {
                node: our.to_string(),
                process: bound_path.app.clone(),
            },
            rsvp: None,
            message: Message::Request(Request {
                inherit: false,
                expects_response: Some(HTTP_SELF_IMPOSED_TIMEOUT),
                ipc: serde_json::to_vec(&IncomingHttpRequest {
                    source_socket_addr: socket_addr.map(|addr| addr.to_string()),
                    method: method.to_string(),
                    raw_path: format!("http://localhost{}", original_path),
                    headers: serialized_headers,
                })
                .unwrap(),
                metadata: None,
            }),
            payload: Some(Payload {
                mime: None,
                bytes: body.to_vec(),
            }),
            signed_capabilities: None,
        }
    };

    let (response_sender, response_receiver) = tokio::sync::oneshot::channel();
    http_response_senders.insert(id, (original_path, response_sender));

    match send_to_loop.send(message).await {
        Ok(_) => {}
        Err(_) => {
            return Ok(
                warp::reply::with_status(vec![], StatusCode::INTERNAL_SERVER_ERROR).into_response(),
            );
        }
    }

    let timeout_duration = tokio::time::Duration::from_secs(HTTP_SELF_IMPOSED_TIMEOUT);
    let result = tokio::time::timeout(timeout_duration, response_receiver).await;

    let (http_response, body) = match result {
        Ok(Ok(res)) => res,
        Ok(Err(_)) => {
            return Ok(
                warp::reply::with_status(vec![], StatusCode::INTERNAL_SERVER_ERROR).into_response(),
            );
        }
        Err(_) => {
            return Ok(
                warp::reply::with_status(vec![], StatusCode::REQUEST_TIMEOUT).into_response(),
            );
        }
    };

    let reply = warp::reply::with_status(
        body,
        StatusCode::from_u16(http_response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
    );
    let mut response = reply.into_response();

    // Merge the deserialized headers into the existing headers
    let existing_headers = response.headers_mut();
    for (header_name, header_value) in deserialize_headers(http_response.headers).iter() {
        if header_name == "set-cookie" || header_name == "Set-Cookie" {
            if let Ok(cookie) = header_value.to_str() {
                let cookie_headers: Vec<&str> = cookie
                    .split("; ")
                    .filter(|&cookie| !cookie.is_empty())
                    .collect();
                for cookie_header in cookie_headers {
                    if let Ok(valid_cookie) = HeaderValue::from_str(cookie_header) {
                        existing_headers.append(header_name, valid_cookie);
                    }
                }
            }
        } else {
            existing_headers.insert(header_name.to_owned(), header_value.to_owned());
        }
    }
    Ok(response)
}

async fn handle_rpc_message(
    our: Arc<String>,
    id: u64,
    body: warp::hyper::body::Bytes,
) -> Result<KernelMessage, StatusCode> {
    let Ok(rpc_message) = serde_json::from_slice::<RpcMessage>(&body) else {
        return Err(StatusCode::BAD_REQUEST);
    };

    let Ok(target_process) = ProcessId::from_str(&rpc_message.process) else {
        return Err(StatusCode::BAD_REQUEST);
    };

    let payload: Option<Payload> = match rpc_message.data {
        None => None,
        Some(b64_bytes) => match base64::decode(b64_bytes) {
            Ok(bytes) => Some(Payload {
                mime: rpc_message.mime,
                bytes,
            }),
            Err(_) => None,
        },
    };

    Ok(KernelMessage {
        id,
        source: Address {
            node: our.to_string(),
            process: HTTP_SERVER_PROCESS_ID.clone(),
        },
        target: Address {
            node: rpc_message.node.unwrap_or(our.to_string()),
            process: target_process,
        },
        rsvp: Some(Address {
            node: our.to_string(),
            process: HTTP_SERVER_PROCESS_ID.clone(),
        }),
        message: Message::Request(Request {
            inherit: false,
            expects_response: Some(15), // NB: no effect on runtime
            ipc: match rpc_message.ipc {
                Some(ipc_string) => ipc_string.into_bytes(),
                None => Vec::new(),
            },
            metadata: rpc_message.metadata,
        }),
        payload,
        signed_capabilities: None,
    })
}

async fn maintain_websocket(
    ws: WebSocket,
    our: Arc<String>,
    jwt_secret_bytes: Arc<Vec<u8>>,
    ws_senders: WebSocketSenders,
    send_to_loop: MessageSender,
) {
    let (mut write_stream, mut read_stream) = ws.split();

    // first, receive a message from client that contains the target process
    // and the auth token

    let Some(Ok(register_msg)) = read_stream.next().await else {
        // stream closed, exit
        let stream = write_stream.reunite(read_stream).unwrap();
        let _ = stream.close().await;
        return;
    };

    let Ok(ws_register) = serde_json::from_slice::<WsRegister>(register_msg.as_bytes()) else {
        // stream error, exit
        let stream = write_stream.reunite(read_stream).unwrap();
        let _ = stream.close().await;
        return;
    };

    let Ok(owner_process) = ProcessId::from_str(&ws_register.target_process) else {
        // invalid process id, exit
        let stream = write_stream.reunite(read_stream).unwrap();
        let _ = stream.close().await;
        return;
    };

    let Ok(our_name) = verify_auth_token(&ws_register.auth_token, &jwt_secret_bytes) else {
        // invalid auth token, exit
        let stream = write_stream.reunite(read_stream).unwrap();
        let _ = stream.close().await;
        return;
    };

    if our_name != *our {
        // invalid auth token, exit
        let stream = write_stream.reunite(read_stream).unwrap();
        let _ = stream.close().await;
        return;
    }

    let ws_channel_id: u64 = rand::random();
    let (ws_sender, mut ws_receiver) = tokio::sync::mpsc::channel(100);
    ws_senders.insert(ws_channel_id, (owner_process.clone(), ws_sender));

    // respond to the client notifying them that the channel is now open
    let Ok(()) = write_stream
        .send(warp::ws::Message::text(
            serde_json::to_string(&WsRegisterResponse {
                channel_id: ws_channel_id,
            })
            .unwrap(),
        ))
        .await
    else {
        // stream error, exit
        let stream = write_stream.reunite(read_stream).unwrap();
        let _ = stream.close().await;
        return;
    };

    loop {
        tokio::select! {
            read = read_stream.next() => {
                match read {
                    None => {
                        // stream closed, remove and exit
                        websocket_close(ws_channel_id, owner_process, &ws_senders, &send_to_loop).await;
                        break;
                    }
                    Some(Err(_e)) => {
                        // stream error, remove and exit
                        websocket_close(ws_channel_id, owner_process, &ws_senders, &send_to_loop).await;
                        break;
                    }
                    Some(Ok(msg)) => {
                        // forward message to process associated with this channel
                        let _ = send_to_loop
                            .send(KernelMessage {
                                id: rand::random(),
                                source: Address {
                                    node: our.to_string(),
                                    process: HTTP_SERVER_PROCESS_ID.clone(),
                                },
                                target: Address {
                                    node: our.to_string(),
                                    process: owner_process.clone(),
                                },
                                rsvp: None,
                                message: Message::Request(Request {
                                    inherit: false,
                                    expects_response: None,
                                    ipc: serde_json::to_vec(&HttpServerAction::WebSocketPush {
                                        channel_id: ws_channel_id,
                                        message_type: WsMessageType::Binary,
                                    }).unwrap(),
                                    metadata: None,
                                }),
                                payload: Some(Payload {
                                    mime: None,
                                    bytes: msg.into_bytes(),
                                }),
                                signed_capabilities: None,
                            })
                            .await;
                    }
                }
            }
            Some(outgoing) = ws_receiver.recv() => {
                // forward message to websocket
                match write_stream.send(outgoing).await {
                    Ok(()) => continue,
                    Err(_e) => {
                        // stream error, remove and exit
                        websocket_close(ws_channel_id, owner_process, &ws_senders, &send_to_loop).await;
                        break;
                    }
                }
            }
        }
    }
    let stream = write_stream.reunite(read_stream).unwrap();
    let _ = stream.close().await;
}

async fn websocket_close(
    channel_id: u64,
    process: ProcessId,
    ws_senders: &WebSocketSenders,
    send_to_loop: &MessageSender,
) {
    ws_senders.remove(&channel_id);
    let _ = send_to_loop
        .send(KernelMessage {
            id: rand::random(),
            source: Address {
                node: "our".to_string(),
                process: HTTP_SERVER_PROCESS_ID.clone(),
            },
            target: Address {
                node: "our".to_string(),
                process,
            },
            rsvp: None,
            message: Message::Request(Request {
                inherit: false,
                expects_response: None,
                ipc: serde_json::to_vec(&HttpServerAction::WebSocketClose(channel_id)).unwrap(),
                metadata: None,
            }),
            payload: Some(Payload {
                mime: None,
                bytes: serde_json::to_vec(&RpcResponseBody {
                    ipc: Vec::new(),
                    payload: None,
                })
                .unwrap(),
            }),
            signed_capabilities: None,
        })
        .await;
}

async fn handle_app_message(
    km: KernelMessage,
    http_response_senders: HttpResponseSenders,
    path_bindings: PathBindings,
    ws_senders: WebSocketSenders,
    jwt_secret_bytes: Arc<Vec<u8>>,
    send_to_loop: MessageSender,
    print_tx: PrintSender,
) {
    // when we get a Response, try to match it to an outstanding HTTP
    // request and send it there.
    // when we get a Request, parse it into an HttpServerAction and perform it.
    match km.message {
        Message::Response((response, _context)) => {
            let Some((_id, (path, sender))) = http_response_senders.remove(&km.id) else {
                return;
            };
            // if path is /rpc/message, return accordingly with base64 encoded payload
            if path == "/rpc:sys:uqbar/message" {
                let payload = km.payload.map(|p| Payload {
                    mime: p.mime,
                    bytes: base64::encode(p.bytes).into_bytes(),
                });

                let mut default_headers = HashMap::new();
                default_headers.insert("Content-Type".to_string(), "text/html".to_string());

                let _ = sender.send((
                    HttpResponse {
                        status: 200,
                        headers: default_headers,
                    },
                    serde_json::to_vec(&RpcResponseBody {
                        ipc: response.ipc,
                        payload,
                    })
                    .unwrap(),
                ));
            } else {
                let Ok(mut response) = serde_json::from_slice::<HttpResponse>(&response.ipc) else {
                    // the receiver will automatically trigger a 503 when sender is dropped.
                    return;
                };
                // XX REFACTOR THIS:
                // for the login case, todo refactor out?
                let segments: Vec<&str> = path
                    .split('/')
                    .filter(|&segment| !segment.is_empty())
                    .collect();
                // If we're getting back a /login from a proxy (or our own node),
                // then we should generate a jwt from the secret + the name of the ship,
                // and then attach it to a header.
                if response.status < 400
                    && (segments.len() == 1 || segments.len() == 4)
                    && matches!(segments.last(), Some(&"login"))
                {
                    if let Some(auth_cookie) = response.headers.get("set-cookie") {
                        let mut ws_auth_username = km.source.node.clone();

                        if segments.len() == 4
                            && matches!(segments.first(), Some(&"http-proxy"))
                            && matches!(segments.get(1), Some(&"serve"))
                        {
                            if let Some(segment) = segments.get(2) {
                                ws_auth_username = segment.to_string();
                            }
                        }
                        if let Some(token) = register::generate_jwt(
                            jwt_secret_bytes.to_vec().as_slice(),
                            ws_auth_username.clone(),
                        ) {
                            let auth_cookie_with_ws = format!(
                                "{}; uqbar-ws-auth_{}={};",
                                auth_cookie,
                                ws_auth_username.clone(),
                                token
                            );
                            response
                                .headers
                                .insert("set-cookie".to_string(), auth_cookie_with_ws);

                            let _ = print_tx
                                .send(Printout {
                                    verbosity: 1,
                                    content: format!(
                                        "SET WS AUTH COOKIE WITH USERNAME: {}",
                                        ws_auth_username
                                    ),
                                })
                                .await;
                        }
                    }
                }
                let _ = sender.send((
                    HttpResponse {
                        status: response.status,
                        headers: response.headers,
                    },
                    match km.payload {
                        None => vec![],
                        Some(p) => p.bytes,
                    },
                ));
            }
        }
        Message::Request(Request { ref ipc, .. }) => {
            let Ok(message) = serde_json::from_slice::<HttpServerAction>(ipc) else {
                println!(
                    "http_server: got malformed request from {}: {:?}\r",
                    km.source, ipc
                );
                send_action_response(
                    km.id,
                    km.source,
                    &send_to_loop,
                    Err(HttpServerError::BadRequest {
                        req: String::from_utf8_lossy(ipc).to_string(),
                    }),
                )
                .await;
                return;
            };
            match message {
                HttpServerAction::Bind {
                    mut path,
                    authenticated,
                    local_only,
                    cache,
                } => {
                    let mut path_bindings = path_bindings.write().await;
                    if km.source.process != "homepage:homepage:uqbar" {
                        path = if path.starts_with('/') {
                            format!("/{}{}", km.source.process, path)
                        } else {
                            format!("/{}/{}", km.source.process, path)
                        };
                    }
                    if !cache {
                        // trim trailing "/"
                        path_bindings.add(
                            &normalize_path(&path),
                            BoundPath {
                                app: km.source.process.clone(),
                                authenticated,
                                local_only,
                                static_content: None,
                            },
                        );
                    } else {
                        let Some(payload) = km.payload else {
                            send_action_response(
                                km.id,
                                km.source,
                                &send_to_loop,
                                Err(HttpServerError::NoPayload),
                            )
                            .await;
                            return;
                        };
                        // trim trailing "/"
                        path_bindings.add(
                            &normalize_path(&path),
                            BoundPath {
                                app: km.source.process.clone(),
                                authenticated,
                                local_only,
                                static_content: Some(payload),
                            },
                        );
                    }
                    send_action_response(km.id, km.source, &send_to_loop, Ok(())).await;
                }
                HttpServerAction::WebSocketOpen(_) => {
                    // we cannot receive these, only send them to processes
                    send_action_response(
                        km.id,
                        km.source,
                        &send_to_loop,
                        Err(HttpServerError::WebSocketPushError {
                            error: "WebSocketOpen is not a valid request".to_string(),
                        }),
                    )
                    .await;
                }
                HttpServerAction::WebSocketPush {
                    channel_id,
                    message_type,
                } => {
                    let Some(payload) = km.payload else {
                        send_action_response(
                            km.id,
                            km.source,
                            &send_to_loop,
                            Err(HttpServerError::NoPayload),
                        )
                        .await;
                        return;
                    };
                    let ws_message = match message_type {
                        WsMessageType::Text => warp::ws::Message::text(
                            String::from_utf8_lossy(&payload.bytes).to_string(),
                        ),
                        WsMessageType::Binary => warp::ws::Message::binary(payload.bytes),
                        WsMessageType::Ping | WsMessageType::Pong => {
                            if payload.bytes.len() > 125 {
                                send_action_response(
                                    km.id,
                                    km.source,
                                    &send_to_loop,
                                    Err(HttpServerError::WebSocketPushError {
                                        error: "Ping and Pong messages must be 125 bytes or less"
                                            .to_string(),
                                    }),
                                )
                                .await;
                                return;
                            }
                            if message_type == WsMessageType::Ping {
                                warp::ws::Message::ping(payload.bytes)
                            } else {
                                warp::ws::Message::pong(payload.bytes)
                            }
                        }
                    };
                    // Send to the websocket if registered
                    if let Some(got) = ws_senders.get(&channel_id) {
                        let owner_process = &got.value().0;
                        let sender = &got.value().1;
                        if owner_process != &km.source.process {
                            send_action_response(
                                km.id,
                                km.source,
                                &send_to_loop,
                                Err(HttpServerError::WebSocketPushError {
                                    error: "WebSocket channel not owned by this process"
                                        .to_string(),
                                }),
                            )
                            .await;
                            return;
                        }
                        match sender.send(ws_message).await {
                            Ok(_) => {
                                send_action_response(km.id, km.source, &send_to_loop, Ok(())).await;
                            }
                            Err(_) => {
                                send_action_response(
                                    km.id,
                                    km.source,
                                    &send_to_loop,
                                    Err(HttpServerError::WebSocketPushError {
                                        error: "WebSocket channel closed".to_string(),
                                    }),
                                )
                                .await;
                            }
                        }
                    } else {
                        send_action_response(
                            km.id,
                            km.source,
                            &send_to_loop,
                            Err(HttpServerError::WebSocketPushError {
                                error: "WebSocket channel not found".to_string(),
                            }),
                        )
                        .await;
                    }
                }
                HttpServerAction::WebSocketClose(channel_id) => {
                    if let Some(got) = ws_senders.get(&channel_id) {
                        if got.value().0 != km.source.process {
                            send_action_response(
                                km.id,
                                km.source,
                                &send_to_loop,
                                Err(HttpServerError::WebSocketPushError {
                                    error: "WebSocket channel not owned by this process"
                                        .to_string(),
                                }),
                            )
                            .await;
                            return;
                        }
                        let _ = got.value().1.send(warp::ws::Message::close()).await;
                        ws_senders.remove(&channel_id);
                        send_action_response(km.id, km.source, &send_to_loop, Ok(())).await;
                    }
                }
            }
        }
    }
}

pub async fn send_action_response(
    id: u64,
    target: Address,
    send_to_loop: &MessageSender,
    result: Result<(), HttpServerError>,
) {
    let _ = send_to_loop
        .send(KernelMessage {
            id,
            source: Address {
                node: "our".to_string(),
                process: HTTP_SERVER_PROCESS_ID.clone(),
            },
            target,
            rsvp: None,
            message: Message::Response((
                Response {
                    inherit: false,
                    ipc: serde_json::to_vec(&result).unwrap(),
                    metadata: None,
                },
                None,
            )),
            payload: None,
            signed_capabilities: None,
        })
        .await;
}
