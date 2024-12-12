use kinode_process_lib::{
    await_message, call_init, eth, get_blob, homepage, http, kernel_types, kimap, net, println,
    Address, Capability, LazyLoadBlob, Message, NodeId, ProcessId, Request, Response, SendError,
    SendErrorKind,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    SetStylesheet(String),
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
    pub identity: Option<net::Identity>,
    pub diagnostics: Option<String>,
    pub eth_rpc_providers: Option<eth::SavedConfigs>,
    pub eth_rpc_access_settings: Option<eth::AccessSettings>,
    pub process_map: Option<kernel_types::ProcessMap>,
    pub stylesheet: Option<String>,
    pub our_tba: eth::Address,
    pub our_owner: eth::Address,
    pub net_key: Option<eth::Bytes>,  // always
    pub routers: Option<eth::Bytes>,  // if indirect
    pub ip: Option<eth::Bytes>,       // if direct
    pub ws_port: Option<eth::Bytes>,  // sometimes, if direct
    pub tcp_port: Option<eth::Bytes>, // sometimes, if direct
}

impl SettingsState {
    fn new(our: Address) -> Self {
        Self {
            our,
            identity: None,
            diagnostics: None,
            eth_rpc_providers: None,
            eth_rpc_access_settings: None,
            process_map: None,
            stylesheet: None,
            our_tba: eth::Address::ZERO,
            our_owner: eth::Address::ZERO,
            net_key: None,
            routers: None,
            ip: None,
            ws_port: None,
            tcp_port: None,
        }
    }

    fn ws_update(&self, http_server: &http::server::HttpServer) {
        http_server.ws_push_all_channels(
            "/",
            http::server::WsMessageType::Text,
            LazyLoadBlob {
                mime: Some("application/json".to_string()),
                bytes: serde_json::to_vec(self).unwrap(),
            },
        )
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

        // running processes
        let Ok(Ok(Message::Response { body, .. })) =
            Request::to(("our", "kernel", "distro", "sys"))
                .body(
                    serde_json::to_vec(&kernel_types::KernelCommand::Debug(
                        kernel_types::KernelPrint::ProcessMap,
                    ))
                    .unwrap(),
                )
                .send_and_await_response(5)
        else {
            return Err(anyhow::anyhow!(
                "failed to get running processes from kernel"
            ));
        };
        let Ok(kernel_types::KernelResponse::Debug(kernel_types::KernelPrintResponse::ProcessMap(
            process_map,
        ))) = serde_json::from_slice(&body)
        else {
            return Err(anyhow::anyhow!("got malformed response from kernel"));
        };
        self.process_map = Some(process_map);

        // stylesheet
        if let Ok(bytes) = (kinode_process_lib::vfs::File {
            path: "/homepage:sys/pkg/kinode.css".to_string(),
            timeout: 5,
        }
        .read())
        {
            self.stylesheet = Some(String::from_utf8_lossy(&bytes).to_string());
        }

        // kimap
        let kimap = kimap::Kimap::default(5);
        let Ok((tba, owner, _bytes)) = kimap.get(self.our.node()) else {
            return Err(anyhow::anyhow!("failed to get kimap node"));
        };
        self.our_tba = tba;
        self.our_owner = owner;
        let Ok((_tba, _owner, bytes)) = kimap.get(&format!("~net-key.{}", self.our.node())) else {
            return Err(anyhow::anyhow!("failed to get net-key"));
        };
        self.net_key = bytes;
        let Ok((_tba, _owner, bytes)) = kimap.get(&format!("~routers.{}", self.our.node())) else {
            return Err(anyhow::anyhow!("failed to get routers"));
        };
        self.routers = bytes;
        let Ok((_tba, _owner, bytes)) = kimap.get(&format!("~ip.{}", self.our.node())) else {
            return Err(anyhow::anyhow!("failed to get ip"));
        };
        self.ip = bytes;
        let Ok((_tba, _owner, bytes)) = kimap.get(&format!("~ws-port.{}", self.our.node())) else {
            return Err(anyhow::anyhow!("failed to get ws-port"));
        };
        self.ws_port = bytes;
        let Ok((_tba, _owner, bytes)) = kimap.get(&format!("~tcp-port.{}", self.our.node())) else {
            return Err(anyhow::anyhow!("failed to get tcp-port"));
        };
        self.tcp_port = bytes;

        Ok(())
    }
}

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v1",
});

call_init!(initialize);
fn initialize(our: Address) {
    // add ourselves to the homepage
    homepage::add_to_homepage("Settings", Some(ICON), Some("/"), None);

    // Grab our state, then enter the main event loop.
    let mut state: SettingsState = SettingsState::new(our);

    let mut http_server = http::server::HttpServer::new(5);

    // Serve the index.html and other UI files found in pkg/ui at the root path.
    // Serving securely at `settings-sys` subdomain
    http_server
        .serve_ui(
            &state.our,
            "ui",
            vec!["/"],
            http::server::HttpBindingConfig::default().secure_subdomain(true),
        )
        .unwrap();
    http_server.secure_bind_http_path("/ask").unwrap();
    http_server.secure_bind_ws_path("/").unwrap();

    main_loop(&mut state, &mut http_server);
}

fn main_loop(state: &mut SettingsState, http_server: &mut http::server::HttpServer) {
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
                let response = handle_request(&source, &body, state, http_server);
                state.ws_update(http_server);
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

fn handle_request(
    source: &Address,
    body: &[u8],
    state: &mut SettingsState,
    http_server: &mut http::server::HttpServer,
) -> SettingsResponse {
    // source node is ALWAYS ourselves since networking is disabled
    if source.process == "http-server:distro:sys" {
        // receive HTTP requests and websocket connection messages from our server
        let server_request = http_server
            .parse_request(body)
            .map_err(|_| SettingsError::MalformedRequest)?;

        http_server.handle_request(
            server_request,
            |req| {
                let result = handle_http_request(state, &req);
                match result {
                    Ok((resp, blob)) => (resp, blob),
                    Err(e) => {
                        println!("error handling HTTP request: {e}");
                        (
                            http::server::HttpResponse {
                                status: 500,
                                headers: HashMap::new(),
                            },
                            Some(LazyLoadBlob {
                                mime: Some("application/text".to_string()),
                                bytes: e.to_string().as_bytes().to_vec(),
                            }),
                        )
                    }
                }
            },
            |_channel_id, _message_type, _blob| {
                // we don't expect websocket messages
            },
        );
        Ok(None)
    } else {
        let settings_request = serde_json::from_slice::<SettingsRequest>(body)
            .map_err(|_| SettingsError::MalformedRequest)?;
        handle_settings_request(state, settings_request)
    }
}

/// Handle HTTP requests from our own frontend.
fn handle_http_request(
    state: &mut SettingsState,
    http_request: &http::server::IncomingHttpRequest,
) -> anyhow::Result<(http::server::HttpResponse, Option<LazyLoadBlob>)> {
    match http_request.method()?.as_str() {
        "GET" => {
            state.fetch()?;
            Ok((
                http::server::HttpResponse::new(http::StatusCode::OK)
                    .header("Content-Type", "application/json"),
                Some(LazyLoadBlob::new(
                    Some("application/json"),
                    serde_json::to_vec(&state)?,
                )),
            ))
        }
        "POST" => {
            let Some(blob) = get_blob() else {
                return Err(anyhow::anyhow!("malformed request"));
            };
            let request = serde_json::from_slice::<SettingsRequest>(&blob.bytes)?;
            let response = handle_settings_request(state, request);
            Ok((
                http::server::HttpResponse::new(http::StatusCode::OK)
                    .header("Content-Type", "application/json"),
                match response {
                    Ok(Some(data)) => Some(LazyLoadBlob::new(
                        Some("application/json"),
                        serde_json::to_vec(&data)?,
                    )),
                    Ok(None) => None,
                    Err(e) => Some(LazyLoadBlob::new(
                        Some("application/json"),
                        serde_json::to_vec(&e)?,
                    )),
                },
            ))
        }
        // Any other method will be rejected.
        _ => Ok((
            http::server::HttpResponse::new(http::StatusCode::METHOD_NOT_ALLOWED),
            None,
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
        SettingsRequest::SetStylesheet(stylesheet) => {
            let Ok(()) = kinode_process_lib::vfs::File {
                path: "/homepage:sys/pkg/kinode.css".to_string(),
                timeout: 5,
            }
            .write(stylesheet.as_bytes()) else {
                return SettingsResponse::Err(SettingsError::KernelNonresponsive);
            };
            Request::to(("our", "homepage", "homepage", "sys"))
                .body(
                    serde_json::json!({ "SetStylesheet": stylesheet })
                        .to_string()
                        .as_bytes(),
                )
                .capabilities(vec![Capability::new(
                    Address::new(&state.our.node, ("homepage", "homepage", "sys")),
                    "\"SetStylesheet\"".to_string(),
                )])
                .send()
                .unwrap();
            state.stylesheet = Some(stylesheet);
            return SettingsResponse::Ok(None);
        }
    }

    state.fetch().map_err(|_| SettingsError::StateFetchFailed)?;
    SettingsResponse::Ok(None)
}
