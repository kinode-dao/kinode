use alloy_rpc_types::Log;
use alloy_sol_types::{sol, SolEvent};
use kinode_process_lib::eth::{EthAddress, EthSubEvent, SubscribeLogsRequest};
use kinode_process_lib::{
    await_message, get_typed_state, print_to_terminal, println, set_state, Address, Message,
    Request, Response,
};
use serde::{Deserialize, Serialize};
use std::collections::{
    hash_map::{Entry, HashMap},
    BTreeMap,
};
use std::str::FromStr;
use std::string::FromUtf8Error;

wit_bindgen::generate!({
    path: "../../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

#[derive(Clone, Debug, Serialize, Deserialize)]
struct State {
    // what contract this state pertains to
    contract_address: Option<String>,
    // namehash to human readable name
    names: HashMap<String, String>,
    // human readable name to most recent on-chain routing information as json
    // NOTE: not every namehash will have a node registered
    nodes: HashMap<String, KnsUpdate>,
    // last block we read from
    block: u64,
}

/// IndexerRequests are used to query discrete information from the indexer
/// for example, if you want to know the human readable name for a namehash,
/// you would send a NamehashToName request.
/// If you want to know the most recent on-chain routing information for a
/// human readable name, you would send a NodeInfo request.
/// The block parameter specifies the recency of the data: the indexer will
/// not respond until it has processed events up to the specified block.
#[derive(Debug, Serialize, Deserialize)]
pub enum IndexerRequests {
    /// return the human readable name for a namehash
    /// returns an Option<String>
    NamehashToName { hash: String, block: u64 },
    /// return the most recent on-chain routing information for a node name.
    /// returns an Option<KnsUpdate>
    NodeInfo { name: String, block: u64 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NetActions {
    KnsUpdate(KnsUpdate),
    KnsBatchUpdate(Vec<KnsUpdate>),
}

impl TryInto<Vec<u8>> for NetActions {
    type Error = anyhow::Error;
    fn try_into(self) -> Result<Vec<u8>, Self::Error> {
        Ok(rmp_serde::to_vec(&self)?)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct KnsUpdate {
    pub name: String, // actual username / domain name
    pub owner: String,
    pub node: String, // hex namehash of node
    pub public_key: String,
    pub ip: String,
    pub port: u16,
    pub routers: Vec<String>,
}

impl KnsUpdate {
    pub fn new(name: &String, node: &String) -> Self {
        Self {
            name: name.clone(),
            node: node.clone(),
            ..Default::default()
        }
    }
}

sol! {
    // Logged whenever a KNS node is created
    event NodeRegistered(bytes32 indexed node, bytes name);
    event KeyUpdate(bytes32 indexed node, bytes32 key);
    event IpUpdate(bytes32 indexed node, uint128 ip);
    event WsUpdate(bytes32 indexed node, uint16 port);
    event WtUpdate(bytes32 indexed node, uint16 port);
    event TcpUpdate(bytes32 indexed node, uint16 port);
    event UdpUpdate(bytes32 indexed node, uint16 port);
    event RoutingUpdate(bytes32 indexed node, bytes32[] routers);
}

struct Component;
impl Guest for Component {
    fn init(our: String) {
        let our: Address = our.parse().unwrap();

        let mut state: State = State {
            contract_address: None,
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
                println!("kns_indexer: error: {:?}", e);
            }
        }
    }
}

fn main(our: Address, mut state: State) -> anyhow::Result<()> {
    // first, await a message from the kernel which will contain the
    // contract address for the KNS version we want to track.
    let mut contract_address: Option<String> = None;
    loop {
        let Ok(Message::Request { source, body, .. }) = await_message() else {
            continue;
        };
        if source.process != "kernel:distro:sys" {
            continue;
        }
        contract_address = Some(std::str::from_utf8(&body).unwrap().to_string());
        break;
    }
    println!(
        "kns_indexer: indexing on contract address {}",
        contract_address.as_ref().unwrap()
    );
    // if contract address changed from a previous run, reset state
    if state.contract_address != contract_address {
        state = State {
            contract_address: contract_address.clone(),
            names: HashMap::new(),
            nodes: HashMap::new(),
            block: 1,
        };
    }
    // shove all state into net::net
    Request::new()
        .target((&our.node, "net", "distro", "sys"))
        .try_body(NetActions::KnsBatchUpdate(
            state.nodes.values().cloned().collect::<Vec<_>>(),
        ))?
        .send()?;

    SubscribeLogsRequest::new(1) // subscription id 1
        .address(EthAddress::from_str(contract_address.unwrap().as_str())?)
        .from_block(state.block - 1)
        .events(vec![
            "NodeRegistered(bytes32,bytes)",
            "KeyUpdate(bytes32,bytes32)",
            "IpUpdate(bytes32,uint128)",
            "WsUpdate(bytes32,uint16)",
            "RoutingUpdate(bytes32,bytes32[])",
        ])
        .send()?;

    let mut pending_requests: BTreeMap<u64, Vec<IndexerRequests>> = BTreeMap::new();

    loop {
        let Ok(message) = await_message() else {
            println!("kns_indexer: got network error");
            continue;
        };
        let Message::Request { source, body, .. } = message else {
            // TODO we could store the subscription ID for eth
            // in case we want to cancel/reset it
            continue;
        };

        if source.process == "eth:distro:sys" {
            handle_eth_message(&our, &mut state, &mut pending_requests, &body)?;
        } else {
            let Ok(request) = serde_json::from_slice::<IndexerRequests>(&body) else {
                println!("kns_indexer: got invalid message");
                continue;
            };

            match request {
                IndexerRequests::NamehashToName { ref hash, block } => {
                    if block <= state.block {
                        Response::new()
                            .body(serde_json::to_vec(&state.names.get(hash))?)
                            .send()?;
                    } else {
                        pending_requests
                            .entry(block)
                            .or_insert(vec![])
                            .push(request);
                    }
                }
                IndexerRequests::NodeInfo { ref name, block } => {
                    if block <= state.block {
                        Response::new()
                            .body(serde_json::to_vec(&state.nodes.get(name))?)
                            .send()?;
                    } else {
                        pending_requests
                            .entry(block)
                            .or_insert(vec![])
                            .push(request);
                    }
                }
            }
        }
    }
}

fn handle_eth_message(
    our: &Address,
    state: &mut State,
    pending_requests: &mut BTreeMap<u64, Vec<IndexerRequests>>,
    body: &[u8],
) -> anyhow::Result<()> {
    let Ok(msg) = serde_json::from_slice::<EthSubEvent>(body) else {
        return Err(anyhow::anyhow!("kns_indexer: got invalid message"));
    };

    match msg {
        EthSubEvent::Log(log) => {
            state.block = log.block_number.expect("expect").to::<u64>();

            let node_id: alloy_primitives::FixedBytes<32> = log.topics[1];

            let name = match state.names.entry(node_id.to_string()) {
                Entry::Occupied(o) => o.into_mut(),
                Entry::Vacant(v) => v.insert(get_name(&log)),
            };

            let node = state
                .nodes
                .entry(name.to_string())
                .or_insert_with(|| KnsUpdate::new(name, &node_id.to_string()));

            let mut send = true;

            match log.topics[0] {
                KeyUpdate::SIGNATURE_HASH => {
                    node.public_key = KeyUpdate::abi_decode_data(&log.data, true)
                        .unwrap()
                        .0
                        .to_string();
                }
                IpUpdate::SIGNATURE_HASH => {
                    let ip = IpUpdate::abi_decode_data(&log.data, true).unwrap().0;
                    node.ip = format!(
                        "{}.{}.{}.{}",
                        (ip >> 24) & 0xFF,
                        (ip >> 16) & 0xFF,
                        (ip >> 8) & 0xFF,
                        ip & 0xFF
                    );
                    // when we get ip data, we should delete any router data,
                    // since the assignment of ip indicates an direct node
                    node.routers = vec![];
                }
                WsUpdate::SIGNATURE_HASH => {
                    node.port = WsUpdate::abi_decode_data(&log.data, true).unwrap().0;
                    // when we get port data, we should delete any router data,
                    // since the assignment of port indicates an direct node
                    node.routers = vec![];
                }
                RoutingUpdate::SIGNATURE_HASH => {
                    node.routers = RoutingUpdate::abi_decode_data(&log.data, true)
                        .unwrap()
                        .0
                        .iter()
                        .map(|r| r.to_string())
                        .collect::<Vec<String>>();
                    // when we get routing data, we should delete any ws/ip data,
                    // since the assignment of routers indicates an indirect node
                    node.ip = "".to_string();
                    node.port = 0;
                }
                _ => {
                    send = false;
                }
            }

            if node.public_key != ""
                && ((node.ip != "" && node.port != 0) || node.routers.len() > 0)
                && send
            {
                print_to_terminal(
                    1,
                    &format!(
                        "kns_indexer: sending ID to net: {node:?} (blocknum {})",
                        state.block
                    ),
                );
                Request::new()
                    .target((&our.node, "net", "distro", "sys"))
                    .try_body(NetActions::KnsUpdate(node.clone()))?
                    .send()?;
            }
        }
    }
    // check the pending_requests btreemap to see if there are any requests that
    // can be handled now that the state block has been updated
    let mut blocks_to_remove = vec![];
    for (block, requests) in pending_requests.iter() {
        if *block <= state.block {
            for request in requests.iter() {
                match request {
                    IndexerRequests::NamehashToName { hash, .. } => {
                        Response::new()
                            .body(serde_json::to_vec(&state.names.get(hash))?)
                            .send()
                            .unwrap();
                    }
                    IndexerRequests::NodeInfo { name, .. } => {
                        Response::new()
                            .body(serde_json::to_vec(&state.nodes.get(name))?)
                            .send()
                            .unwrap();
                    }
                }
            }
            blocks_to_remove.push(*block);
        } else {
            break;
        }
    }
    for block in blocks_to_remove.iter() {
        pending_requests.remove(block);
    }

    set_state(&bincode::serialize(state)?);
    Ok(())
}

fn get_name(log: &Log) -> String {
    let decoded = NodeRegistered::abi_decode_data(&log.data, true).unwrap();
    let name = match dnswire_decode(decoded.0.clone()) {
        Ok(n) => n,
        Err(_) => {
            println!("kns_indexer: failed to decode name: {:?}", decoded.0);
            panic!("")
        }
    };
    name
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
