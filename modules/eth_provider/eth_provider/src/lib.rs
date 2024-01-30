use kinode_process_lib::{
    await_message, call_init,
    eth_alloy::{EthProviderRequests, RpcResponse},
    get_blob,
    http::{self, HttpClientError, HttpClientResponse, HttpServerRequest, WsMessageType},
    println, Address, LazyLoadBlob as Blob, Message, Request,
};

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};

wit_bindgen::generate!({
    path: "../../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

#[derive(Debug, Serialize, Deserialize)]
struct RpcPath {
    pub process_addr: Address,
    pub rpc_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct State {
    conn: WsConnection,
    subscription_inits: HashSet<u64>,
    subscriptions_to_process_id: HashMap<String, u64>,
    id_to_process_addr: HashMap<u64, Address>,
    id_to_process_id: HashMap<u64, u64>,
    current_id: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct WsConnection {
    our: Address,
    channel: u32,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct Subscriptions {
    r#type: String,
    id: u64,
    process: Option<Address>,
}

impl WsConnection {
    fn new(our: Address, channel: u32) -> Self {
        Self { our, channel }
    }

    fn send(&self, blob: Blob) {
        let _ = http::send_ws_client_push(
            self.our.node.clone(),
            self.channel,
            WsMessageType::Text,
            blob,
        );
    }
}

call_init!(init);
fn init(our: Address) {
    // listen to first message as an rpc_url initializer.
    let mut rpc_url: Option<String> = None;
    loop {
        let Ok(Message::Request { body, .. }) = await_message() else {
            continue;
        };
        rpc_url = Some(std::str::from_utf8(&body).unwrap().to_string());
        break;
    }

    let channel = 123454321;
    // open a websocket to the rpc_url, populate state.
    // todo add retry logic
    let msg = http::open_ws_connection_and_await(our.node.clone(), rpc_url.unwrap(), None, channel)
        .unwrap()
        .unwrap();

    let mut state =
        match serde_json::from_slice::<Result<HttpClientResponse, HttpClientError>>(msg.body()) {
            Ok(Ok(HttpClientResponse::WebSocketAck)) => State {
                conn: WsConnection::new(our.clone(), channel),
                current_id: 0,
                id_to_process_addr: HashMap::new(),
                id_to_process_id: HashMap::new(),
                subscriptions_to_process_id: HashMap::new(),
                subscription_inits: HashSet::new(),
            },
            _ => {
                println!("eth_provider: error: {:?}", "unexpected response");
                return;
            }
        };

    loop {
        match handle_message(&our, &mut state) {
            Ok(_) => {}
            Err(e) => {
                println!("eth_provider: error: {:?}", e);
            }
        }
    }
}

fn handle_message(our: &Address, state: &mut State) -> anyhow::Result<()> {
    let message = await_message()?;

    match message {
        Message::Request {
            source,
            body,
            metadata,
            ..
        } => {
            if source.process == "http_server:distro:sys" {
                handle_http_request(body, state)?;
            } else {
                handle_request(body, state, source, metadata)?;
            }
        }
        Message::Response { .. } => {
            println!("ok");
        }
    }

    Ok(())
}

fn handle_http_request(body: Vec<u8>, state: &mut State) -> anyhow::Result<()> {
    if let HttpServerRequest::WebSocketPush { message_type, .. } =
        serde_json::from_slice::<HttpServerRequest>(&body)?
    {
        if let WsMessageType::Text = message_type {
            let blob = match get_blob() {
                Some(blob) => blob,
                None => {
                    return Err(anyhow::anyhow!(": failed to get blob"));
                }
            };
            let response = serde_json::from_slice::<serde_json::Value>(&blob.bytes)?;

            if let Some(id) = response.get("id") {
                if state.subscription_inits.contains(&id.as_u64().unwrap()) {
                    let subscription = response
                        .get("result")
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .to_string();

                    state
                        .subscriptions_to_process_id
                        .insert(subscription, id.as_u64().unwrap());
                } else {
                    let process_addr = state.id_to_process_addr.get(&id.as_u64().unwrap()).unwrap();
                    let process_id = state.id_to_process_id.get(&id.as_u64().unwrap()).unwrap();

                    Request::new()
                        .target(process_addr.clone())
                        .body(serde_json::to_vec(&response.get("result"))?)
                        .metadata(&process_id.to_string())
                        .send()?;
                }
            } else {
                let result = response
                    .get("params")
                    .unwrap()
                    .get("result")
                    .unwrap()
                    .to_string();

                let subscription = response
                    .get("params")
                    .unwrap()
                    .get("subscription")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string();

                let process_id = state
                    .subscriptions_to_process_id
                    .get(&subscription)
                    .unwrap();
                let process_addr = state.id_to_process_addr.get(process_id).unwrap();

                Request::new()
                    .target(process_addr.clone())
                    .body(serde_json::to_vec(&EthProviderRequests::RpcResponse(
                        RpcResponse { result },
                    ))?)
                    .metadata(&process_id.to_string())
                    .send()?;
            }
        }
    }
    Ok(())
}

fn handle_request(
    body: Vec<u8>,
    state: &mut State,
    source: Address,
    metadata: Option<String>,
) -> anyhow::Result<()> {
    if let EthProviderRequests::RpcRequest(req) =
        serde_json::from_slice::<EthProviderRequests>(&body)?
    {
        let current_id = state.current_id.clone();
        state.current_id += 1;

        state
            .id_to_process_addr
            .insert(current_id.clone(), source.clone());

        state
            .id_to_process_id
            .insert(current_id.clone(), metadata.unwrap().parse().unwrap());

        if req.method == "eth_subscribe" {
            state.subscription_inits.insert(current_id.clone());
        }

        let inflight = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "method": req.method,
            "params": serde_json::from_str::<serde_json::Value>(&req.params.clone()).unwrap(),
            "id": current_id,
        }))?;

        state.conn.send(Blob {
            mime: Some("application/json".to_string()),
            bytes: inflight.into(),
        });
    }

    Ok(())
}
