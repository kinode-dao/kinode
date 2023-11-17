use crate::http_server::server_fns::*;
use crate::register;
use crate::types::*;
use anyhow::Result;

use futures::SinkExt;
use futures::StreamExt;

use route_recognizer::Router;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use warp::http::{header::HeaderValue, StatusCode};
use warp::ws::{WebSocket, Ws};
use warp::{Filter, Reply};

mod server_fns;

// types and constants
type HttpSender = tokio::sync::oneshot::Sender<HttpResponse>;
type HttpResponseSenders = Arc<Mutex<HashMap<u64, (String, HttpSender)>>>;
type PathBindings = Arc<RwLock<Router<BoundPath>>>;

// node -> ID -> random ID

/// http driver
pub async fn http_server(
    our_name: String,
    our_port: u16,
    jwt_secret_bytes: Vec<u8>,
    mut recv_in_server: MessageReceiver,
    send_to_loop: MessageSender,
    print_tx: PrintSender,
) -> Result<()> {
    let http_response_senders = Arc::new(Mutex::new(HashMap::new()));
    let websockets: WebSockets = Arc::new(Mutex::new(HashMap::new()));
    let ws_proxies: WebSocketProxies = Arc::new(Mutex::new(HashMap::new())); // channel_id -> node

    // Add RPC path
    let mut bindings_map: Router<BoundPath> = Router::new();
    let rpc_bound_path = BoundPath {
        app: ProcessId::from_str("rpc:sys:uqbar").unwrap(),
        authenticated: false,
        local_only: true,
        original_path: "/rpc:sys:uqbar/message".to_string(),
    };
    bindings_map.add("/rpc:sys:uqbar/message", rpc_bound_path);

    // Add encryptor binding
    let encryptor_bound_path = BoundPath {
        app: ProcessId::from_str("encryptor:sys:uqbar").unwrap(),
        authenticated: false,
        local_only: true,
        original_path: "/encryptor:sys:uqbar".to_string(),
    };
    bindings_map.add("/encryptor:sys:uqbar", encryptor_bound_path);

    let path_bindings: PathBindings = Arc::new(RwLock::new(bindings_map));

    let _ = tokio::join!(
        http_serve(
            our_name.clone(),
            our_port,
            http_response_senders.clone(),
            path_bindings.clone(),
            websockets.clone(),
            jwt_secret_bytes.clone(),
            send_to_loop.clone(),
            print_tx.clone()
        ),
        async move {
            while let Some(kernel_message) = recv_in_server.recv().await {
                let KernelMessage {
                    id,
                    source,
                    message,
                    payload,
                    ..
                } = kernel_message;

                if let Err(e) = http_handle_messages(
                    our_name.clone(),
                    id,
                    source.clone(),
                    message,
                    payload,
                    http_response_senders.clone(),
                    path_bindings.clone(),
                    websockets.clone(),
                    ws_proxies.clone(),
                    jwt_secret_bytes.clone(),
                    send_to_loop.clone(),
                    print_tx.clone(),
                )
                .await
                {
                    send_to_loop
                        .send(make_error_message(our_name.clone(), id, source.clone(), e))
                        .await
                        .unwrap();
                }
            }
        }
    );
    Err(anyhow::anyhow!("http_server: exited"))
}

async fn handle_websocket(
    ws: WebSocket,
    our: String,
    jwt_secret_bytes: Vec<u8>,
    websockets: WebSockets,
    send_to_loop: MessageSender,
    print_tx: PrintSender,
) {
    let (write_stream, mut read_stream) = ws.split();
    let write_stream = Arc::new(Mutex::new(write_stream));

    // How do we handle authentication?
    let ws_id: u64 = rand::random();

    while let Some(Ok(msg)) = read_stream.next().await {
        if msg.is_binary() {
            let _ = print_tx
                .send(Printout {
                    verbosity: 1,
                    content: "GOT WEBSOCKET BYTES".to_string(),
                })
                .await;
            let bytes = msg.as_bytes();
            let _ = print_tx
                .send(Printout {
                    verbosity: 1,
                    content: format!(
                        "WEBSOCKET MESSAGE (BYTES) {}",
                        String::from_utf8(bytes.to_vec()).unwrap_or_default()
                    ),
                })
                .await;
            match serde_json::from_slice::<WebSocketClientMessage>(bytes) {
                Ok(parsed_msg) => {
                    handle_incoming_ws(
                        parsed_msg,
                        our.clone(),
                        jwt_secret_bytes.clone().to_vec(),
                        websockets.clone(),
                        send_to_loop.clone(),
                        print_tx.clone(),
                        write_stream.clone(),
                        ws_id,
                    )
                    .await;
                }
                Err(e) => {
                    let _ = print_tx
                        .send(Printout {
                            verbosity: 1,
                            content: format!("Failed to parse WebSocket message: {}", e),
                        })
                        .await;
                }
            }
        } else if msg.is_text() {
            if let Ok(msg_str) = msg.to_str() {
                let _ = print_tx
                    .send(Printout {
                        verbosity: 1,
                        content: format!("WEBSOCKET MESSAGE (TEXT): {}", msg_str),
                    })
                    .await;
                if let Ok(parsed_msg) = serde_json::from_str(msg_str) {
                    handle_incoming_ws(
                        parsed_msg,
                        our.clone(),
                        jwt_secret_bytes.clone().to_vec(),
                        websockets.clone(),
                        send_to_loop.clone(),
                        print_tx.clone(),
                        write_stream.clone(),
                        ws_id,
                    )
                    .await;
                }
            }
        } else if msg.is_close() {
            // Delete the websocket from the map
            let mut ws_map = websockets.lock().await;
            for (node, node_map) in ws_map.iter_mut() {
                for (channel_id, id_map) in node_map.iter_mut() {
                    if id_map.remove(&ws_id).is_some() {
                        // Send disconnect message
                        send_ws_disconnect(
                            node.clone(),
                            our.clone(),
                            channel_id.clone(),
                            send_to_loop.clone(),
                            print_tx.clone(),
                        )
                        .await;
                    }
                }
            }
        }
    }
}

async fn http_handle_messages(
    our: String,
    id: u64,
    source: Address,
    message: Message,
    payload: Option<Payload>,
    http_response_senders: HttpResponseSenders,
    path_bindings: PathBindings,
    websockets: WebSockets,
    ws_proxies: WebSocketProxies,
    jwt_secret_bytes: Vec<u8>,
    send_to_loop: MessageSender,
    print_tx: PrintSender,
) -> Result<(), HttpServerError> {
    match message {
        Message::Response((ref response, _)) => {
            let mut senders = http_response_senders.lock().await;
            match senders.remove(&id) {
                // if no corresponding entry, nowhere to send response
                None => {}
                Some((path, channel)) => {
                    // if path is /rpc/message, return accordingly with base64 encoded payload
                    if path == *"/rpc:sys:uqbar/message" {
                        let payload = payload.map(|p| {
                            let bytes = p.bytes;
                            let base64_bytes = base64::encode(bytes);
                            Payload {
                                mime: p.mime,
                                bytes: base64_bytes.into_bytes(),
                            }
                        });
                        let body = serde_json::json!({
                            "ipc": response.ipc,
                            "payload": payload
                        })
                        .to_string()
                        .as_bytes()
                        .to_vec();
                        let mut default_headers = HashMap::new();
                        default_headers.insert("Content-Type".to_string(), "text/html".to_string());

                        let _ = channel.send(HttpResponse {
                            status: 200,
                            headers: default_headers,
                            body: Some(body),
                        });
                        // error case here?
                    } else {
                        //  else try deserializing ipc into a HttpResponse
                        let json = serde_json::from_slice::<HttpResponse>(&response.ipc);
                        match json {
                            Ok(mut response) => {
                                let Some(payload) = payload else {
                                    return Err(HttpServerError::NoBytes);
                                };
                                let bytes = payload.bytes;

                                // for the login case, todo refactor out?
                                let segments: Vec<&str> = path
                                    .split('/')
                                    .filter(|&segment| !segment.is_empty())
                                    .collect();

                                // If we're getting back a /login from a proxy (or our own node), then we should generate a jwt from the secret + the name of the ship, and then attach it to a header
                                if response.status < 400
                                    && (segments.len() == 1 || segments.len() == 4)
                                    && matches!(segments.last(), Some(&"login"))
                                {
                                    if let Some(auth_cookie) = response.headers.get("set-cookie") {
                                        let mut ws_auth_username = our.clone();

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
                                            response.headers.insert(
                                                "set-cookie".to_string(),
                                                auth_cookie_with_ws,
                                            );

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
                                let _ = channel.send(HttpResponse {
                                    status: response.status,
                                    headers: response.headers,
                                    body: Some(bytes),
                                });
                            }
                            Err(_json_parsing_err) => {
                                let mut error_headers = HashMap::new();
                                error_headers
                                    .insert("Content-Type".to_string(), "text/html".to_string());

                                let _ = channel.send(HttpResponse {
                                    status: 503,
                                    headers: error_headers,
                                    body: Some(
                                        "Internal Server Error".to_string().as_bytes().to_vec(),
                                    ),
                                });
                            }
                        }
                    }
                }
            }
        }
        Message::Request(Request { ipc, .. }) => {
            if let Ok(message) = serde_json::from_slice(&ipc) {
                match message {
                    HttpServerMessage::BindPath {
                        path,
                        authenticated,
                        local_only,
                    } => {
                        let mut path_bindings = path_bindings.write().await;
                        let app = source.process.clone().to_string();

                        let mut path = path.clone();
                        if app != "homepage:homepage:uqbar" {
                            path = if path.starts_with('/') {
                                format!("/{}{}", app, path)
                            } else {
                                format!("/{}/{}", app, path)
                            };
                        }
                        // trim trailing "/"
                        path = normalize_path(&path);

                        let bound_path = BoundPath {
                            app: source.process,
                            authenticated,
                            local_only,
                            original_path: path.clone(),
                        };

                        path_bindings.add(&path, bound_path);
                    }
                    HttpServerMessage::WebSocketPush(WebSocketPush { target, is_text }) => {
                        let Some(payload) = payload else {
                            return Err(HttpServerError::NoBytes);
                        };
                        let bytes = payload.bytes;

                        let mut ws_map = websockets.lock().await;
                        let send_text = is_text.unwrap_or(false);
                        let response_data = if send_text {
                            warp::ws::Message::text(
                                String::from_utf8(bytes.clone()).unwrap_or_default(),
                            )
                        } else {
                            warp::ws::Message::binary(bytes.clone())
                        };

                        // Send to the proxy, if registered
                        if let Some(channel_id) = target.id.clone() {
                            let locked_proxies = ws_proxies.lock().await;

                            if let Some(proxy_nodes) = locked_proxies.get(&channel_id) {
                                for proxy_node in proxy_nodes {
                                    let id: u64 = rand::random();
                                    let bytes_content = bytes.clone();

                                    // Send a message to the encryptor
                                    let message = KernelMessage {
                                            id,
                                            source: Address {
                                                node: our.clone(),
                                                process: HTTP_SERVER_PROCESS_ID.clone(),
                                            },
                                            target: Address {
                                                node: proxy_node.clone(),
                                                process: HTTP_SERVER_PROCESS_ID.clone(),
                                            },
                                            rsvp: None,
                                            message: Message::Request(Request {
                                                inherit: false,
                                                expects_response: None,
                                                ipc: serde_json::json!({ // this is the JSON to forward
                                                    "WebSocketPush": {
                                                        "target": {
                                                            "node": our.clone(), // it's ultimately for us, but through the proxy
                                                            "id": channel_id.clone(),
                                                        },
                                                        "is_text": send_text,
                                                    }
                                                }).to_string().into_bytes(),
                                                metadata: None,
                                            }),
                                            payload: Some(Payload {
                                                mime: Some("application/octet-stream".to_string()),
                                                bytes: bytes_content,
                                            }),
                                            signed_capabilities: None,
                                        };

                                    send_to_loop.send(message).await.unwrap();
                                }
                            }
                        }

                        // Send to the websocket if registered
                        if let Some(node_map) = ws_map.get_mut(&target.node) {
                            if let Some(socket_id) = &target.id {
                                if let Some(ws_map) = node_map.get_mut(socket_id) {
                                    // Iterate over ws_map values and send message to all websockets
                                    for ws in ws_map.values_mut() {
                                        let mut locked_write_stream = ws.lock().await;
                                        let _ =
                                            locked_write_stream.send(response_data.clone()).await;
                                        // TODO: change this to binary
                                    }
                                } else {
                                    // Send to all websockets
                                    for ws_map in node_map.values_mut() {
                                        for ws in ws_map.values_mut() {
                                            let mut locked_write_stream = ws.lock().await;
                                            let _ = locked_write_stream
                                                .send(response_data.clone())
                                                .await;
                                        }
                                    }
                                }
                            } else {
                                // Send to all websockets
                                for ws_map in node_map.values_mut() {
                                    for ws in ws_map.values_mut() {
                                        let mut locked_write_stream = ws.lock().await;
                                        let _ =
                                            locked_write_stream.send(response_data.clone()).await;
                                    }
                                }
                            }
                        } else {
                            // Do nothing because we don't have a WS for that node
                        }
                    }
                    HttpServerMessage::ServerAction(ServerAction { action }) => {
                        if action == "get-jwt-secret" && source.node == our {
                            let id: u64 = rand::random();
                            let message = KernelMessage {
                                id,
                                source: Address {
                                    node: our.clone(),
                                    process: HTTP_SERVER_PROCESS_ID.clone(),
                                },
                                target: source,
                                rsvp: Some(Address {
                                    node: our.clone(),
                                    process: HTTP_SERVER_PROCESS_ID.clone(),
                                }),
                                message: Message::Request(Request {
                                    inherit: false,
                                    expects_response: None,
                                    ipc: serde_json::json!({
                                        "action": "set-jwt-secret"
                                    })
                                    .to_string()
                                    .into_bytes(),
                                    metadata: None,
                                }),
                                payload: Some(Payload {
                                    mime: Some("application/octet-stream".to_string()), // TODO adjust MIME type as needed
                                    bytes: jwt_secret_bytes.clone(),
                                }),
                                signed_capabilities: None,
                            };

                            send_to_loop.send(message).await.unwrap();
                        }
                    }
                    HttpServerMessage::WsRegister(WsRegister {
                        auth_token,
                        ws_auth_token: _,
                        channel_id,
                    }) => {
                        if let Ok(_node) =
                            parse_auth_token(auth_token, jwt_secret_bytes.clone().to_vec())
                        {
                            add_ws_proxy(ws_proxies.clone(), channel_id, source.node.clone()).await;
                        }
                    }
                    HttpServerMessage::WsProxyDisconnect(WsProxyDisconnect { channel_id }) => {
                        let _ = print_tx
                            .send(Printout {
                                verbosity: 1,
                                content: "WsDisconnect".to_string(),
                            })
                            .await;
                        // Check the ws_proxies for this channel_id, if it exists, delete the node that forwarded
                        let mut locked_proxies = ws_proxies.lock().await;
                        if let Some(proxy_nodes) = locked_proxies.get_mut(&channel_id) {
                            let _ = print_tx
                                .send(Printout {
                                    verbosity: 1,
                                    content: "disconnected".to_string(),
                                })
                                .await;
                            proxy_nodes.remove(&source.node);
                        }
                    }
                    HttpServerMessage::WsMessage(WsMessage {
                        auth_token,
                        ws_auth_token: _,
                        channel_id,
                        target,
                        json,
                    }) => {
                        if let Ok(_node) =
                            parse_auth_token(auth_token, jwt_secret_bytes.clone().to_vec())
                        {
                            add_ws_proxy(ws_proxies.clone(), channel_id, source.node.clone()).await;

                            handle_ws_message(
                                target.clone(),
                                json.clone(),
                                our.clone(),
                                send_to_loop.clone(),
                                print_tx.clone(),
                            )
                            .await;
                        }
                    }
                    HttpServerMessage::EncryptedWsMessage(EncryptedWsMessage {
                        auth_token,
                        ws_auth_token: _,
                        channel_id,
                        target,
                        encrypted,
                        nonce,
                    }) => {
                        if let Ok(_node) =
                            parse_auth_token(auth_token, jwt_secret_bytes.clone().to_vec())
                        {
                            add_ws_proxy(
                                ws_proxies.clone(),
                                channel_id.clone(),
                                source.node.clone(),
                            )
                            .await;

                            handle_encrypted_ws_message(
                                target.clone(),
                                our.clone(),
                                channel_id.clone(),
                                encrypted.clone(),
                                nonce.clone(),
                                send_to_loop.clone(),
                                print_tx.clone(),
                            )
                            .await;
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

// TODO: add a way to register a websocket connection (should be a Vector of websockets)
// Then forward websocket messages to the correct place
async fn http_serve(
    our: String,
    our_port: u16,
    http_response_senders: HttpResponseSenders,
    path_bindings: PathBindings,
    websockets: WebSockets,
    jwt_secret_bytes: Vec<u8>,
    send_to_loop: MessageSender,
    print_tx: PrintSender,
) {
    let cloned_msg_tx = send_to_loop.clone();
    let cloned_print_tx = print_tx.clone();
    let cloned_our = our.clone();
    let cloned_jwt_secret_bytes = jwt_secret_bytes.clone();
    let ws_route = warp::path::end()
        .and(warp::ws())
        .and(warp::any().map(move || cloned_our.clone()))
        .and(warp::any().map(move || cloned_jwt_secret_bytes.clone()))
        .and(warp::any().map(move || websockets.clone()))
        .and(warp::any().map(move || cloned_msg_tx.clone()))
        .and(warp::any().map(move || cloned_print_tx.clone()))
        .map(
            |ws_connection: Ws,
             our: String,
             jwt_secret_bytes: Vec<u8>,
             websockets: WebSockets,
             send_to_loop: MessageSender,
             print_tx: PrintSender| {
                ws_connection.on_upgrade(move |ws: WebSocket| async move {
                    handle_websocket(
                        ws,
                        our,
                        jwt_secret_bytes,
                        websockets,
                        send_to_loop,
                        print_tx,
                    )
                    .await
                })
            },
        );

    let print_tx_move = print_tx.clone();
    let filter = warp::filters::method::method()
        .and(warp::addr::remote())
        .and(warp::path::full())
        .and(warp::filters::header::headers_cloned())
        .and(
            warp::filters::query::raw()
                .or(warp::any().map(String::default))
                .unify()
                .map(|query_string: String| {
                    if query_string.is_empty() {
                        HashMap::new()
                    } else {
                        match serde_urlencoded::from_str(&query_string) {
                            Ok(map) => map,
                            Err(_) => HashMap::new(),
                        }
                    }
                }),
        )
        .and(warp::filters::body::bytes())
        .and(warp::any().map(move || our.clone()))
        .and(warp::any().map(move || http_response_senders.clone()))
        .and(warp::any().map(move || path_bindings.clone()))
        .and(warp::any().map(move || jwt_secret_bytes.clone()))
        .and(warp::any().map(move || send_to_loop.clone()))
        .and(warp::any().map(move || print_tx_move.clone()))
        .and_then(handler);

    let filter_with_ws = ws_route.or(filter);

    let _ = print_tx
        .send(Printout {
            verbosity: 1,
            content: format!("http_server: running on: {}", our_port),
        })
        .await;
    warp::serve(filter_with_ws)
        .run(([0, 0, 0, 0], our_port))
        .await;
}

async fn handler(
    method: warp::http::Method,
    address: Option<SocketAddr>,
    path: warp::path::FullPath,
    headers: warp::http::HeaderMap,
    query_params: HashMap<String, String>,
    body: warp::hyper::body::Bytes,
    our: String,
    http_response_senders: HttpResponseSenders,
    path_bindings: PathBindings,
    jwt_secret_bytes: Vec<u8>,
    send_to_loop: MessageSender,
    _print_tx: PrintSender,
) -> Result<impl warp::Reply, warp::Rejection> {
    let address = match address {
        Some(a) => a.to_string(),
        None => "".to_string(),
    };
    // trim trailing "/"
    let original_path = normalize_path(path.as_str());
    let id: u64 = rand::random();
    let real_headers = serialize_headers(&headers);
    let path_bindings = path_bindings.read().await;

    let Ok(route) = path_bindings.recognize(&original_path) else {
        return Ok(warp::reply::with_status(vec![], StatusCode::NOT_FOUND).into_response());
    };
    let bound_path = route.handler();

    let app = bound_path.app.to_string();
    let url_params: HashMap<&str, &str> = route.params().into_iter().collect();
    let raw_path = remove_process_id(&original_path);
    let path = remove_process_id(&bound_path.original_path);

    if bound_path.authenticated {
        let auth_token = real_headers.get("cookie").cloned().unwrap_or_default();
        if !auth_cookie_valid(our.clone(), &auth_token, jwt_secret_bytes) {
            // send 401
            return Ok(warp::reply::with_status(vec![], StatusCode::UNAUTHORIZED).into_response());
        }
    }

    if bound_path.local_only && !address.starts_with("127.0.0.1:") {
        // send 403
        return Ok(warp::reply::with_status(vec![], StatusCode::FORBIDDEN).into_response());
    }

    // RPC functionality: if path is /rpc:sys:uqbar/message,
    // we extract message from base64 encoded bytes in data
    // and send it to the correct app.

    let message = if app == *"rpc:sys:uqbar" {
        let rpc_message: RpcMessage = match serde_json::from_slice(&body) {
            // to_vec()?
            Ok(v) => v,
            Err(_) => {
                return Ok(
                    warp::reply::with_status(vec![], StatusCode::BAD_REQUEST).into_response()
                );
            }
        };

        let target_process = match ProcessId::from_str(&rpc_message.process) {
            Ok(p) => p,
            Err(_) => {
                return Ok(
                    warp::reply::with_status(vec![], StatusCode::BAD_REQUEST).into_response()
                );
            }
        };

        let payload = match base64::decode(rpc_message.data.unwrap_or("".to_string())) {
            Ok(bytes) => Some(Payload {
                mime: rpc_message.mime,
                bytes,
            }),
            Err(_) => None,
        };
        let node = match rpc_message.node {
            Some(node_str) => node_str,
            None => our.clone(),
        };

        let ipc_bytes: Vec<u8> = match rpc_message.ipc {
            Some(ipc_string) => ipc_string.into_bytes(),
            None => Vec::new(),
        };

        KernelMessage {
            id,
            source: Address {
                node: our.clone(),
                process: HTTP_SERVER_PROCESS_ID.clone(),
            },
            target: Address {
                node,
                process: target_process,
            },
            rsvp: Some(Address {
                node: our.clone(),
                process: HTTP_SERVER_PROCESS_ID.clone(),
            }),
            message: Message::Request(Request {
                inherit: false,
                expects_response: Some(15), // no effect on runtime
                ipc: ipc_bytes,
                metadata: rpc_message.metadata,
            }),
            payload,
            signed_capabilities: None,
        }
    } else if app == *"encryptor:sys:uqbar" {
        let body_json = match String::from_utf8(body.to_vec()) {
            Ok(s) => s,
            Err(_) => {
                return Ok(
                    warp::reply::with_status(vec![], StatusCode::BAD_REQUEST).into_response()
                );
            }
        };

        let body: serde_json::Value = match serde_json::from_str(&body_json) {
            Ok(v) => v,
            Err(_) => {
                return Ok(
                    warp::reply::with_status(vec![], StatusCode::BAD_REQUEST).into_response()
                );
            }
        };

        let channel_id = body["channel_id"].as_str().unwrap_or("");
        let public_key_hex = body["public_key_hex"].as_str().unwrap_or("");

        KernelMessage {
            id,
            source: Address {
                node: our.clone(),
                process: HTTP_SERVER_PROCESS_ID.clone(),
            },
            target: Address {
                node: our.clone(),
                process: ProcessId::from_str("encryptor:sys:uqbar").unwrap(),
            },
            rsvp: None, //?
            message: Message::Request(Request {
                inherit: false,
                expects_response: None,
                ipc: serde_json::json!(
                    EncryptorMessage::GetKey(
                        GetKeyAction {
                            channel_id: channel_id.to_string(),
                            public_key_hex: public_key_hex.to_string(),
                        }
                    )
                )
                .to_string()
                .into_bytes(),
                metadata: None,
            }),
            payload: None,
            signed_capabilities: None,
        }
    } else {
        // otherwise, make a message, to the correct app.
        KernelMessage {
            id,
            source: Address {
                node: our.clone(),
                process: HTTP_SERVER_PROCESS_ID.clone(),
            },
            target: Address {
                node: our.clone(),
                process: bound_path.app.clone(),
            },
            rsvp: Some(Address {
                node: our.clone(),
                process: HTTP_SERVER_PROCESS_ID.clone(),
            }),
            message: Message::Request(Request {
                inherit: false,
                expects_response: Some(15), // no effect on runtime
                ipc: serde_json::json!({
                    "address": address,
                    "method": method.to_string(),
                    "raw_path": raw_path.clone(),
                    "path": path.clone(),
                    "headers": serialize_headers(&headers),
                    "query_params": query_params,
                    "url_params": url_params,
                })
                .to_string()
                .into_bytes(),
                metadata: None,
            }),
            payload: Some(Payload {
                mime: Some("application/octet-stream".to_string()), // TODO adjust MIME type as needed
                bytes: body.to_vec(),
            }),
            signed_capabilities: None,
        }
    };
    let (response_sender, response_receiver) = oneshot::channel();
    http_response_senders
        .lock()
        .await
        .insert(id, (original_path.clone(), response_sender));

    send_to_loop.send(message).await.unwrap();
    let timeout_duration = tokio::time::Duration::from_secs(15); // adjust as needed
    let result = tokio::time::timeout(timeout_duration, response_receiver).await;

    let from_channel = match result {
        Ok(Ok(from_channel)) => from_channel,
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
        match from_channel.body {
            Some(val) => val,
            None => vec![],
        },
        StatusCode::from_u16(from_channel.status).unwrap(),
    );
    let mut response = reply.into_response();

    // Merge the deserialized headers into the existing headers
    let existing_headers = response.headers_mut();
    for (header_name, header_value) in deserialize_headers(from_channel.headers).iter() {
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
            existing_headers.insert(header_name.clone(), header_value.clone());
        }
    }
    Ok(response)
}

pub async fn find_open_port(start_at: u16) -> Option<u16> {
    for port in start_at..=u16::MAX {
        let bind_addr = format!("0.0.0.0:{}", port);
        if is_port_available(&bind_addr).await {
            return Some(port);
        }
    }
    None
}
