use kinode_process_lib::{
    await_message, call_init, eth, get_blob, homepage, http, kernel_types, net, println, Address,
    LazyLoadBlob, Message, NodeId, ProcessId, Request, Response, SendError, SendErrorKind,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const ICON: &str = include_str!("icon");

#[derive(Debug, Serialize, Deserialize)]
struct ContactsState {
    our: Address,
}

impl ContactsState {
    fn new(our: Address) -> Self {
        Self { our }
    }
}

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v0",
});

call_init!(initialize);
fn initialize(our: Address) {
    // add ourselves to the homepage
    homepage::add_to_homepage("Contacts", Some(ICON), Some("/"), None);

    // Grab our state, then enter the main event loop.
    let mut state: ContactsState = ContactsState::new(our);

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

fn main_loop(state: &mut ContactsState, http_server: &mut http::server::HttpServer) {
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
                // state.ws_update(http_server);
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
    state: &mut ContactsState,
    http_server: &mut http::server::HttpServer,
) -> SettingsResponse {
    // source node is ALWAYS ourselves since networking is disabled
    if source.process == "http_server:distro:sys" {
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
    state: &mut ContactsState,
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
                .send()
                .unwrap();
            state.stylesheet = Some(stylesheet);
            return SettingsResponse::Ok(None);
        }
    }

    state.fetch().map_err(|_| SettingsError::StateFetchFailed)?;
    SettingsResponse::Ok(None)
}
