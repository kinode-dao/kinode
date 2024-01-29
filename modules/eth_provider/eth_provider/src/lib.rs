use kinode_process_lib::eth_alloy::{
    EthProviderRequests,
    Provider,
    RpcRequest,
    RpcResponse,
};

use kinode_process_lib::{
    Address,
    LazyLoadBlob as Blob,
    Message,
    ProcessId,
    Request, 
    Response,
    await_message,
    http,
    println
};

use kinode_process_lib::http::{
    WsMessageType,
    HttpClientError,
    HttpClientResponse,
    HttpServerRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Serialize, Deserialize)]
enum EthAction {
    Path,
}

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
    subscriptions_to_id: HashMap<String, u64>,
    id_to_process: HashMap<u64, Address>,
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

    fn new (our: Address, channel: u32) -> Self {
        Self {
            our,
            channel,
        }
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

struct Component;
impl Guest for Component {
    fn init(our: String) {

        let our: Address = our.parse().unwrap();

        match main(our) {
            Ok(_) => {}
            Err(e) => {
                println!(": error: {:?}", e);
            }
        }
    }
}

fn main(our: Address) -> anyhow::Result<()> {

    let msg = Request::new()
        .target(Address::new(&our.node, ProcessId::new(Some("eth"), "distro", "sys")))
        .body(serde_json::to_vec(&EthAction::Path).unwrap())
        .send_and_await_response(5)
        .unwrap().unwrap();

    let rpc_path = serde_json::from_slice::<RpcPath>(&msg.body()).unwrap();

    let channel = 123454321;

    let msg = http::open_ws_connection_and_await
        (our.node.clone(), rpc_path.rpc_url.unwrap(), None, channel)
            .unwrap().unwrap();
    
    let mut state = match serde_json::from_slice::<Result<HttpClientResponse, HttpClientError>>(msg.body()) {
        Ok(Ok(HttpClientResponse::WebSocketAck)) => {
            State { 
                conn: WsConnection::new(rpc_path.process_addr, channel), 
                current_id: 0,
                id_to_process: HashMap::new(),
                subscriptions_to_id: HashMap::new(),
                subscription_inits: HashSet::new(),
            }
        },
        _ => {
            return Err(anyhow::anyhow!(": failed to open ws connection"))
        }
    };

    loop {
        match await_message() {
            Ok(msg) =>  {
                if msg.is_request() {
                    let _ = handle_request(&our, msg, &mut state);
                } else {
                    let _ = handle_response(&our, msg, &mut state);
                }
            }
            Err(e) => {
                break;
            }
            _ => {}
        }
    }

    Ok(())

}


fn handle_request(our: &Address, msg: Message, state: &mut State) -> anyhow::Result<()> {

    // the body can have arbitrary data in it, and I want to detect if that data is a type of request. Do I need to match it against an enum?
    // the body of the message is definitely a request, so I want to match whatever serialized data is in msg.body()
    // what should I do here?
    match serde_json::from_slice::<EthProviderRequests>(&msg.body()) { 
        Ok(EthProviderRequests::Test) => {
            println!("~\n~\n~\n got test {:?}", msg.source());
            return Ok(());
        }
        Ok(EthProviderRequests::RpcRequest(req)) => {
            println!("~\n~\n~\n got request: {:?}", req);
            let _ = handle_rpc_request(msg, req, state);
            return Ok(());
        }
        Err(e) => { }
        _ => {}
    }

    match serde_json::from_slice::<HttpServerRequest>(&msg.body()) {

        Ok(HttpServerRequest::WebSocketPush{message_type, .. }) => {

            match message_type {
                WsMessageType::Text => {
                    println!("got text message");

                    let response = serde_json::from_slice::<serde_json::Value>(&msg.blob().unwrap().bytes).unwrap();

                    if let Some(id) = response.get("id") {
                        if state.subscription_inits.contains(&id.as_u64().unwrap()) {

                            let subscription = response
                                .get("result").unwrap()
                                .as_str().unwrap()
                                .to_string();

                            state.subscriptions_to_id.insert(subscription, id.as_u64().unwrap());

                        } else {

                            let target = state.id_to_process.get(&id.as_u64().unwrap()).unwrap();

                            Request::new()
                                .target(target.clone())
                                .body(serde_json::to_vec(&response.get("result"))?)
                                .send()?;

                        }

                    } else {

                        let result = response
                            .get("params").unwrap()
                            .get("result").unwrap()
                            .to_string();

                        let subscription = response
                            .get("params").unwrap()
                            .get("subscription").unwrap()
                            .as_str().unwrap()
                            .to_string();

                        let subscription_id = state.subscriptions_to_id.get(&subscription).unwrap();

                        let target = state.id_to_process.get(subscription_id).unwrap();

                        Request::new()
                            .target(target)
                            .body(serde_json::to_vec(&EthProviderRequests::RpcResponse(RpcResponse{ result }))?)
                            .metadata(&subscription_id.to_string())
                            .send()?;

                    }

                }
                WsMessageType::Binary => {
                    println!("got binary message");
                }
                WsMessageType::Ping | WsMessageType::Pong => {
                    println!("got ping/pong");

                }
                WsMessageType::Close => {

                }
            }
            return Ok(());
        }
        Err(e) => {
            println!("~\n~\n~\n got error: {:?}", e);
        }
        _ => {}
    }

    Ok(())
}


fn handle_rpc_request(msg: Message, req: RpcRequest, state: &mut State) -> anyhow::Result<()> {

    let current_id = state.current_id.clone();

    state.current_id += 1;

    state.id_to_process.insert(current_id.clone(), msg.source().clone());

    if (req.method == "eth_subscribe") {
        state.subscription_inits.insert(current_id.clone());
    }

    let inflight = serde_json::to_string(&json!({
        "jsonrpc": "2.0",
        "method": req.method,
        "params": serde_json::from_str::<serde_json::Value>(&req.params.clone()).unwrap(),
        "id": current_id,
    })).unwrap();

    state.conn.send(Blob {
        mime: Some("application/json".to_string()),
        bytes: inflight.into()
    });

    Ok(())
}

fn handle_response(our: &Address, msg: Message, state: &mut State) -> anyhow::Result<()> {

    Ok(())
}