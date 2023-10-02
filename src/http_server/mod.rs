use crate::http_server::server_fns::*;
use crate::register;
use crate::types::*;
use anyhow::Result;

use futures::SinkExt;
use futures::StreamExt;
use serde_urlencoded;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use warp::http::{header::HeaderValue, StatusCode};
use warp::ws::{WebSocket, Ws};
use warp::{Filter, Reply};

mod server_fns;

// types and constants
type HttpSender = tokio::sync::oneshot::Sender<HttpResponse>;
type HttpResponseSenders = Arc<Mutex<HashMap<u64, (String, HttpSender)>>>;

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

    let _ = tokio::join!(
        http_serve(
            our_name.clone(),
            our_port,
            http_response_senders.clone(),
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
                    id.clone(),
                    source.clone(),
                    message,
                    payload,
                    http_response_senders.clone(),
                    websockets.clone(),
                    ws_proxies.clone(),
                    jwt_secret_bytes.clone(),
                    send_to_loop.clone(),
                    print_tx.clone(),
                )
                .await
                {
                    send_to_loop
                        .send(make_error_message(
                            our_name.clone(),
                            id.clone(),
                            source.clone(),
                            e,
                        ))
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
                    content: format!("GOT WEBSOCKET BYTES"),
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
            match serde_json::from_slice::<WebSocketClientMessage>(&bytes) {
                Ok(parsed_msg) => {
                    handle_incoming_ws(
                        parsed_msg,
                        our.clone(),
                        jwt_secret_bytes.clone().to_vec(),
                        websockets.clone(),
                        send_to_loop.clone(),
                        print_tx.clone(),
                        write_stream.clone(),
                        ws_id.clone(),
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
            match msg.to_str() {
                Ok(msg_str) => {
                    let _ = print_tx
                        .send(Printout {
                            verbosity: 1,
                            content: format!("WEBSOCKET MESSAGE (TEXT): {}", msg_str),
                        })
                        .await;
                    match serde_json::from_str(&msg_str) {
                        Ok(parsed_msg) => {
                            handle_incoming_ws(
                                parsed_msg,
                                our.clone(),
                                jwt_secret_bytes.clone().to_vec(),
                                websockets.clone(),
                                send_to_loop.clone(),
                                print_tx.clone(),
                                write_stream.clone(),
                                ws_id.clone(),
                            )
                            .await;
                        }
                        _ => (),
                    }
                }
                _ => (),
            }
        } else if msg.is_close() {
            // Delete the websocket from the map
            let mut ws_map = websockets.lock().await;
            for (node, node_map) in ws_map.iter_mut() {
                for (channel_id, id_map) in node_map.iter_mut() {
                    if let Some(_) = id_map.remove(&ws_id) {
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
    websockets: WebSockets,
    ws_proxies: WebSocketProxies,
    jwt_secret_bytes: Vec<u8>,
    send_to_loop: MessageSender,
    print_tx: PrintSender,
) -> Result<(), HttpServerError> {
    match message {
        Message::Response((ref response, _)) => {
            let mut senders = http_response_senders.lock().await;

            let json =
                serde_json::from_str::<HttpResponse>(&response.ipc.clone().unwrap_or_default());

            match json {
                Ok(mut response) => {
                    let Some(payload) = payload else {
                        return Err(HttpServerError::NoBytes);
                    };

                    let bytes = payload.bytes;

                    let _ = print_tx
                        .send(Printout {
                            verbosity: 1,
                            content: format!("ID: {}", id.to_string()),
                        })
                        .await;
                    for (id, _) in senders.iter() {
                        let _ = print_tx
                            .send(Printout {
                                verbosity: 1,
                                content: format!("existing: {}", id.to_string()),
                            })
                            .await;
                    }

                    match senders.remove(&id) {
                        Some((path, channel)) => {
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
                                        && matches!(segments.get(0), Some(&"http-proxy"))
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
                            let _ = channel.send(HttpResponse {
                                status: response.status,
                                headers: response.headers,
                                body: Some(bytes),
                            });
                        }
                        None => {
                            println!(
                                "http_server: inconsistent state, no key found for id {}",
                                id
                            );
                        }
                    }
                }
                Err(_json_parsing_err) => {
                    let mut error_headers = HashMap::new();
                    error_headers.insert("Content-Type".to_string(), "text/html".to_string());
                    match senders.remove(&id) {
                        Some((_path, channel)) => {
                            let _ = channel.send(HttpResponse {
                                status: 503,
                                headers: error_headers,
                                body: Some(format!("Internal Server Error").as_bytes().to_vec()),
                            });
                        }
                        None => {}
                    }
                }
            }
        }
        Message::Request(Request { ipc, .. }) => {
            let Some(json) = ipc else {
                return Err(HttpServerError::NoJson);
            };

            match serde_json::from_str(&json) {
                Ok(message) => {
                    match message {
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
                                            id: id.clone(),
                                            source: Address {
                                                node: our.clone(),
                                                process: ProcessId::Name("http_server".into()),
                                            },
                                            target: Address {
                                                node: proxy_node.clone(),
                                                process: ProcessId::Name("http_server".into()),
                                            },
                                            rsvp: None,
                                            message: Message::Request(Request {
                                                inherit: false,
                                                expects_response: None,
                                                ipc: Some(serde_json::json!({ // this is the JSON to forward
                                                    "WebSocketPush": {
                                                        "target": {
                                                            "node": our.clone(), // it's ultimately for us, but through the proxy
                                                            "id": channel_id.clone(),
                                                        },
                                                        "is_text": send_text,
                                                    }
                                                }).to_string()),
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
                                            let _ = locked_write_stream
                                                .send(response_data.clone())
                                                .await; // TODO: change this to binary
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
                                            let _ = locked_write_stream
                                                .send(response_data.clone())
                                                .await;
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
                                    id: id.clone(),
                                    source: Address {
                                        node: our.clone(),
                                        process: ProcessId::Name("http_server".into()),
                                    },
                                    target: source,
                                    rsvp: Some(Address {
                                        node: our.clone(),
                                        process: ProcessId::Name("http_server".into()),
                                    }),
                                    message: Message::Request(Request {
                                        inherit: false,
                                        expects_response: None,
                                        ipc: Some(
                                            serde_json::json!({
                                                "action": "set-jwt-secret"
                                            })
                                            .to_string(),
                                        ),
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
                                add_ws_proxy(ws_proxies.clone(), channel_id, source.node.clone())
                                    .await;
                            }
                        }
                        HttpServerMessage::WsProxyDisconnect(WsProxyDisconnect { channel_id }) => {
                            let _ = print_tx
                                .send(Printout {
                                    verbosity: 1,
                                    content: format!("WsDisconnect"),
                                })
                                .await;
                            // Check the ws_proxies for this channel_id, if it exists, delete the node that forwarded
                            let mut locked_proxies = ws_proxies.lock().await;
                            if let Some(proxy_nodes) = locked_proxies.get_mut(&channel_id) {
                                let _ = print_tx
                                    .send(Printout {
                                        verbosity: 1,
                                        content: format!("disconnected"),
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
                                add_ws_proxy(ws_proxies.clone(), channel_id, source.node.clone())
                                    .await;

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
                Err(_) => (),
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
                .or(warp::any().map(|| String::default()))
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
    send_to_loop: MessageSender,
    _print_tx: PrintSender,
) -> Result<impl warp::Reply, warp::Rejection> {
    let address = match address {
        Some(a) => a.to_string(),
        None => "".to_string(),
    };

    let path_str = path.as_str().to_string();
    let id: u64 = rand::random();
    let message = KernelMessage {
        id: id.clone(),
        source: Address {
            node: our.clone(),
            process: ProcessId::Name("http_server".into()),
        },
        target: Address {
            node: our.clone(),
            process: ProcessId::Name("http_bindings".into()),
        },
        rsvp: Some(Address {
            node: our.clone(),
            process: ProcessId::Name("http_server".into()),
        }),
        message: Message::Request(Request {
            inherit: false,
            expects_response: Some(30), // TODO evaluate timeout
            ipc: Some(
                serde_json::json!({
                    "action": "request".to_string(),
                    "address": address,
                    "method": method.to_string(),
                    "path": path_str.clone(),
                    "headers": serialize_headers(&headers),
                    "query_params": query_params,
                })
                .to_string(),
            ),
            metadata: None,
        }),
        payload: Some(Payload {
            mime: Some("application/octet-stream".to_string()), // TODO adjust MIME type as needed
            bytes: body.to_vec(),
        }),
        signed_capabilities: None,
    };

    let (response_sender, response_receiver) = oneshot::channel();
    http_response_senders
        .lock()
        .await
        .insert(id, (path_str, response_sender));

    send_to_loop.send(message).await.unwrap();
    let from_channel = response_receiver.await.unwrap();
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
