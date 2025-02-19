use crate::hyperware::process::settings::{
    Direct, EthConfigRequest as SettingsEthConfigAction, HiRequest, Identity as SettingsIdentity,
    NodeOrRpcUrl as SettingsNodeOrRpcUrl, NodeRouting as SettingsNodeRouting,
    Request as SettingsRequest, Response as SettingsResponse, SettingsData, SettingsError,
};
use hyperware_process_lib::{
    await_message, call_init,
    eth::{self, Provider},
    get_blob, get_capability, homepage, http, kernel_types,
    hypermap::{self, HYPERMAP_ADDRESS},
    net, println, Address, Capability, LazyLoadBlob, Message, ProcessId, Request, Response,
    SendError, SendErrorKind,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, str::FromStr, vec};

const ICON: &str = include_str!("icon");

wit_bindgen::generate!({
    path: "target/wit",
    world: "settings-sys-v0",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize],
});

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
            .send_and_await_response(60)
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
            .send_and_await_response(60)
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
            .send_and_await_response(60)
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
            .send_and_await_response(60)
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
                .send_and_await_response(60)
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
        if let Ok(bytes) = (hyperware_process_lib::vfs::File {
            path: "/homepage:sys/pkg/hyperware.css".to_string(),
            timeout: 60,
        }
        .read())
        {
            self.stylesheet = Some(String::from_utf8_lossy(&bytes).to_string());
        }

        // hypermap
        let hypermap = if cfg!(feature = "simulation-mode") {
            let fake_provider = Provider::new(31337, 60);
            hypermap::Hypermap::new(
                fake_provider,
                eth::Address::from_str(HYPERMAP_ADDRESS).unwrap(),
            )
        } else {
            hypermap::Hypermap::default(60)
        };

        let Ok((tba, owner, _bytes)) = hypermap.get(self.our.node()) else {
            return Err(anyhow::anyhow!(
                "failed to get hypermap node {} on hypermap {}",
                self.our.node(),
                hypermap.address()
            ));
        };
        self.our_tba = tba;
        self.our_owner = owner;
        let Ok((_tba, _owner, bytes)) = hypermap.get(&format!("~net-key.{}", self.our.node())) else {
            return Err(anyhow::anyhow!("failed to get net-key"));
        };
        self.net_key = bytes;
        let Ok((_tba, _owner, bytes)) = hypermap.get(&format!("~routers.{}", self.our.node())) else {
            return Err(anyhow::anyhow!("failed to get routers"));
        };
        self.routers = bytes;
        let Ok((_tba, _owner, bytes)) = hypermap.get(&format!("~ip.{}", self.our.node())) else {
            return Err(anyhow::anyhow!("failed to get ip"));
        };
        self.ip = bytes;
        let Ok((_tba, _owner, bytes)) = hypermap.get(&format!("~ws-port.{}", self.our.node())) else {
            return Err(anyhow::anyhow!("failed to get ws-port"));
        };
        self.ws_port = bytes;
        let Ok((_tba, _owner, bytes)) = hypermap.get(&format!("~tcp-port.{}", self.our.node())) else {
            return Err(anyhow::anyhow!("failed to get tcp-port"));
        };
        self.tcp_port = bytes;

        // update homepage widget
        homepage::add_to_homepage("Settings", Some(ICON), Some("/"), Some(&make_widget(self)));

        Ok(())
    }
}

call_init!(initialize);
fn initialize(our: Address) {
    // Grab our state, then enter the main event loop.
    let mut state: SettingsState = SettingsState::new(our);

    let mut http_server = http::server::HttpServer::new(60);

    // Serve the index.html and other UI files found in pkg/ui at the root path.
    // Serving securely at `settings-sys` subdomain
    http_server
        .serve_ui(
            "ui",
            vec!["/"],
            http::server::HttpBindingConfig::default().secure_subdomain(true),
        )
        .unwrap();
    http_server.secure_bind_http_path("/ask").unwrap();
    http_server.secure_bind_ws_path("/").unwrap();
    // insecure to allow widget to call refresh
    http_server
        .bind_http_path("/refresh", http::server::HttpBindingConfig::default())
        .unwrap();

    // populate state
    // this will add ourselves to the homepage
    while let Err(e) = state.fetch() {
        println!("failed to fetch settings: {e}, trying again in 5s...");
        homepage::add_to_homepage(
            "Settings",
            Some(ICON),
            Some("/"),
            Some(&make_widget(&state)),
        );
        std::thread::sleep(std::time::Duration::from_millis(5000));
    }

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
                                status: 6000,
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
    if let Ok(path) = http_request.path() {
        if &path == "/refresh" {
            state.fetch()?;
            return Ok((http::server::HttpResponse::new(http::StatusCode::OK), None));
        }
    }
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
        SettingsRequest::Hi(HiRequest {
            node,
            content,
            timeout,
        }) => {
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
                        // convert Identity to SettingsIdentity
                        let settings_identity = SettingsIdentity {
                            name: peer.name,
                            networking_key: peer.networking_key,
                            routing: match peer.routing {
                                net::NodeRouting::Direct { ip, ports } => {
                                    SettingsNodeRouting::Direct(Direct {
                                        ip,
                                        ports: ports.into_iter().map(|(p, q)| (p, q)).collect(),
                                    })
                                }
                                net::NodeRouting::Routers(routers) => {
                                    SettingsNodeRouting::Routers(routers)
                                }
                            },
                        };
                        return Ok(Some(SettingsData::PeerId(settings_identity)));
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
        SettingsRequest::EthConfig(settings_eth_config_request) => {
            // convert SettingsEthConfigRequest to EthConfigRequest
            let action = eth_config_convert(settings_eth_config_request)?;
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
        SettingsRequest::Reset => {
            // reset HNS
            let hns_address = Address::new(&state.our.node, ("hns-indexer", "hns-indexer", "sys"));
            let root_cap = get_capability(&hns_address, "{\"root\":true}");

            if let Some(cap) = root_cap {
                Request::to(("our", "hns-indexer", "hns-indexer", "sys"))
                    .body(serde_json::to_vec(&SettingsRequest::Reset).unwrap())
                    .capabilities(vec![cap])
                    .send()
                    .unwrap();
            }
        }
        SettingsRequest::KillProcess(pid_str) => {
            // kill a process
            let Ok(pid) = pid_str.parse::<ProcessId>() else {
                return SettingsResponse::Err(SettingsError::MalformedRequest);
            };
            if let Err(_) = Request::to(("our", "kernel", "distro", "sys"))
                .body(serde_json::to_vec(&kernel_types::KernelCommand::KillProcess(pid)).unwrap())
                .send_and_await_response(30)
                .unwrap()
            {
                return SettingsResponse::Err(SettingsError::KernelNonresponsive);
            }
        }
        SettingsRequest::SetStylesheet(stylesheet) => {
            let Ok(()) = hyperware_process_lib::vfs::File {
                path: "/homepage:sys/pkg/hyperware.css".to_string(),
                timeout: 60,
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

fn eth_config_convert(
    settings_eth_config_request: SettingsEthConfigAction,
) -> Result<eth::EthConfigAction, SettingsError> {
    match settings_eth_config_request {
        SettingsEthConfigAction::AddProvider(settings_provider_config) => {
            Ok(eth::EthConfigAction::AddProvider(eth::ProviderConfig {
                chain_id: settings_provider_config.chain_id,
                provider: match settings_provider_config.node_or_rpc_url {
                    SettingsNodeOrRpcUrl::Node(node_str) => {
                        // the eth module does not actually need the full routing info
                        // so we can just use the name as the hns update
                        eth::NodeOrRpcUrl::Node {
                            hns_update: net::HnsUpdate {
                                name: node_str,
                                public_key: "".to_string(),
                                ips: vec![],
                                ports: std::collections::BTreeMap::new(),
                                routers: vec![],
                            },
                            use_as_provider: true,
                        }
                    }
                    SettingsNodeOrRpcUrl::RpcUrl(url) => {
                        eth::NodeOrRpcUrl::RpcUrl { url, auth: None }
                    }
                },
                trusted: true,
            }))
        }
        SettingsEthConfigAction::RemoveProvider((chain_id, provider_str)) => Ok(
            eth::EthConfigAction::RemoveProvider((chain_id, provider_str)),
        ),
        SettingsEthConfigAction::SetPublic => Ok(eth::EthConfigAction::SetPublic),
        SettingsEthConfigAction::SetPrivate => Ok(eth::EthConfigAction::SetPrivate),
        SettingsEthConfigAction::AllowNode(node) => Ok(eth::EthConfigAction::AllowNode(node)),
        SettingsEthConfigAction::UnallowNode(node) => Ok(eth::EthConfigAction::UnallowNode(node)),
        SettingsEthConfigAction::DenyNode(node) => Ok(eth::EthConfigAction::DenyNode(node)),
        SettingsEthConfigAction::UndenyNode(node) => Ok(eth::EthConfigAction::UndenyNode(node)),
    }
}

fn make_widget(state: &SettingsState) -> String {
    let owner_string = state.our_owner.to_string();
    let tba_string = state.our_tba.to_string();
    return format!(
        r#"<html>
    <head>
        <meta name="viewport" content="width=device-width, initial-scale=1">
        <link rel="stylesheet" href="/hyperware.css">
    </head>
    <body style="margin: 0; padding: 8px; width: 100%; height: 100%; padding-bottom: 30px;">
        <article id="onchain-id">
            <h3>{}</h3>
            <details style="word-wrap: break-word;">
                <summary><p style="display: inline;">{} processes running</p></summary>
                <ul style="margin: 8px; list-style-type: none;">
                    {}
                </ul>
            </details>
            <details style="word-wrap: break-word;">
                <summary><p style="display: inline;">{} RPC providers</p></summary>
                <ul style="margin: 8px; list-style-type: none;">
                    {}
                </ul>
            </details>
        </article>

        <br />

        <article id="addrs">
            <p>owner: <a href="https://etherscan.io/address/{}#multichain-portfolio" target="_blank">{}</a></p>
            <p>token-bound account: <a href="https://etherscan.io/address/{}#multichain-portfolio" target="_blank">{}</a></p>
        </article>

        <br />

        <article id="net">
            <details style="word-wrap: break-word;">
                <summary><p style="display: inline;">{}</p></summary>
                <p style="white-space: pre; margin: 8px;">{}</p>
            </details>
        </article>

        <br />

        <button id="refresh" onclick="this.innerHTML='⌛'; fetch('/settings:settings:sys/refresh').then(() => setTimeout(() => window.location.reload(), 1000))" style="width: 30px; height: 30px; display: flex; align-items: center; justify-content: center; padding: 0; font-size: 24px;">⟳</button>

        <br />

        <a href="/settings:settings:sys/" target="_blank">Adjust Settings</a>
    </body>
    </html>"#,
        state.our.node(),
        state.process_map.as_ref().map(|m| m.len()).unwrap_or(0),
        state
            .process_map
            .as_ref()
            .map(|m| {
                let mut v = m
                    .keys()
                    .map(|pid| format!("<li>{}</li>", pid))
                    .collect::<Vec<_>>();
                v.sort();
                v.join("\n")
            })
            .unwrap_or_default(),
        state
            .eth_rpc_providers
            .as_ref()
            .map(|m| m.len())
            .unwrap_or(0),
        state
            .eth_rpc_providers
            .as_ref()
            .map(|m| {
                let mut v = m
                    .iter()
                    .filter_map(|config| {
                        match &config.provider {
                            eth::NodeOrRpcUrl::Node {
                                hns_update,
                                use_as_provider,
                            } => {
                                if *use_as_provider {
                                    Some(format!(
                                        "<li style=\"border-bottom: 1px solid black; padding: 2px;\">{}: Chain ID {}</li>",
                                        hns_update.name,
                                        config.chain_id
                                    ))
                                } else {
                                    None
                                }
                            }
                            eth::NodeOrRpcUrl::RpcUrl { url, .. } => Some(format!( // TODO
                                "<li style=\"border-bottom: 1px solid black; padding: 2px;\">{}: Chain ID {}</li>",
                                url,
                                config.chain_id
                            )),
                        }
                    })
                    .collect::<Vec<_>>();
                v.sort();
                v.join("\n")
            })
            .unwrap_or_default(),
        owner_string,
        format!(
            "{}..{}",
            &owner_string[..4],
            &owner_string[owner_string.len() - 4..]
        ),
        tba_string,
        format!(
            "{}..{}",
            &tba_string[..4],
            &tba_string[tba_string.len() - 4..]
        ),
        match &state.identity.as_ref().expect("identity not set!!").routing {
            net::NodeRouting::Direct { .. } => "direct node".to_string(),
            net::NodeRouting::Routers(routers) => format!("indirect node with {} routers", routers.len()),
        },
        match &state.identity.as_ref().expect("identity not set!!").routing {
            net::NodeRouting::Direct { ip, ports } => {
                let mut v = ports
                    .iter()
                    .map(|p| format!("{}: {}", p.0, p.1))
                    .collect::<Vec<_>>();
                v.push(format!("ip: {}", ip));
                v.join("\n")
            }
            net::NodeRouting::Routers(routers) => routers.join("\n"),
        },
    );
}
