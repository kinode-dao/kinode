use alloy_sol_types::{sol, SolEvent};
use kinode_process_lib::{
    await_message, eth, get_typed_state, println, set_state, Address, Message, Request, Response,
};
use serde::{Deserialize, Serialize};
use std::collections::{
    hash_map::{Entry, HashMap},
    BTreeMap,
};
use std::string::FromUtf8Error;

wit_bindgen::generate!({
    path: "wit",
    world: "process",
    exports: {
        world: Component,
    },
});

#[derive(Clone, Debug, Serialize, Deserialize)]
struct State {
    chain_id: u64,
    // what contract this state pertains to
    contract_address: String,
    // namehash to human readable name
    names: HashMap<String, String>,
    // human readable name to most recent on-chain routing information as json
    // NOTE: not every namehash will have a node registered
    nodes: HashMap<String, KnsUpdate>,
    // last block we have an update from
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
    /// set block to 0 if you just want to get the current state of the indexer
    NodeInfo { name: String, block: u64 },
    /// return the entire state of the indexer at the given block
    /// set block to 0 if you just want to get the current state of the indexer
    GetState { block: u64 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NetAction {
    KnsUpdate(KnsUpdate),
    KnsBatchUpdate(Vec<KnsUpdate>),
}

impl TryInto<Vec<u8>> for NetAction {
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

fn subscribe_to_logs(eth_provider: &eth::Provider, from_block: u64, filter: eth::Filter) {
    loop {
        match eth_provider.subscribe(1, filter.clone().from_block(from_block)) {
            Ok(()) => break,
            Err(_) => {
                println!("failed to subscribe to chain! trying again in 5s...");
                std::thread::sleep(std::time::Duration::from_secs(5));
                continue;
            }
        }
    }
    println!("subscribed to logs successfully");
}

struct Component;
impl Guest for Component {
    fn init(our: String) {
        let our: Address = our.parse().unwrap();

        // first, await a message from the kernel which will contain the
        // chain ID and contract address for the KNS version we want to track.
        let chain_id: u64;
        let contract_address: String;
        loop {
            let Ok(Message::Request { source, body, .. }) = await_message() else {
                continue;
            };
            if source.process != "kernel:distro:sys" {
                continue;
            }
            (chain_id, contract_address) = serde_json::from_slice(&body).unwrap();
            break;
        }
        println!("indexing on contract address {}", contract_address);

        // if we have state, load it in
        let state: State = match get_typed_state(|bytes| Ok(bincode::deserialize::<State>(bytes)?))
        {
            Some(s) => {
                // if chain id or contract address changed from a previous run, reset state
                if s.chain_id != chain_id || s.contract_address != contract_address {
                    println!(
                        "resetting state because runtime contract address or chain ID changed"
                    );
                    State {
                        chain_id,
                        contract_address,
                        names: HashMap::new(),
                        nodes: HashMap::new(),
                        block: 1,
                    }
                } else {
                    println!("loading in {} persisted PKI entries", s.nodes.len());
                    s
                }
            }
            None => State {
                chain_id,
                contract_address: contract_address.clone(),
                names: HashMap::new(),
                nodes: HashMap::new(),
                block: 1,
            },
        };

        match main(our, state) {
            Ok(_) => {}
            Err(e) => {
                println!("error: {:?}", e);
            }
        }
    }
}

fn main(our: Address, mut state: State) -> anyhow::Result<()> {
    // shove all state into net::net
    Request::new()
        .target((&our.node, "net", "distro", "sys"))
        .try_body(NetAction::KnsBatchUpdate(
            state.nodes.values().cloned().collect::<Vec<_>>(),
        ))?
        .send()?;

    let filter = eth::Filter::new()
        .address(state.contract_address.parse::<eth::Address>().unwrap())
        .from_block(state.block - 1)
        .to_block(eth::BlockNumberOrTag::Latest)
        .events(vec![
            "NodeRegistered(bytes32,bytes)",
            "KeyUpdate(bytes32,bytes32)",
            "IpUpdate(bytes32,uint128)",
            "WsUpdate(bytes32,uint16)",
            "RoutingUpdate(bytes32,bytes32[])",
        ]);

    // 60s timeout -- these calls can take a long time
    // if they do time out, we try them again
    let eth_provider = eth::Provider::new(state.chain_id, 60);

    // if block in state is < current_block, get logs from that part.
    if state.block < eth_provider.get_block_number().unwrap_or(u64::MAX) {
        loop {
            match eth_provider.get_logs(&filter) {
                Ok(logs) => {
                    for log in logs {
                        handle_log(&our, &mut state, &log)?;
                    }
                    break;
                }
                Err(_) => {
                    println!("failed to fetch logs! trying again in 5s...");
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    continue;
                }
            }
        }
    }
    // shove all state into net::net
    Request::new()
        .target((&our.node, "net", "distro", "sys"))
        .try_body(NetAction::KnsBatchUpdate(
            state.nodes.values().cloned().collect::<Vec<_>>(),
        ))?
        .send()?;

    set_state(&bincode::serialize(&state)?);

    subscribe_to_logs(&eth_provider, state.block - 1, filter.clone());

    let mut pending_requests: BTreeMap<u64, Vec<IndexerRequests>> = BTreeMap::new();

    loop {
        let Ok(message) = await_message() else {
            println!("got network error");
            continue;
        };
        let Message::Request { source, body, .. } = message else {
            // TODO we could store the subscription ID for eth
            // in case we want to cancel/reset it
            continue;
        };

        if source.process == "eth:distro:sys" {
            handle_eth_message(
                &our,
                &mut state,
                &eth_provider,
                &mut pending_requests,
                &body,
                &filter,
            )?;
        } else {
            let Ok(request) = serde_json::from_slice::<IndexerRequests>(&body) else {
                println!("got invalid message");
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
                IndexerRequests::GetState { block } => {
                    if block <= state.block {
                        Response::new().body(serde_json::to_vec(&state)?).send()?;
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
    eth_provider: &eth::Provider,
    pending_requests: &mut BTreeMap<u64, Vec<IndexerRequests>>,
    body: &[u8],
    filter: &eth::Filter,
) -> anyhow::Result<()> {
    let Ok(eth_result) = serde_json::from_slice::<eth::EthSubResult>(body) else {
        return Err(anyhow::anyhow!("got invalid message"));
    };

    match eth_result {
        Ok(eth::EthSub { result, .. }) => {
            if let eth::SubscriptionResult::Log(log) = result {
                handle_log(our, state, &log)?;
            }
        }
        Err(_e) => {
            println!("got eth subscription error");
            subscribe_to_logs(&eth_provider, state.block - 1, filter.clone());
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
                    IndexerRequests::GetState { .. } => {
                        Response::new()
                            .body(serde_json::to_vec(&state)?)
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

fn handle_log(our: &Address, state: &mut State, log: &eth::Log) -> anyhow::Result<()> {
    let node_id = log.topics[1];

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
        Request::new()
            .target((&our.node, "net", "distro", "sys"))
            .try_body(NetAction::KnsUpdate(node.clone()))?
            .send()?;
    }

    // if new block is > 100 from last block, save state
    let block = log.block_number.expect("expect").to::<u64>();
    if block > state.block + 100 {
        kinode_process_lib::print_to_terminal(
            1,
            &format!(
                "persisting {} PKI entries at block {}",
                state.nodes.len(),
                block
            ),
        );
        state.block = block;
        set_state(&bincode::serialize(state)?);
    }
    Ok(())
}

fn get_name(log: &eth::Log) -> String {
    let decoded = NodeRegistered::abi_decode_data(&log.data, true).unwrap();
    let name = match dnswire_decode(decoded.0.clone()) {
        Ok(n) => n,
        Err(_) => {
            println!("failed to decode name: {:?}", decoded.0);
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
