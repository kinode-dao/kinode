use kinode_process_lib::{println, *};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
extern crate base64;

const ICON: &str = include_str!("icon");

#[derive(Debug, Serialize, Deserialize)]
enum SettingsRequest {
    Hi {
        node: NodeId,
        content: String,
        timeout: u64,
    },
    PeerId(NodeId),
    EthConfig(eth::EthConfigAction),
    Shutdown,
    KillProcess(ProcessId),
}

type SettingsResponse = Result<Option<SettingsData>, SettingsError>;

#[derive(Debug, Serialize, Deserialize)]
enum SettingsData {
    PeerId(net::Identity),
}

#[derive(Debug, Serialize, Deserialize)]
enum SettingsError {
    HiTimeout,
    HiOffline,
    KernelNonresponsive,
    MalformedRequest,
    StateFetchFailed,
}

/// never gets persisted
#[derive(Debug, Serialize, Deserialize)]
struct SettingsState {
    pub our: Address,
    pub ws_clients: HashSet<u32>,
    pub identity: Option<net::Identity>,
    pub diagnostics: Option<String>,
    pub eth_rpc_providers: Option<eth::SavedConfigs>,
    pub eth_rpc_access_settings: Option<eth::AccessSettings>,
}

impl SettingsState {
    fn new(our: Address) -> Self {
        Self {
            our,
            ws_clients: HashSet::new(),
            identity: None,
            diagnostics: None,
            eth_rpc_providers: None,
            eth_rpc_access_settings: None,
        }
    }

    fn ws_update(&self) {
        for channel in &self.ws_clients {
            http::send_ws_push(
                *channel,
                http::WsMessageType::Text,
                LazyLoadBlob {
                    mime: Some("application/json".to_string()),
                    bytes: serde_json::to_vec(self).unwrap(),
                },
            );
        }
    }

    /// get data that the settings page presents to user
    /// - get Identity struct from net:distro:sys
    /// - get ETH RPC providers from eth:distro:sys
    /// - get ETH RPC access settings from eth:distro:sys
    /// - get running processes from kernel:distro:sys
    fn fetch(&mut self) -> anyhow::Result<()> {
        // identity
        let Ok(Ok(Message::Response { body, .. })) = Request::to(("our", "net", "distro", "sys"))
            .body(rmp_serde::to_vec(&net::NetAction::GetPeer(self.our.node.clone())).unwrap())
            .send_and_await_response(5)
        else {
            return Err(anyhow::anyhow!("failed to get identity from net"));
        };
        let Ok(net::NetResponse::Peer(Some(identity))) = rmp_serde::from_slice(&body) else {
            return Err(anyhow::anyhow!("got malformed response from net"));
        };
        self.identity = Some(identity);

        // diagnostics string
        let Ok(Ok(Message::Response { body, .. })) = Request::to(("our", "net", "distro", "sys"))
            .body(rmp_serde::to_vec(&net::NetAction::GetDiagnostics).unwrap())
            .send_and_await_response(5)
        else {
            return Err(anyhow::anyhow!("failed to get diagnostics from net"));
        };
        let Ok(net::NetResponse::Diagnostics(diagnostics_string)) = rmp_serde::from_slice(&body)
        else {
            return Err(anyhow::anyhow!("got malformed response from net"));
        };
        self.diagnostics = Some(diagnostics_string);

        // eth rpc providers
        let Ok(Ok(Message::Response { body, .. })) = Request::to(("our", "eth", "distro", "sys"))
            .body(serde_json::to_vec(&eth::EthConfigAction::GetProviders).unwrap())
            .send_and_await_response(5)
        else {
            return Err(anyhow::anyhow!("failed to get providers from eth"));
        };
        let Ok(eth::EthConfigResponse::Providers(providers)) = serde_json::from_slice(&body) else {
            return Err(anyhow::anyhow!("got malformed response from eth"));
        };
        self.eth_rpc_providers = Some(providers);

        // eth rpc access settings
        let Ok(Ok(Message::Response { body, .. })) = Request::to(("our", "eth", "distro", "sys"))
            .body(serde_json::to_vec(&eth::EthConfigAction::GetAccessSettings).unwrap())
            .send_and_await_response(5)
        else {
            return Err(anyhow::anyhow!("failed to get access settings from eth"));
        };
        let Ok(eth::EthConfigResponse::AccessSettings(access_settings)) =
            serde_json::from_slice(&body)
        else {
            return Err(anyhow::anyhow!("got malformed response from eth"));
        };
        self.eth_rpc_access_settings = Some(access_settings);

        // TODO: running processes
        Ok(())
    }
}

wit_bindgen::generate!({
    path: "wit",
    world: "process",
});

call_init!(initialize);
fn initialize(our: Address) {
    // add ourselves to the homepage
    Request::to(("our", "homepage", "homepage", "sys"))
        .body(
            serde_json::json!({
                "Add": {
                    "label": "Settings",
                    "icon": ICON,
                    "path": "/", // just our root
                }
            })
            .to_string()
            .as_bytes()
            .to_vec(),
        )
        .send()
        .unwrap();

    // Serve the index.html and other UI files found in pkg/ui at the root path.
    http::serve_ui(&our, "ui", true, false, vec!["/"]).unwrap();
    http::bind_http_path("/ask", true, false).unwrap();
    http::bind_ws_path("/", true, false).unwrap();

    // Grab our state, then enter the main event loop.
    let mut state: SettingsState = SettingsState::new(our);
    match state.fetch() {
        Ok(()) => {}
        Err(e) => {
            println!("failed to fetch initial state: {e}");
        }
    }
    main_loop(&mut state);
}

fn main_loop(state: &mut SettingsState) {
    loop {
        match await_message() {
            Err(send_error) => {
                println!("got send error: {send_error:?}");
                continue;
            }
            Ok(Message::Request {
                source,
                body,
                expects_response,
                ..
            }) => {
                if source.node() != state.our.node {
                    continue; // ignore messages from other nodes
                }
                let response = handle_request(&source, &body, state);
                if expects_response.is_some() {
                    Response::new()
                        .body(serde_json::to_vec(&response).unwrap())
                        .send()
                        .unwrap();
                }
            }
            _ => continue, // ignore responses
        }
    }
}

fn handle_request(source: &Address, body: &[u8], state: &mut SettingsState) -> SettingsResponse {
    // source node is ALWAYS ourselves since networking is disabled
    if source.process == "http_server:distro:sys" {
        // receive HTTP requests and websocket connection messages from our server
        match serde_json::from_slice::<http::HttpServerRequest>(body)
            .map_err(|_| SettingsError::MalformedRequest)?
        {
            http::HttpServerRequest::Http(ref incoming) => {
                match handle_http_request(state, incoming) {
                    Ok(()) => Ok(None),
                    Err(e) => {
                        println!("error handling HTTP request: {e}");
                        http::send_response(
                            http::StatusCode::INTERNAL_SERVER_ERROR,
                            None,
                            "Service Unavailable".to_string().as_bytes().to_vec(),
                        );
                        Ok(None)
                    }
                }
            }
            http::HttpServerRequest::WebSocketOpen { channel_id, .. } => {
                state.ws_clients.insert(channel_id);
                Ok(None)
            }
            http::HttpServerRequest::WebSocketClose(channel_id) => {
                // client frontend closed a websocket
                state.ws_clients.remove(&channel_id);
                Ok(None)
            }
            http::HttpServerRequest::WebSocketPush { .. } => {
                // client frontend sent a websocket message
                // we don't expect this! we only use websockets to push updates
                Ok(None)
            }
        }
    } else {
        let settings_request = serde_json::from_slice::<SettingsRequest>(body)
            .map_err(|_| SettingsError::MalformedRequest)?;
        handle_settings_request(state, settings_request)
    }
}

/// Handle HTTP requests from our own frontend.
fn handle_http_request(
    state: &mut SettingsState,
    http_request: &http::IncomingHttpRequest,
) -> anyhow::Result<()> {
    match http_request.method()?.as_str() {
        "GET" => Ok(http::send_response(
            http::StatusCode::OK,
            Some(HashMap::from([(
                String::from("Content-Type"),
                String::from("application/json"),
            )])),
            serde_json::to_vec(&state)?,
        )),
        "POST" => {
            let Some(blob) = get_blob() else {
                return Ok(http::send_response(
                    http::StatusCode::BAD_REQUEST,
                    None,
                    vec![],
                ));
            };
            let request = serde_json::from_slice::<SettingsRequest>(&blob.bytes)?;
            let response = handle_settings_request(state, request);
            state.ws_update();
            Ok(http::send_response(
                http::StatusCode::OK,
                None,
                match response {
                    Ok(Some(data)) => serde_json::to_vec(&data)?,
                    Ok(None) => "null".as_bytes().to_vec(),
                    Err(e) => serde_json::to_vec(&e)?,
                },
            ))
        }
        // Any other method will be rejected.
        _ => Ok(http::send_response(
            http::StatusCode::METHOD_NOT_ALLOWED,
            None,
            vec![],
        )),
    }
}

fn handle_settings_request(
    state: &mut SettingsState,
    request: SettingsRequest,
) -> SettingsResponse {
    match request {
        SettingsRequest::Hi {
            node,
            content,
            timeout,
        } => {
            if let Err(SendError { kind, .. }) = Request::to((&node, "net", "distro", "sys"))
                .body(content.into_bytes())
                .send_and_await_response(timeout)
                .unwrap()
            {
                match kind {
                    SendErrorKind::Timeout => {
                        println!("message to {node} timed out");
                        return Err(SettingsError::HiTimeout);
                    }
                    SendErrorKind::Offline => {
                        println!("{node} is offline or does not exist");
                        return Err(SettingsError::HiOffline);
                    }
                }
            } else {
                return Ok(None);
            }
        }
        SettingsRequest::PeerId(node) => {
            // get peer info
            match Request::to(("our", "net", "distro", "sys"))
                .body(rmp_serde::to_vec(&net::NetAction::GetPeer(node)).unwrap())
                .send_and_await_response(30)
                .unwrap()
            {
                Ok(msg) => match rmp_serde::from_slice::<net::NetResponse>(msg.body()) {
                    Ok(net::NetResponse::Peer(Some(peer))) => {
                        println!("got peer info: {peer:?}");
                        return Ok(Some(SettingsData::PeerId(peer)));
                    }
                    Ok(net::NetResponse::Peer(None)) => {
                        println!("peer not found");
                        return Ok(None);
                    }
                    _ => {
                        return Err(SettingsError::KernelNonresponsive);
                    }
                },
                Err(_) => {
                    return Err(SettingsError::KernelNonresponsive);
                }
            }
        }
        SettingsRequest::EthConfig(action) => {
            match Request::to(("our", "eth", "distro", "sys"))
                .body(serde_json::to_vec(&action).unwrap())
                .send_and_await_response(30)
                .unwrap()
            {
                Ok(msg) => match serde_json::from_slice::<eth::EthConfigResponse>(msg.body()) {
                    Ok(eth::EthConfigResponse::PermissionDenied) => {
                        return Err(SettingsError::KernelNonresponsive);
                    }
                    Ok(other) => {
                        println!("eth config action succeeded: {other:?}");
                    }
                    Err(_) => {
                        return Err(SettingsError::KernelNonresponsive);
                    }
                },
                Err(_) => {
                    return Err(SettingsError::KernelNonresponsive);
                }
            }
        }
        SettingsRequest::Shutdown => {
            // shutdown the node IMMEDIATELY!
            Request::to(("our", "kernel", "distro", "sys"))
                .body(serde_json::to_vec(&kernel_types::KernelCommand::Shutdown).unwrap())
                .send()
                .unwrap();
        }
        SettingsRequest::KillProcess(pid) => {
            // kill a process
            if let Err(_) = Request::to(("our", "kernel", "distro", "sys"))
                .body(serde_json::to_vec(&kernel_types::KernelCommand::KillProcess(pid)).unwrap())
                .send_and_await_response(30)
                .unwrap()
            {
                return SettingsResponse::Err(SettingsError::KernelNonresponsive);
            }
        }
    }

    state.fetch().map_err(|_| SettingsError::StateFetchFailed)?;
    state.ws_update();
    SettingsResponse::Ok(None)
}
