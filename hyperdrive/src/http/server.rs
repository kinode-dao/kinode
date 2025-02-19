use crate::http::server_types::{
    HttpResponse, HttpServerAction, HttpServerError, HttpServerRequest, IncomingHttpRequest,
    MessageType, RpcResponseBody, WsMessageType,
};
use crate::http::utils;
use crate::keygen;
use base64::{engine::general_purpose::STANDARD as base64_standard, Engine};
use dashmap::DashMap;
use futures::{SinkExt, StreamExt};
use http::uri::Authority;
use lib::types::core::{
    check_process_id_hypermap_safe, Address, KernelCommand, KernelMessage, LazyLoadBlob, LoginInfo,
    Message, MessageReceiver, MessageSender, PrintSender, Printout, ProcessId, Request, Response,
    HTTP_SERVER_PROCESS_ID,
};
use route_recognizer::Router;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::RwLock;
use warp::{
    http::{
        header::{HeaderValue, SET_COOKIE},
        StatusCode,
    },
    ws::{WebSocket, Ws},
    Filter, Reply,
};

#[cfg(not(feature = "simulation-mode"))]
const HTTP_SELF_IMPOSED_TIMEOUT: u64 = 15;
#[cfg(feature = "simulation-mode")]
const HTTP_SELF_IMPOSED_TIMEOUT: u64 = 600;

const WS_SELF_IMPOSED_MAX_CONNECTIONS: u32 = 128;

const LOGIN_HTML: &str = include_str!("login.html");

/// mapping from a given HTTP request (assigned an ID) to the oneshot
/// channel that will get a response from the app that handles the request,
/// and a string which contains the path that the request was made to.
type HttpResponseSenders = Arc<DashMap<u64, (String, HttpSender)>>;
type HttpSender = tokio::sync::oneshot::Sender<(HttpResponse, Vec<u8>)>;

/// mapping from an open websocket connection to a channel that will ingest
/// WebSocketPush messages from the app that handles the connection, and
/// send them to the connection.
type WebSocketSenders = Arc<DashMap<u32, (ProcessId, WebSocketSender)>>;
type WebSocketSender = tokio::sync::mpsc::Sender<warp::ws::Message>;

type PathBindings = Arc<RwLock<Router<BoundPath>>>;
type WsPathBindings = Arc<RwLock<Router<BoundWsPath>>>;

struct BoundPath {
    pub app: Option<ProcessId>, // if None, path has been unbound
    pub path: String,
    pub secure_subdomain: Option<String>,
    pub authenticated: bool,
    pub local_only: bool,
    pub static_content: Option<LazyLoadBlob>, // TODO store in filesystem and cache
}

struct BoundWsPath {
    pub app: Option<ProcessId>, // if None, path has been unbound
    pub secure_subdomain: Option<String>,
    pub authenticated: bool,
    pub extension: bool,
}

async fn send_push(
    id: u64,
    lazy_load_blob: Option<LazyLoadBlob>,
    source: Address,
    send_to_loop: &MessageSender,
    ws_senders: WebSocketSenders,
    channel_id: u32,
    message_type: WsMessageType,
    maybe_ext: Option<MessageType>,
) -> bool {
    let Some(mut blob) = lazy_load_blob else {
        send_action_response(id, source, send_to_loop, Err(HttpServerError::NoBlob)).await;
        return true;
    };
    if maybe_ext.is_some() {
        let WsMessageType::Binary = message_type else {
            // TODO
            send_action_response(id, source, send_to_loop, Err(HttpServerError::NoBlob)).await;
            return true;
        };
        let action = HttpServerAction::WebSocketExtPushData {
            id,
            hyperware_message_type: maybe_ext.unwrap(),
            blob: blob.bytes,
        };
        blob.bytes = rmp_serde::to_vec_named(&action).unwrap();
    }
    let ws_message = match message_type {
        WsMessageType::Text => {
            warp::ws::Message::text(String::from_utf8_lossy(&blob.bytes).to_string())
        }
        WsMessageType::Binary => warp::ws::Message::binary(blob.bytes),
        WsMessageType::Ping | WsMessageType::Pong => {
            if blob.bytes.len() > 125 {
                send_action_response(
                    id,
                    source,
                    send_to_loop,
                    Err(HttpServerError::WsPingPongTooLong),
                )
                .await;
                return true;
            }
            if message_type == WsMessageType::Ping {
                warp::ws::Message::ping(blob.bytes)
            } else {
                warp::ws::Message::pong(blob.bytes)
            }
        }
        WsMessageType::Close => {
            unreachable!();
        }
    };
    // Send to the websocket if registered
    if let Some(got) = ws_senders.get(&channel_id) {
        let owner_process = &got.value().0;
        let sender = &got.value().1;
        if owner_process != &source.process {
            send_action_response(
                id,
                source,
                send_to_loop,
                Err(HttpServerError::WsChannelNotFound),
            )
            .await;
            return true;
        }
        match sender.send(ws_message).await {
            Ok(_) => {}
            Err(_) => {
                send_action_response(
                    id,
                    source.clone(),
                    send_to_loop,
                    Err(HttpServerError::WsChannelNotFound),
                )
                .await;
                return true;
            }
        }
    } else {
        send_action_response(
            id,
            source.clone(),
            send_to_loop,
            Err(HttpServerError::WsChannelNotFound),
        )
        .await;
        return true;
    }
    false
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
    encoded_keyfile: Vec<u8>,
    jwt_secret_bytes: Vec<u8>,
    mut recv_in_server: MessageReceiver,
    send_to_loop: MessageSender,
    print_tx: PrintSender,
) -> anyhow::Result<()> {
    let http_response_senders: HttpResponseSenders = Arc::new(DashMap::new());
    let ws_senders: WebSocketSenders = Arc::new(DashMap::new());

    let mut bindings_map: Router<BoundPath> = Router::new();

    // add local-only RPC path
    bindings_map.add(
        "/rpc:distro:sys/message",
        BoundPath {
            app: Some(ProcessId::new(Some("rpc"), "distro", "sys")),
            path: "/rpc:distro:sys/message".to_string(),
            secure_subdomain: None,
            authenticated: false,
            local_only: true,
            static_content: None,
        },
    );

    let path_bindings: PathBindings = Arc::new(RwLock::new(bindings_map));
    let ws_path_bindings: WsPathBindings = Arc::new(RwLock::new(Router::new()));

    tokio::spawn(serve(
        Arc::new(our_name),
        our_port,
        http_response_senders.clone(),
        path_bindings.clone(),
        ws_path_bindings.clone(),
        ws_senders.clone(),
        Arc::new(encoded_keyfile),
        Arc::new(jwt_secret_bytes),
        send_to_loop.clone(),
        print_tx.clone(),
    ));

    while let Some(km) = recv_in_server.recv().await {
        handle_app_message(
            km,
            http_response_senders.clone(),
            path_bindings.clone(),
            ws_path_bindings.clone(),
            ws_senders.clone(),
            send_to_loop.clone(),
            print_tx.clone(),
        )
        .await;
    }
    Err(anyhow::anyhow!("http-server: http-server loop exited"))
}

/// The 'server' part. Listens on a port assigned by runtime, and handles
/// all HTTP requests on it. Also allows incoming websocket connections.
async fn serve(
    our: Arc<String>,
    our_port: u16,
    http_response_senders: HttpResponseSenders,
    path_bindings: PathBindings,
    ws_path_bindings: WsPathBindings,
    ws_senders: WebSocketSenders,
    encoded_keyfile: Arc<Vec<u8>>,
    jwt_secret_bytes: Arc<Vec<u8>>,
    send_to_loop: MessageSender,
    print_tx: PrintSender,
) {
    // filter to receive websockets
    let cloned_our = our.clone();
    let cloned_jwt_secret_bytes = jwt_secret_bytes.clone();
    let cloned_msg_tx = send_to_loop.clone();
    let cloned_print_tx = print_tx.clone();
    let ws_route = warp::ws()
        .and(warp::addr::remote())
        .and(warp::path::full())
        .and(warp::filters::host::optional())
        .and(warp::filters::header::headers_cloned())
        .and(warp::any().map(move || cloned_our.clone()))
        .and(warp::any().map(move || cloned_jwt_secret_bytes.clone()))
        .and(warp::any().map(move || ws_senders.clone()))
        .and(warp::any().map(move || ws_path_bindings.clone()))
        .and(warp::any().map(move || cloned_msg_tx.clone()))
        .and(warp::any().map(move || cloned_print_tx.clone()))
        .and_then(ws_handler);

    #[cfg(feature = "simulation-mode")]
    let fake_node = "true";
    #[cfg(not(feature = "simulation-mode"))]
    let fake_node = "false";

    // filter to receive and handle login requests
    let login_html: Arc<String> = Arc::new(
        LOGIN_HTML
            .replace("${node}", &our)
            .replace("${fake}", fake_node),
    );
    let cloned_our = our.clone();
    let cloned_login_html: &'static str = login_html.to_string().leak();
    let login = warp::path("login").and(warp::path::end()).and(
        warp::get()
            .map(move || {
                warp::reply::with_status(warp::reply::html(cloned_login_html), StatusCode::OK)
            })
            .or(warp::post()
                .and(warp::filters::host::optional())
                .and(warp::query::<HashMap<String, String>>())
                .and(warp::body::content_length_limit(1024 * 16))
                .and(warp::body::bytes())
                .and(warp::any().map(move || cloned_our.clone()))
                .and(warp::any().map(move || encoded_keyfile.clone()))
                .and_then(login_handler)),
    );

    // filter to receive all other HTTP requests
    let filter = warp::filters::method::method()
        .and(warp::addr::remote())
        .and(warp::filters::host::optional())
        .and(warp::path::full())
        .and(warp::query::<HashMap<String, String>>())
        .and(warp::filters::header::headers_cloned())
        .and(warp::filters::body::bytes())
        .and(warp::any().map(move || our.clone()))
        .and(warp::any().map(move || http_response_senders.clone()))
        .and(warp::any().map(move || path_bindings.clone()))
        .and(warp::any().map(move || jwt_secret_bytes.clone()))
        .and(warp::any().map(move || send_to_loop.clone()))
        .and(warp::any().map(move || print_tx.clone()))
        .and(warp::any().map(move || login_html.clone()))
        .and_then(http_handler);

    let filter_with_ws = ws_route.or(login).or(filter);
    warp::serve(filter_with_ws)
        .run(([0, 0, 0, 0], our_port))
        .await;
}

/// handle non-GET requests on /login. if POST, validate password
/// and return auth token, which will be stored in a cookie.
///
/// if redirect is provided in URL, such as ?redirect=/chess:chess:sys/,
/// the browser will be redirected to that path after successful login.
async fn login_handler(
    host: Option<warp::host::Authority>,
    query_params: HashMap<String, String>,
    body: warp::hyper::body::Bytes,
    our: Arc<String>,
    encoded_keyfile: Arc<Vec<u8>>,
) -> Result<impl warp::Reply, warp::Rejection> {
    let Ok(info) = serde_json::from_slice::<LoginInfo>(&body) else {
        return Ok(warp::reply::with_status(
            warp::reply::json(&"Failed to parse login info"),
            StatusCode::BAD_REQUEST,
        )
        .into_response());
    };

    #[cfg(feature = "simulation-mode")]
    let info = LoginInfo {
        password_hash: "secret".to_string(),
        subdomain: info.subdomain,
    };

    match keygen::decode_keyfile(&encoded_keyfile, &info.password_hash) {
        Ok(keyfile) => {
            let token = match keygen::generate_jwt(
                &keyfile.jwt_secret_bytes,
                our.as_ref(),
                &info.subdomain,
            ) {
                Some(token) => token,
                None => {
                    return Ok(warp::reply::with_status(
                        warp::reply::json(&"Failed to generate JWT"),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    )
                    .into_response())
                }
            };

            let mut response = if let Some(redirect) = query_params.get("redirect") {
                warp::reply::with_status(warp::reply(), StatusCode::SEE_OTHER).into_response()
            } else {
                warp::reply::with_status(
                    warp::reply::json(&base64_standard.encode(encoded_keyfile.to_vec())),
                    StatusCode::OK,
                )
                .into_response()
            };

            let cookie = match info.subdomain.unwrap_or_default().as_str() {
                "" => format!("hyperware-auth_{our}={token};"),
                subdomain => {
                    // enforce that subdomain string only contains a-z, 0-9, ., :, and -
                    let subdomain = subdomain
                        .chars()
                        .filter(|c| {
                            c.is_ascii_alphanumeric() || c == &'-' || c == &':' || c == &'.'
                        })
                        .collect::<String>();
                    format!("hyperware-auth_{our}@{subdomain}={token};")
                }
            };

            match HeaderValue::from_str(&cookie) {
                Ok(v) => {
                    response.headers_mut().append(SET_COOKIE, v);
                    response
                        .headers_mut()
                        .append("HttpOnly", HeaderValue::from_static("true"));
                    response
                        .headers_mut()
                        .append("Secure", HeaderValue::from_static("true"));
                    response
                        .headers_mut()
                        .append("SameSite", HeaderValue::from_static("Strict"));

                    if let Some(redirect) = query_params.get("redirect") {
                        // get http/https from request headers
                        let proto = match response.headers().get("X-Forwarded-Proto") {
                            Some(proto) => proto.to_str().unwrap_or("http").to_string(),
                            None => "http".to_string(),
                        };

                        response.headers_mut().append(
                            "Location",
                            HeaderValue::from_str(&format!(
                                "{proto}://{}{redirect}",
                                host.unwrap()
                            ))
                            .unwrap(),
                        );
                        response
                            .headers_mut()
                            .append("Content-Length", HeaderValue::from_str("0").unwrap());
                    }

                    Ok(response)
                }
                Err(e) => Ok(warp::reply::with_status(
                    warp::reply::json(&format!("Failed to generate Auth JWT: {e}")),
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
                .into_response()),
            }
        }
        Err(e) => Ok(warp::reply::with_status(
            warp::reply::json(&format!("Failed to decode keyfile: {e}")),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response()),
    }
}

async fn ws_handler(
    ws_connection: Ws,
    socket_addr: Option<SocketAddr>,
    path: warp::path::FullPath,
    host: Option<warp::host::Authority>,
    headers: warp::http::HeaderMap,
    our: Arc<String>,
    jwt_secret_bytes: Arc<Vec<u8>>,
    ws_senders: WebSocketSenders,
    ws_path_bindings: WsPathBindings,
    send_to_loop: MessageSender,
    print_tx: PrintSender,
) -> Result<impl warp::Reply, warp::Rejection> {
    let original_path = utils::normalize_path(path.as_str());
    Printout::new(
        2,
        HTTP_SERVER_PROCESS_ID.clone(),
        format!("http-server: ws request for {original_path}"),
    )
    .send(&print_tx)
    .await;

    if ws_senders.len() >= WS_SELF_IMPOSED_MAX_CONNECTIONS as usize {
        Printout::new(
            0,
            HTTP_SERVER_PROCESS_ID.clone(),
            format!(
                "http-server: too many open websockets ({})! rejecting incoming",
                ws_senders.len()
            ),
        )
        .send(&print_tx)
        .await;
        return Err(warp::reject::reject());
    }

    let serialized_headers = utils::serialize_headers(&headers);

    let ws_path_bindings = ws_path_bindings.read().await;
    let Ok(route) = ws_path_bindings.recognize(original_path) else {
        return Err(warp::reject::not_found());
    };
    let bound_path = route.handler();

    let Some(app) = bound_path.app.clone() else {
        return Err(warp::reject::not_found());
    };

    if bound_path.authenticated {
        let Some(auth_token) = serialized_headers.get("cookie") else {
            return Err(warp::reject::not_found());
        };

        if let Some(ref subdomain) = bound_path.secure_subdomain {
            // assert that host matches what this app wants it to be
            let host = match host {
                Some(host) => host,
                None => return Err(warp::reject::not_found()),
            };
            // parse out subdomain from host (there can only be one)
            let request_subdomain = host.host().split('.').next().unwrap_or("");
            if request_subdomain != subdomain
                || !utils::auth_token_valid(&our, Some(&app), auth_token, &jwt_secret_bytes)
            {
                return Err(warp::reject::not_found());
            }
        } else {
            if !utils::auth_token_valid(&our, None, auth_token, &jwt_secret_bytes) {
                return Err(warp::reject::not_found());
            }
        }
    }

    let is_local = socket_addr
        .map(|addr| addr.ip().is_loopback())
        .unwrap_or(false);

    if bound_path.extension && !is_local {
        return Err(warp::reject::reject());
    }

    let extension = bound_path.extension;

    drop(ws_path_bindings);

    // stripping ProcessId from path
    let formatted_path = format!(
        "/{}",
        original_path
            .trim_start_matches('/')
            .strip_prefix(&app.to_string())
            .unwrap_or("")
            .trim_start_matches('/')
    );

    Ok(ws_connection.on_upgrade(move |ws: WebSocket| async move {
        maintain_websocket(
            ws,
            our.clone(),
            app,
            formatted_path,
            ws_senders.clone(),
            send_to_loop.clone(),
            print_tx.clone(),
            extension,
        )
        .await;
    }))
}

async fn http_handler(
    method: warp::http::Method,
    socket_addr: Option<SocketAddr>,
    host: Option<warp::host::Authority>,
    path: warp::path::FullPath,
    query_params: HashMap<String, String>,
    headers: warp::http::HeaderMap,
    body: warp::hyper::body::Bytes,
    our: Arc<String>,
    http_response_senders: HttpResponseSenders,
    path_bindings: PathBindings,
    jwt_secret_bytes: Arc<Vec<u8>>,
    send_to_loop: MessageSender,
    print_tx: PrintSender,
    login_html: Arc<String>,
) -> Result<impl warp::Reply, warp::Rejection> {
    let original_path = utils::normalize_path(path.as_str());
    let base_path = original_path.split('/').skip(1).next().unwrap_or("");
    Printout::new(
        2,
        HTTP_SERVER_PROCESS_ID.clone(),
        format!("http-server: request for {original_path}"),
    )
    .send(&print_tx)
    .await;

    let id: u64 = rand::random();
    let serialized_headers = utils::serialize_headers(&headers);

    let path_bindings = path_bindings.read().await;
    let route = if let Ok(route) = path_bindings.recognize(&original_path) {
        route
    } else if let Ok(base_route) = path_bindings.recognize(base_path) {
        // if the specific path isn't found, try the base path which should
        // be just the process ID. use the base path configuration to handle
        // paths that have not been specifically bound by that process.
        base_route
    } else {
        Printout::new(
            2,
            HTTP_SERVER_PROCESS_ID.clone(),
            format!("http-server: no route found for {original_path}"),
        )
        .send(&print_tx)
        .await;
        return Ok(warp::reply::with_status(vec![], StatusCode::NOT_FOUND).into_response());
    };
    let bound_path = route.handler();

    let Some(app) = &bound_path.app else {
        return Ok(warp::reply::with_status(vec![], StatusCode::NOT_FOUND).into_response());
    };

    let host = host.unwrap_or(warp::host::Authority::from_static("localhost"));

    if bound_path.authenticated {
        if let Some(ref subdomain) = bound_path.secure_subdomain {
            let request_subdomain = host.host().split('.').next().unwrap_or("");
            // assert that host matches what this app wants it to be
            if request_subdomain.is_empty() {
                return Ok(warp::reply::with_status(
                    "attempted to access secure subdomain without host",
                    StatusCode::UNAUTHORIZED,
                )
                .into_response());
            }
            if request_subdomain != subdomain {
                let query_string = if !query_params.is_empty() {
                    let params: Vec<String> = query_params
                        .iter()
                        .map(|(key, value)| format!("{}={}", key, value))
                        .collect();
                    format!("?{}", params.join("&"))
                } else {
                    String::new()
                };

                return Ok(warp::http::Response::builder()
                    .status(StatusCode::TEMPORARY_REDIRECT)
                    .header(
                        "Location",
                        format!(
                            "{}://{}.{}{}{}",
                            match headers.get("X-Forwarded-Proto") {
                                Some(proto) => proto.to_str().unwrap_or("http"),
                                None => "http",
                            },
                            subdomain,
                            host,
                            original_path,
                            query_string,
                        ),
                    )
                    .body(vec![])
                    .into_response());
            }
            if !utils::auth_token_valid(
                &our,
                Some(&app),
                serialized_headers.get("cookie").unwrap_or(&"".to_string()),
                &jwt_secret_bytes,
            ) {
                // redirect to login page so they can get an auth token
                return Ok(warp::http::Response::builder()
                    .status(StatusCode::OK)
                    .body(login_html.to_string())
                    .into_response());
            }
        } else {
            if !utils::auth_token_valid(
                &our,
                None,
                serialized_headers.get("cookie").unwrap_or(&"".to_string()),
                &jwt_secret_bytes,
            ) {
                // redirect to login page so they can get an auth token
                return Ok(warp::http::Response::builder()
                    .status(StatusCode::OK)
                    .body(login_html.to_string())
                    .into_response());
            }
        }
    }

    let is_local = socket_addr
        .map(|addr| addr.ip().is_loopback())
        .unwrap_or(false);

    if bound_path.local_only && !is_local {
        return Ok(warp::reply::with_status(vec![], StatusCode::FORBIDDEN).into_response());
    }

    // if path has static content and this is a GET request, serve it
    if method == warp::http::Method::GET {
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
    }

    // RPC functionality: if path is /rpc:distro:sys/message,
    // we extract message from base64 encoded bytes in data
    // and send it to the correct app.
    let (message, is_fire_and_forget) = if app == &"rpc:distro:sys" {
        match handle_rpc_message(our, id, body, print_tx).await {
            Ok((message, is_fire_and_forget)) => (message, is_fire_and_forget),
            Err(e) => {
                return Ok(warp::reply::with_status(vec![], e).into_response());
            }
        }
    } else {
        // otherwise, make a message to the correct app
        let url_params: HashMap<String, String> = route
            .params()
            .into_iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        (
            KernelMessage {
                id,
                source: Address {
                    node: our.to_string(),
                    process: HTTP_SERVER_PROCESS_ID.clone(),
                },
                target: Address {
                    node: our.to_string(),
                    process: app.clone(),
                },
                rsvp: None,
                message: Message::Request(Request {
                    inherit: false,
                    expects_response: Some(HTTP_SELF_IMPOSED_TIMEOUT),
                    body: serde_json::to_vec(&HttpServerRequest::Http(IncomingHttpRequest {
                        source_socket_addr: socket_addr.map(|addr| addr.to_string()),
                        method: method.to_string(),
                        url: format!(
                            "http://{}{}", // note that protocol is being lost here
                            host.host(),
                            original_path
                        ),
                        bound_path: bound_path.path.clone(),
                        headers: serialized_headers,
                        url_params,
                        query_params,
                    }))
                    .unwrap(),
                    metadata: None,
                    capabilities: vec![],
                }),
                lazy_load_blob: Some(LazyLoadBlob {
                    mime: None,
                    bytes: body.to_vec(),
                }),
            },
            false,
        )
    };

    // unlock to avoid deadlock with .write()s
    drop(path_bindings);

    if is_fire_and_forget {
        message.send(&send_to_loop).await;
        return Ok(warp::reply::with_status(vec![], StatusCode::OK).into_response());
    }

    let (response_sender, response_receiver) = tokio::sync::oneshot::channel();
    http_response_senders.insert(id, (original_path.to_string(), response_sender));

    message.send(&send_to_loop).await;

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
    for (header_name, header_value) in utils::deserialize_headers(http_response.headers).iter() {
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
        }
        existing_headers.insert(header_name.to_owned(), header_value.to_owned());
    }
    Ok(response)
}

async fn handle_rpc_message(
    our: Arc<String>,
    id: u64,
    body: warp::hyper::body::Bytes,
    print_tx: PrintSender,
) -> Result<(KernelMessage, bool), StatusCode> {
    let Ok(rpc_message) = serde_json::from_slice::<utils::RpcMessage>(&body) else {
        return Err(StatusCode::BAD_REQUEST);
    };

    let Ok(target_process) = rpc_message.process.parse::<ProcessId>() else {
        return Err(StatusCode::BAD_REQUEST);
    };

    Printout::new(
        2,
        HTTP_SERVER_PROCESS_ID.clone(),
        format!("http-server: passing on RPC message to {target_process}"),
    )
    .send(&print_tx)
    .await;

    let blob: Option<LazyLoadBlob> = match rpc_message.data {
        None => None,
        Some(b64_bytes) => match base64_standard.decode(b64_bytes) {
            Ok(bytes) => Some(LazyLoadBlob {
                mime: rpc_message.mime,
                bytes,
            }),
            Err(_) => None,
        },
    };

    let rsvp = rpc_message.expects_response.map(|_er| Address {
        node: our.to_string(),
        process: HTTP_SERVER_PROCESS_ID.clone(),
    });
    Ok((
        KernelMessage {
            id,
            source: Address::new(&*our, HTTP_SERVER_PROCESS_ID.clone()),
            target: Address::new(rpc_message.node.unwrap_or(our.to_string()), target_process),
            rsvp,
            message: Message::Request(Request {
                inherit: false,
                expects_response: rpc_message.expects_response,
                body: match rpc_message.body {
                    Some(body_string) => body_string.into_bytes(),
                    None => Vec::new(),
                },
                metadata: rpc_message.metadata,
                capabilities: vec![],
            }),
            lazy_load_blob: blob,
        },
        rpc_message.expects_response.is_none(),
    ))
}

fn make_websocket_message(
    our: String,
    app: ProcessId,
    channel_id: u32,
    ws_msg_type: WsMessageType,
    msg: Vec<u8>,
) -> Option<KernelMessage> {
    Some(KernelMessage {
        id: rand::random(),
        source: Address::new(&our, HTTP_SERVER_PROCESS_ID.clone()),
        target: Address::new(&our, &app),
        rsvp: None,
        message: Message::Request(Request {
            inherit: false,
            expects_response: None,
            body: serde_json::to_vec(&HttpServerRequest::WebSocketPush {
                channel_id,
                message_type: ws_msg_type,
            })
            .unwrap(),
            metadata: None,
            capabilities: vec![],
        }),
        lazy_load_blob: Some(LazyLoadBlob {
            mime: None,
            bytes: msg,
        }),
    })
}

fn make_ext_websocket_message(
    our: String,
    app: ProcessId,
    channel_id: u32,
    ws_msg_type: WsMessageType,
    msg: Vec<u8>,
) -> Option<KernelMessage> {
    let option = match rmp_serde::from_slice::<HttpServerAction>(&msg) {
        Err(_) => Some((
            rand::random(),
            Message::Request(Request {
                inherit: false,
                expects_response: None,
                body: serde_json::to_vec(&HttpServerRequest::WebSocketPush {
                    channel_id,
                    message_type: ws_msg_type,
                })
                .unwrap(),
                metadata: None,
                capabilities: vec![],
            }),
            Some(LazyLoadBlob {
                mime: None,
                bytes: msg,
            }),
        )),
        Ok(HttpServerAction::WebSocketExtPushData {
            id,
            hyperware_message_type,
            blob,
        }) => Some((
            id,
            match hyperware_message_type {
                MessageType::Request => Message::Request(Request {
                    inherit: false,
                    expects_response: None,
                    body: serde_json::to_vec(&HttpServerRequest::WebSocketPush {
                        channel_id,
                        message_type: ws_msg_type,
                    })
                    .unwrap(),
                    metadata: None,
                    capabilities: vec![],
                }),
                MessageType::Response => Message::Response((
                    Response {
                        inherit: false,
                        body: serde_json::to_vec(&HttpServerRequest::WebSocketPush {
                            channel_id,
                            message_type: ws_msg_type,
                        })
                        .unwrap(),
                        metadata: None,
                        capabilities: vec![],
                    },
                    None,
                )),
            },
            Some(LazyLoadBlob {
                mime: None,
                bytes: blob,
            }),
        )),
        Ok(m) => {
            println!("http server: got unexpected message from ext websocket: {m:?}\r");
            None
        }
    };
    let Some((id, message, blob)) = option else {
        return None;
    };

    Some(KernelMessage {
        id,
        source: Address::new(&our, HTTP_SERVER_PROCESS_ID.clone()),
        target: Address::new(&our, app),
        rsvp: None,
        message,
        lazy_load_blob: blob,
    })
}

async fn maintain_websocket(
    ws: WebSocket,
    our: Arc<String>,
    app: ProcessId,
    path: String,
    ws_senders: WebSocketSenders,
    send_to_loop: MessageSender,
    print_tx: PrintSender,
    extension: bool,
) {
    let (mut write_stream, mut read_stream) = ws.split();

    let channel_id: u32 = rand::random();
    let (ws_sender, mut ws_receiver) = tokio::sync::mpsc::channel(100);
    ws_senders.insert(channel_id, (app.clone(), ws_sender));

    Printout::new(
        2,
        HTTP_SERVER_PROCESS_ID.clone(),
        format!("http-server: new websocket connection to {app} with id {channel_id}"),
    )
    .send(&print_tx)
    .await;

    KernelMessage::builder()
        .id(rand::random())
        .source((&*our, HTTP_SERVER_PROCESS_ID.clone()))
        .target((&*our, &app))
        .message(Message::Request(Request {
            inherit: false,
            expects_response: None,
            body: serde_json::to_vec(&HttpServerRequest::WebSocketOpen { path, channel_id })
                .unwrap(),
            metadata: None,
            capabilities: vec![],
        }))
        .build()
        .unwrap()
        .send(&send_to_loop)
        .await;

    let make_ws_message = if extension {
        make_ext_websocket_message
    } else {
        make_websocket_message
    };

    loop {
        tokio::select! {
            read = read_stream.next() => {
                match read {
                    Some(Ok(msg)) => {
                        let ws_msg_type = if msg.is_text() {
                            WsMessageType::Text
                        } else if msg.is_binary() {
                            WsMessageType::Binary
                        } else if msg.is_ping() {
                            WsMessageType::Ping
                        } else if msg.is_pong() {
                            WsMessageType::Pong
                        } else {
                            WsMessageType::Close
                        };

                        if let Some(message) = make_ws_message(
                            our.to_string(),
                            app.clone(),
                            channel_id,
                            ws_msg_type,
                            msg.into_bytes(),
                        ) {
                            let _ = send_to_loop.send(message).await;
                        }
                    }
                    _ => {
                        websocket_close(channel_id, app.clone(), &ws_senders, &send_to_loop).await;
                        break;
                    }
                }
            }
            Some(outgoing) = ws_receiver.recv() => {
                match write_stream.send(outgoing).await {
                    Ok(()) => continue,
                    Err(_) => {
                        websocket_close(channel_id, app.clone(), &ws_senders, &send_to_loop).await;
                        break;
                    }
                }
            }
        }
    }
    Printout::new(
        2,
        HTTP_SERVER_PROCESS_ID.clone(),
        format!("http-server: websocket connection {channel_id} closed"),
    )
    .send(&print_tx)
    .await;
    let stream = write_stream.reunite(read_stream).unwrap();
    let _ = stream.close().await;
}

async fn websocket_close(
    channel_id: u32,
    process: ProcessId,
    ws_senders: &WebSocketSenders,
    send_to_loop: &MessageSender,
) {
    ws_senders.remove(&channel_id);
    KernelMessage::builder()
        .id(rand::random())
        .source(("our", HTTP_SERVER_PROCESS_ID.clone()))
        .target(("our", &process))
        .message(Message::Request(Request {
            inherit: false,
            expects_response: None,
            body: serde_json::to_vec(&HttpServerRequest::WebSocketClose(channel_id)).unwrap(),
            metadata: None,
            capabilities: vec![],
        }))
        .build()
        .unwrap()
        .send(send_to_loop)
        .await;
}

async fn handle_app_message(
    km: KernelMessage,
    http_response_senders: HttpResponseSenders,
    path_bindings: PathBindings,
    ws_path_bindings: WsPathBindings,
    ws_senders: WebSocketSenders,
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
            // if path is /rpc/message, return accordingly with base64 encoded blob
            if path == "/rpc:distro:sys/message" {
                let blob = km.lazy_load_blob.map(|p| LazyLoadBlob {
                    mime: p.mime,
                    bytes: base64_standard.encode(p.bytes).into_bytes(),
                });

                let mut default_headers = HashMap::new();
                default_headers.insert("Content-Type".to_string(), "text/html".to_string());

                let _ = sender.send((
                    HttpResponse {
                        status: 200,
                        headers: default_headers,
                    },
                    serde_json::to_vec(&RpcResponseBody {
                        body: response.body,
                        lazy_load_blob: blob,
                    })
                    .unwrap(),
                ));
            } else {
                let Ok(response) = serde_json::from_slice::<HttpResponse>(&response.body) else {
                    // the receiver will automatically trigger a 503 when sender is dropped.
                    return;
                };
                let _ = sender.send((
                    HttpResponse {
                        status: response.status,
                        headers: response.headers,
                    },
                    match km.lazy_load_blob {
                        None => vec![],
                        Some(p) => p.bytes,
                    },
                ));
            }
        }
        Message::Request(Request {
            ref body,
            expects_response,
            ..
        }) => {
            let Ok(message) = serde_json::from_slice::<HttpServerAction>(body) else {
                println!(
                    "http-server: got malformed request from {}: {:?}\r",
                    km.source, body
                );
                send_action_response(
                    km.id,
                    km.source,
                    &send_to_loop,
                    Err(HttpServerError::MalformedRequest),
                )
                .await;
                return;
            };
            match message {
                HttpServerAction::Bind {
                    path,
                    authenticated,
                    local_only,
                    cache,
                } => {
                    if check_process_id_hypermap_safe(&km.source.process).is_err() {
                        let source = km.source.clone();
                        send_action_response(
                            km.id,
                            km.source,
                            &send_to_loop,
                            Err(HttpServerError::InvalidSourceProcess),
                        )
                        .await;
                        return;
                    }
                    let path = utils::format_path_with_process(&km.source.process, &path);
                    let mut path_bindings = path_bindings.write().await;
                    Printout::new(
                        3,
                        HTTP_SERVER_PROCESS_ID.clone(),
                        format!(
                            "http: binding {path}, {}, {}, {}",
                            if authenticated {
                                "authenticated"
                            } else {
                                "unauthenticated"
                            },
                            if local_only { "local only" } else { "open" },
                            if cache { "cached" } else { "dynamic" },
                        ),
                    )
                    .send(&print_tx)
                    .await;
                    if !cache {
                        path_bindings.add(
                            &path,
                            BoundPath {
                                app: Some(km.source.process.clone()),
                                path: path.clone(),
                                secure_subdomain: None,
                                authenticated,
                                local_only,
                                static_content: None,
                            },
                        );
                    } else {
                        let Some(blob) = km.lazy_load_blob else {
                            send_action_response(
                                km.id,
                                km.source,
                                &send_to_loop,
                                Err(HttpServerError::NoBlob),
                            )
                            .await;
                            return;
                        };
                        path_bindings.add(
                            &path,
                            BoundPath {
                                app: Some(km.source.process.clone()),
                                path: path.clone(),
                                secure_subdomain: None,
                                authenticated,
                                local_only,
                                static_content: Some(blob),
                            },
                        );
                    }
                }
                HttpServerAction::SecureBind { path, cache } => {
                    if check_process_id_hypermap_safe(&km.source.process).is_err() {
                        let source = km.source.clone();
                        send_action_response(
                            km.id,
                            km.source,
                            &send_to_loop,
                            Err(HttpServerError::InvalidSourceProcess),
                        )
                        .await;
                        return;
                    }
                    let path = utils::format_path_with_process(&km.source.process, &path);
                    let subdomain = utils::generate_secure_subdomain(&km.source.process);
                    let mut path_bindings = path_bindings.write().await;
                    Printout::new(
                        3,
                        HTTP_SERVER_PROCESS_ID.clone(),
                        format!(
                            "http: binding subdomain {subdomain} with path {path}, {}",
                            if cache { "cached" } else { "dynamic" },
                        ),
                    )
                    .send(&print_tx)
                    .await;
                    if !cache {
                        path_bindings.add(
                            &path,
                            BoundPath {
                                app: Some(km.source.process.clone()),
                                path: path.clone(),
                                secure_subdomain: Some(subdomain),
                                authenticated: true,
                                local_only: false,
                                static_content: None,
                            },
                        );
                    } else {
                        let Some(blob) = km.lazy_load_blob else {
                            send_action_response(
                                km.id,
                                km.source,
                                &send_to_loop,
                                Err(HttpServerError::NoBlob),
                            )
                            .await;
                            return;
                        };
                        path_bindings.add(
                            &path,
                            BoundPath {
                                app: Some(km.source.process.clone()),
                                path: path.clone(),
                                secure_subdomain: Some(subdomain),
                                authenticated: true,
                                local_only: false,
                                static_content: Some(blob),
                            },
                        );
                    }
                }
                HttpServerAction::Unbind { path } => {
                    let path = utils::format_path_with_process(&km.source.process, &path);
                    let mut path_bindings = path_bindings.write().await;
                    path_bindings.add(
                        &path,
                        BoundPath {
                            app: None,
                            path: path.clone(),
                            secure_subdomain: None,
                            authenticated: false,
                            local_only: false,
                            static_content: None,
                        },
                    );
                }
                HttpServerAction::WebSocketBind {
                    path,
                    authenticated,
                    extension,
                } => {
                    if check_process_id_hypermap_safe(&km.source.process).is_err() {
                        let source = km.source.clone();
                        send_action_response(
                            km.id,
                            km.source,
                            &send_to_loop,
                            Err(HttpServerError::InvalidSourceProcess),
                        )
                        .await;
                        return;
                    }
                    let path = utils::format_path_with_process(&km.source.process, &path);
                    let mut ws_path_bindings = ws_path_bindings.write().await;
                    ws_path_bindings.add(
                        &path,
                        BoundWsPath {
                            app: Some(km.source.process.clone()),
                            secure_subdomain: None,
                            authenticated,
                            extension,
                        },
                    );
                }
                HttpServerAction::WebSocketSecureBind { path, extension } => {
                    if check_process_id_hypermap_safe(&km.source.process).is_err() {
                        let source = km.source.clone();
                        send_action_response(
                            km.id,
                            km.source,
                            &send_to_loop,
                            Err(HttpServerError::InvalidSourceProcess),
                        )
                        .await;
                        return;
                    }
                    let path = utils::format_path_with_process(&km.source.process, &path);
                    let subdomain = utils::generate_secure_subdomain(&km.source.process);
                    let mut ws_path_bindings = ws_path_bindings.write().await;
                    ws_path_bindings.add(
                        &path,
                        BoundWsPath {
                            app: Some(km.source.process.clone()),
                            secure_subdomain: Some(subdomain),
                            authenticated: true,
                            extension,
                        },
                    );
                }
                HttpServerAction::WebSocketUnbind { mut path } => {
                    let path = utils::format_path_with_process(&km.source.process, &path);
                    let mut ws_path_bindings = ws_path_bindings.write().await;
                    ws_path_bindings.add(
                        &path,
                        BoundWsPath {
                            app: None,
                            secure_subdomain: None,
                            authenticated: false,
                            extension: false,
                        },
                    );
                }
                HttpServerAction::WebSocketOpen { .. } => {
                    // we cannot receive these, only send them to processes
                    send_action_response(
                        km.id,
                        km.source.clone(),
                        &send_to_loop,
                        Err(HttpServerError::MalformedRequest),
                    )
                    .await;
                }
                HttpServerAction::WebSocketPush {
                    channel_id,
                    message_type,
                } => {
                    let is_return = send_push(
                        km.id,
                        km.lazy_load_blob,
                        km.source.clone(),
                        &send_to_loop,
                        ws_senders,
                        channel_id,
                        message_type,
                        None,
                    )
                    .await;
                    if is_return {
                        return;
                    }
                }
                HttpServerAction::WebSocketExtPushOutgoing {
                    channel_id,
                    message_type,
                    desired_reply_type,
                } => {
                    send_push(
                        km.id,
                        km.lazy_load_blob,
                        km.source.clone(),
                        &send_to_loop,
                        ws_senders,
                        channel_id,
                        message_type,
                        Some(desired_reply_type),
                    )
                    .await;
                    return;
                }
                HttpServerAction::WebSocketExtPushData { .. } => {
                    send_action_response(
                        km.id,
                        km.source,
                        &send_to_loop,
                        Err(HttpServerError::MalformedRequest),
                    )
                    .await;
                    return;
                }
                HttpServerAction::WebSocketClose(channel_id) => {
                    if let Some(got) = ws_senders.get(&channel_id) {
                        if got.value().0 != km.source.process {
                            send_action_response(
                                km.id,
                                km.source,
                                &send_to_loop,
                                Err(HttpServerError::WsChannelNotFound),
                            )
                            .await;
                            return;
                        }
                        let _ = got.value().1.send(warp::ws::Message::close()).await;
                        ws_senders.remove(&channel_id);
                    }
                }
            }
            if km.rsvp.is_some() || expects_response.is_some() {
                let target = km.rsvp.unwrap_or(km.source);
                send_action_response(km.id, target, &send_to_loop, Ok(())).await;
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
    KernelMessage::builder()
        .id(id)
        .source(("our", HTTP_SERVER_PROCESS_ID.clone()))
        .target(target)
        .message(Message::Response((
            Response {
                inherit: false,
                body: serde_json::to_vec(&result).unwrap(),
                metadata: None,
                capabilities: vec![],
            },
            None,
        )))
        .build()
        .unwrap()
        .send(send_to_loop)
        .await;
}
