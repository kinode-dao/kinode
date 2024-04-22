use kinode_process_lib::{println, *};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
extern crate base64;

const ICON: &str = ""; // include_str!("icon");

#[derive(Debug, Serialize, Deserialize)]
enum SettingsRequest {}

#[derive(Debug, Serialize, Deserialize)]
struct SettingsState {
    pub settings: HashMap<String, String>,
    pub clients: HashSet<u32>, // doesn't get persisted
}

fn save_state(state: &SettingsState) {
    set_state(&bincode::serialize(&state.settings).unwrap());
}

fn load_state() -> SettingsState {
    match get_typed_state(|bytes| Ok(bincode::deserialize::<HashMap<String, String>>(bytes)?)) {
        Some(settings) => SettingsState {
            settings,
            clients: HashSet::new(),
        },
        None => SettingsState {
            settings: HashMap::new(),
            clients: HashSet::new(),
        },
    }
}

fn send_ws_update(
    our: &Address,
    open_channels: &HashSet<u32>,
    update: Vec<u8>,
) -> anyhow::Result<()> {
    for channel in open_channels {
        Request::new()
            .target((&our.node, "http_server", "distro", "sys"))
            .body(serde_json::to_vec(
                &http::HttpServerAction::WebSocketPush {
                    channel_id: *channel,
                    message_type: http::WsMessageType::Binary,
                },
            )?)
            .blob(LazyLoadBlob {
                mime: Some("application/json".to_string()),
                bytes: update.clone(),
            })
            .send()?;
    }
    Ok(())
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
    let mut state: SettingsState = load_state();
    main_loop(&our, &mut state);
}

fn main_loop(our: &Address, state: &mut SettingsState) {
    loop {
        match await_message() {
            Err(send_error) => {
                println!("got send error: {send_error:?}");
                continue;
            }
            Ok(message) => match handle_request(&our, &message, state) {
                Ok(()) => continue,
                Err(e) => println!("error handling request: {:?}", e),
            },
        }
    }
}

fn handle_request(
    our: &Address,
    message: &Message,
    state: &mut SettingsState,
) -> anyhow::Result<()> {
    if !message.is_request() {
        return Ok(());
    }
    // source node is ALWAYS ourselves since networking is disabled
    if message.source().process == "http_server:distro:sys" {
        // receive HTTP requests and websocket connection messages from our server
        match serde_json::from_slice::<http::HttpServerRequest>(message.body())? {
            http::HttpServerRequest::Http(ref incoming) => {
                match handle_http_request(our, state, incoming) {
                    Ok(()) => Ok(()),
                    Err(e) => {
                        http::send_response(
                            http::StatusCode::SERVICE_UNAVAILABLE,
                            None,
                            "Service Unavailable".to_string().as_bytes().to_vec(),
                        );
                        Err(anyhow::anyhow!("error handling http request: {e:?}"))
                    }
                }
            }
            http::HttpServerRequest::WebSocketOpen { channel_id, .. } => {
                state.clients.insert(channel_id);
                Ok(())
            }
            http::HttpServerRequest::WebSocketClose(channel_id) => {
                // client frontend closed a websocket
                state.clients.remove(&channel_id);
                Ok(())
            }
            http::HttpServerRequest::WebSocketPush { .. } => {
                // client frontend sent a websocket message
                // we don't expect this! we only use websockets to push updates
                Ok(())
            }
        }
    } else {
        let settings_request = serde_json::from_slice::<SettingsRequest>(message.body())?;
        handle_settings_request(our, state, &settings_request)
    }
}

/// Handle chess protocol messages from other nodes.
fn handle_settings_request(
    our: &Address,
    state: &mut SettingsState,
    request: &SettingsRequest,
) -> anyhow::Result<()> {
    todo!()
}

/// Handle HTTP requests from our own frontend.
fn handle_http_request(
    our: &Address,
    state: &mut SettingsState,
    http_request: &http::IncomingHttpRequest,
) -> anyhow::Result<()> {
    if http_request.bound_path(Some(&our.process.to_string())) != "/games" {
        http::send_response(
            http::StatusCode::NOT_FOUND,
            None,
            "Not Found".to_string().as_bytes().to_vec(),
        );
        return Ok(());
    }
    match http_request.method()?.as_str() {
        "GET" => Ok(http::send_response(
            http::StatusCode::OK,
            Some(HashMap::from([(
                String::from("Content-Type"),
                String::from("application/json"),
            )])),
            serde_json::to_vec(&state.settings)?,
        )),
        "POST" => {
            let Some(blob) = get_blob() else {
                return Ok(http::send_response(
                    http::StatusCode::BAD_REQUEST,
                    None,
                    vec![],
                ));
            };
            let blob_json = serde_json::from_slice::<serde_json::Value>(&blob.bytes)?;

            todo!()
        }
        "PUT" => {
            let Some(blob) = get_blob() else {
                return Ok(http::send_response(
                    http::StatusCode::BAD_REQUEST,
                    None,
                    vec![],
                ));
            };
            let blob_json = serde_json::from_slice::<serde_json::Value>(&blob.bytes)?;
            todo!()
        }
        // Any other method will be rejected.
        _ => Ok(http::send_response(
            http::StatusCode::METHOD_NOT_ALLOWED,
            None,
            vec![],
        )),
    }
}
