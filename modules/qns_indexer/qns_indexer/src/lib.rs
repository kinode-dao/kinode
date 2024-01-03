use alloy_primitives::{B256, FixedBytes};
use alloy_rpc_types::Log;
use alloy_sol_types::{sol, SolEvent};
use hex;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::{Entry, HashMap};
use std::string::FromUtf8Error;
use std::str::FromStr;
use uqbar_process_lib::{
    await_message, get_typed_state, http, println, set_state, Address, Message, Payload,
    Request, Response, 
};

use uqbar_process_lib::eth::{
    EthAddress,
    SubscribeLogsRequest,
};

wit_bindgen::generate!({
    path: "../../../wit",
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
    EventSubscription(Log),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NetActions {
    QnsUpdate(QnsUpdate),
    QnsBatchUpdate(Vec<QnsUpdate>),
}

impl TryInto<Vec<u8>> for NetActions {
    type Error = anyhow::Error;
    fn try_into(self) -> Result<Vec<u8>, Self::Error> {
        Ok(rmp_serde::to_vec(&self)?)
    }
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

impl QnsUpdate {
    pub fn new(name: &String, node: &String) -> Self {
        Self {
            name: name.clone(),
            node: node.clone(),
            ..Default::default()
        }
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

        match main(our, state) {
            Ok(_) => {}
            Err(e) => {
                println!("qns_indexer: error: {:?}", e);
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

    SubscribeLogsRequest::new()
        .address(EthAddress::from_str("0x4C8D8d4A71cE21B4A16dAbf4593cDF30d79728F1")?)
        .from_block(state.block - 1)
        .events(vec![
            "NodeRegistered(bytes32,bytes)",
            "KeyUpdate(bytes32,bytes32)",
            "IpUpdate(bytes32,uint128)",
            "WsUpdate(bytes32,uint16)",
            "RoutingUpdate(bytes32,bytes32[])",
        ]).send()?;

    http::bind_http_path("/node/:name", false, false)?;

    loop {
        let Ok(message) = await_message() else {
            println!("qns_indexer: got network error");
            continue;
        };
        let Message::Request { source, ipc, .. } = message else {
            // TODO we should store the subscription ID for eth
            // incase we want to cancel/reset it
            continue;
        };

        if source.process == "http_server:sys:uqbar" {
            if let Ok(ipc_json) = serde_json::from_slice::<serde_json::Value>(&ipc) {
                if ipc_json["path"].as_str().unwrap_or_default() == "/node/:name" {
                    if let Some(name) = ipc_json["url_params"]["name"].as_str() {
                        if let Some(node) = state.nodes.get(name) {
                            Response::new()
                                .ipc(serde_json::to_vec(&http::HttpResponse {
                                    status: 200,
                                    headers: HashMap::from([(
                                        "Content-Type".to_string(),
                                        "application/json".to_string(),
                                    )]),
                                })?)
                                .payload(Payload {
                                    mime: Some("application/json".to_string()),
                                    bytes: serde_json::to_string(&node)?.as_bytes().to_vec(),
                                })
                                .send()?;
                            continue;
                        }
                    }
                }
            }
            Response::new()
                .ipc(serde_json::to_vec(&http::HttpResponse {
                    status: 404,
                    headers: HashMap::from([(
                        "Content-Type".to_string(),
                        "application/json".to_string(),
                    )]),
                })?)
                .send()?;
            continue;
        }

        let Ok(msg) = serde_json::from_slice::<AllActions>(&ipc) else {
            println!("qns_indexer: got invalid message");
            continue;
        };

        match msg {
            AllActions::EventSubscription(e) => {

                state.block = e.clone().block_number.expect("expect").to::<u64>();

                let node_id: alloy_primitives::FixedBytes<32> = e.topics[1];

                let name = match state.names.entry(node_id.clone().to_string()) {
                    Entry::Occupied(o) => o.into_mut(),
                    Entry::Vacant(v) => v.insert(get_name(&e, &node_id))
                };

                let mut node = state.nodes
                    .entry(name.to_string())
                    .or_insert_with(|| QnsUpdate::new(name, &node_id.to_string()));

                let mut send = true;

                match e.topics[0].clone() {
                    KeyUpdate::SIGNATURE_HASH => {
                        node.public_key = KeyUpdate::abi_decode_data(&e.data, true)
                            .unwrap().0.to_string();
                    }
                    IpUpdate::SIGNATURE_HASH => {
                        let ip = IpUpdate::abi_decode_data(&e.data, true).unwrap().0;
                        node.ip = format!("{}.{}.{}.{}",
                            (ip >> 24) & 0xFF,
                            (ip >> 16) & 0xFF,
                            (ip >> 8) & 0xFF,
                            ip & 0xFF
                        );
                    }
                    WsUpdate::SIGNATURE_HASH => {
                        node.port = WsUpdate::abi_decode_data(&e.data, true).unwrap().0;
                    }
                    RoutingUpdate::SIGNATURE_HASH => {
                        node.routers = RoutingUpdate::abi_decode_data(&e.data, true).unwrap().0
                            .iter()
                            .map(|r| r.to_string())
                            .collect::<Vec<String>>();
                    }
                    _ => { send = false; }
                }

                if send {
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

fn get_name(log: &Log, node_id: &B256) -> String {

    let decoded = NodeRegistered::abi_decode_data(&log.data, true).unwrap();
    let name = match dnswire_decode(decoded.0.clone()) {
        Ok(n) => { n }
        Err(_) => {
            println!("qns_indexer: failed to decode name: {:?}", decoded.0);
            panic!("")
        }
    };
    name

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

