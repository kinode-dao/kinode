use alloy_primitives::FixedBytes;
use alloy_sol_types::{sol, SolEvent};
use hex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::string::FromUtf8Error;
use uqbar_process_lib::{
    get_typed_state, http, receive, set_state, Address, Message, Payload, Request, Response,
};

wit_bindgen::generate!({
    path: "../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

struct Component;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct State {
    // namehash to human readable name
    names: HashMap<String, String>,
    // human readable name to most recent on-chain routing information as json
    // NOTE: not every namehash will have a node registered
    nodes: HashMap<String, QnsUpdate>,
    // last block we read from
    block: u64,
}

#[derive(Debug, Serialize, Deserialize)]
enum AllActions {
    EventSubscription(EthEvent),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EthEvent {
    address: String,
    block_hash: String,
    block_number: String,
    data: String,
    log_index: String,
    removed: bool,
    topics: Vec<String>,
    transaction_hash: String,
    transaction_index: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NetActions {
    QnsUpdate(QnsUpdate),
    QnsBatchUpdate(Vec<QnsUpdate>),
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct QnsUpdate {
    pub name: String, // actual username / domain name
    pub owner: String,
    pub node: String, // hex namehash of node
    pub public_key: String,
    pub ip: String,
    pub port: u16,
    pub routers: Vec<String>,
}

impl TryInto<Vec<u8>> for NetActions {
    type Error = anyhow::Error;

    fn try_into(self) -> Result<Vec<u8>, Self::Error> {
        Ok(rmp_serde::to_vec(&self)?)
    }
}

sol! {
    // Logged whenever a QNS node is created
    event NodeRegistered(bytes32 indexed node, bytes name);

    event KeyUpdate(bytes32 indexed node, bytes32 key);

    event IpUpdate(bytes32 indexed node, uint128 ip);
    event WsUpdate(bytes32 indexed node, uint16 port);
    event WtUpdate(bytes32 indexed node, uint16 port);
    event TcpUpdate(bytes32 indexed node, uint16 port);
    event UdpUpdate(bytes32 indexed node, uint16 port);

    event RoutingUpdate(bytes32 indexed node, bytes32[] routers);
}

fn subscribe_to_qns(from_block: u64) -> Vec<u8> {
    json!({
        "SubscribeEvents": {
            "addresses": [
                // QNSRegistry on sepolia
                "0x1C5595336Fd763a81887472D30D6CbD736Acf0E3",
            ],
            "from_block": from_block,
            "to_block": null,
            "events": [
                "NodeRegistered(bytes32,bytes)",
                "KeyUpdate(bytes32,bytes32)",
                "IpUpdate(bytes32,uint128)",
                "WsUpdate(bytes32,uint16)",
                "WtUpdate(bytes32,uint16)",
                "TcpUpdate(bytes32,uint16)",
                "UdpUpdate(bytes32,uint16)",
                "RoutingUpdate(bytes32,bytes32[])",
            ],
            "topic1": null,
            "topic2": null,
            "topic3": null,
        }
    })
    .to_string()
    .as_bytes()
    .to_vec()
}

impl Guest for Component {
    fn init(our: String) {
        let our = Address::from_str(&our).unwrap();

        let mut state: State = State {
            names: HashMap::new(),
            nodes: HashMap::new(),
            block: 1,
        };

        // if we have state, load it in
        match get_typed_state(|bytes| Ok(bincode::deserialize(bytes)?)) {
            Some(s) => {
                state = s;
            }
            None => {}
        }

        println!("qns_indexer: starting at block {}", state.block);

        match main(our, state) {
            Ok(_) => {}
            Err(e) => {
                println!("qns_indexer: ended with error: {:?}", e);
            }
        }
    }
}

fn main(our: Address, mut state: State) -> anyhow::Result<()> {
    // shove all state into net::net
    Request::new()
        .target((&our.node, "net", "sys", "uqbar"))
        .try_ipc(NetActions::QnsBatchUpdate(
            state.nodes.values().cloned().collect::<Vec<_>>(),
        ))?
        .send()?;

    Request::new()
        .target((&our.node, "eth_rpc", "sys", "uqbar"))
        .ipc(subscribe_to_qns(state.block - 1))
        .expects_response(5)
        .send()?;

    http::bind_http_path("/node/:name", false, false)?;

    loop {
        let Ok((source, message)) = receive() else {
            println!("qns_indexer: got network error");
            continue;
        };
        let Message::Request(request) = message else {
            // TODO we should store the subscription ID for eth_rpc
            // incase we want to cancel/reset it
            continue;
        };

        if source.process == "http_server:sys:uqbar" {
            if let Ok(ipc_json) = serde_json::from_slice::<serde_json::Value>(&request.ipc) {
                if ipc_json["path"].as_str().unwrap_or_default() == "/node/:name" {
                    if let Some(name) = ipc_json["url_params"]["name"].as_str() {
                        if let Some(node) = state.nodes.get(name) {
                            Response::new()
                                .ipc(
                                    serde_json::to_vec(&http::HttpResponse {
                                        status: 200,
                                        headers: HashMap::from([(
                                            "Content-Type".to_string(),
                                            "application/json".to_string(),
                                        )]),
                                    })
                                    .unwrap(),
                                )
                                .payload(Payload {
                                    mime: Some("application/json".to_string()),
                                    bytes: serde_json::to_string(&node)
                                        .unwrap()
                                        .as_bytes()
                                        .to_vec(),
                                })
                                .send()?;
                            continue;
                        }
                    }
                }
            }
            Response::new()
                .ipc(
                    serde_json::to_vec(&http::HttpResponse {
                        status: 404,
                        headers: HashMap::from([(
                            "Content-Type".to_string(),
                            "application/json".to_string(),
                        )]),
                    })
                    .unwrap(),
                )
                .send()?;
            continue;
        }

        let Ok(msg) = serde_json::from_slice::<AllActions>(&request.ipc) else {
            println!("qns_indexer: got invalid message");
            continue;
        };


        match msg {
            // Probably more message types later...maybe not...
            AllActions::EventSubscription(e) => {
                state.block = hex_to_u64(&e.block_number).unwrap();
                let nodeId = &e.topics[1];

                let name = if decode_hex(&e.topics[0].clone()) == NodeRegistered::SIGNATURE_HASH {
                    let decoded = NodeRegistered::decode_data(&decode_hex_to_vec(&e.data), true).unwrap();
                    match dnswire_decode(decoded.0.clone()) {
                        Ok(name) => { state.names.insert(nodeId.to_string(), name.clone()); }
                        Err(_) => { println!("qns_indexer: failed to decode name: {:?}", decoded.0); }
                    }
                    continue;
                } else if let Some(name) = state.names.get(nodeId) {
                    name.clone()
                } else {
                    println!("qns_indexer: failed to find name: {:?}", nodeId);
                    continue;
                };

                let node = state.nodes.entry(name.clone()).or_insert_with(QnsUpdate::default);

                if node.name == "" {
                    node.name = name.clone();
                }

                if node.node == "" {
                    node.node = nodeId.clone();
                }

                match decode_hex(&e.topics[0].clone()) {
                    NodeRegistered::SIGNATURE_HASH => {}
                    KeyUpdate::SIGNATURE_HASH => {
                        let decoded = KeyUpdate::decode_data(&decode_hex_to_vec(&e.data), true).unwrap();
                        node.public_key = format!("0x{}", hex::encode(decoded.0));
                    }
                    IpUpdate::SIGNATURE_HASH => {
                        let decoded = IpUpdate::decode_data(&decode_hex_to_vec(&e.data), true).unwrap();
                        let ip = decoded.0;
                        node.ip = format!(
                            "{}.{}.{}.{}",
                            (ip >> 24) & 0xFF,
                            (ip >> 16) & 0xFF,
                            (ip >> 8) & 0xFF,
                            ip & 0xFF
                        );
                    }
                    WsUpdate::SIGNATURE_HASH => {
                        let decoded = WsUpdate::decode_data(&decode_hex_to_vec(&e.data), true).unwrap();
                        node.port = decoded.0;
                    }
                    WtUpdate::SIGNATURE_HASH => {
                        let decoded = WtUpdate::decode_data(&decode_hex_to_vec(&e.data), true).unwrap();
                    }
                    TcpUpdate::SIGNATURE_HASH => {
                        let decoded = TcpUpdate::decode_data(&decode_hex_to_vec(&e.data), true).unwrap();
                    }
                    UdpUpdate::SIGNATURE_HASH => { 
                        let decoded = UdpUpdate::decode_data(&decode_hex_to_vec(&e.data), true).unwrap();
                    }
                    RoutingUpdate::SIGNATURE_HASH => {
                        let decoded = RoutingUpdate::decode_data(&decode_hex_to_vec(&e.data), true).unwrap();
                        let routers_raw = decoded.0;
                        node.routers = routers_raw    
                            .iter()
                            .map(|r| {
                                let key = hex::encode(r);
                                match state.names.get(&key) {
                                    Some(name) => name.clone(),
                                    None => format!("proposed router did not exist: 0x{}", key),
                                }
                            })
                            .collect::<Vec<String>>();

                    }
                    event => {
                        println!("qns_indexer: got unknown event: {:?}", event);
                    }
                }

                if node.public_key != "0x" && (( node.ip != "" && node.port != 0) || node.routers.len() > 0) {

                    Request::new()
                        .target((&our.node, "net", "sys", "uqbar"))
                        .try_ipc(NetActions::QnsUpdate(node.clone()))?
                        .send()?;

                }

            }
        }
        set_state(&bincode::serialize(&state)?);
    }
}
// helpers
// TODO these probably exist somewhere in alloy...not sure where though.
fn decode_hex(s: &str) -> FixedBytes<32> {
    // If the string starts with "0x", skip the prefix
    let hex_part = if s.starts_with("0x") { &s[2..] } else { s };

    let mut arr = [0_u8; 32];
    arr.copy_from_slice(&hex::decode(hex_part).unwrap()[0..32]);
    FixedBytes(arr)
}

fn decode_hex_to_vec(s: &str) -> Vec<u8> {
    // If the string starts with "0x", skip the prefix
    let hex_part = if s.starts_with("0x") { &s[2..] } else { s };

    hex::decode(hex_part).unwrap()
}

fn hex_to_u64(hex: &str) -> Result<u64, std::num::ParseIntError> {
    let without_prefix = if hex.starts_with("0x") {
        &hex[2..]
    } else {
        hex
    };
    u64::from_str_radix(without_prefix, 16)
}

fn dnswire_decode(wire_format_bytes: Vec<u8>) -> Result<String, FromUtf8Error> {
    let mut i = 0;
    let mut result = Vec::new();

    while i < wire_format_bytes.len() {
        let len = wire_format_bytes[i] as usize;
        if len == 0 {
            break;
        }
        let end = i + len + 1;
        let mut span = wire_format_bytes[i + 1..end].to_vec();
        span.push('.' as u8);
        result.push(span);
        i = end;
    }

    let flat: Vec<_> = result.into_iter().flatten().collect();

    let name = String::from_utf8(flat)?;

    // Remove the trailing '.' if it exists (it should always exist)
    if name.ends_with('.') {
        Ok(name[0..name.len() - 1].to_string())
    } else {
        Ok(name)
    }
}
