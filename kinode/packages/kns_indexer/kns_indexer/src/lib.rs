use crate::kinode::process::kns_indexer::{
    GetStateRequest, IndexerRequests, NamehashToNameRequest, NodeInfoRequest,
};
use alloy_sol_types::{sol, SolEvent};
use kinode_process_lib::{
    await_message, call_init, eth, net, println, Address, Message, Request, Response,
};
use serde::{Deserialize, Serialize};
use std::collections::{
    hash_map::{Entry, HashMap},
    BTreeMap,
};

wit_bindgen::generate!({
    path: "target/wit",
    world: "kns-indexer-sys-v0",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize],
});

#[cfg(not(feature = "simulation-mode"))]
const KNS_ADDRESS: &'static str = "0xca5b5811c0c40aab3295f932b1b5112eb7bb4bd6"; // optimism
#[cfg(feature = "simulation-mode")]
const KNS_ADDRESS: &'static str = "0x5FbDB2315678afecb367f032d93F642f64180aa3"; // local

#[cfg(not(feature = "simulation-mode"))]
const CHAIN_ID: u64 = 10; // optimism
#[cfg(feature = "simulation-mode")]
const CHAIN_ID: u64 = 31337; // local

#[cfg(not(feature = "simulation-mode"))]
const KNS_FIRST_BLOCK: u64 = 114_923_786; // optimism
#[cfg(feature = "simulation-mode")]
const KNS_FIRST_BLOCK: u64 = 1; // local

#[derive(Clone, Debug, Serialize, Deserialize)]
struct State {
    chain_id: u64,
    // what contract this state pertains to
    contract_address: String,
    // namehash to human readable name
    names: HashMap<String, String>,
    // human readable name to most recent on-chain routing information as json
    // NOTE: not every namehash will have a node registered
    nodes: HashMap<String, net::KnsUpdate>,
    // last block we have an update from
    block: u64,
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

fn subscribe_to_logs(eth_provider: &eth::Provider, filter: eth::Filter) {
    loop {
        match eth_provider.subscribe(1, filter.clone()) {
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

call_init!(init);
fn init(our: Address) {
    println!("indexing on contract address {}", KNS_ADDRESS);

    // we **can** persist PKI state between boots but with current size, it's
    // more robust just to reload the whole thing. the new contracts will allow
    // us to quickly verify we have the updated mapping with root hash, but right
    // now it's tricky to recover from missed events.
    //
    // let state: State = match get_typed_state(|bytes| Ok(bincode::deserialize::<State>(bytes)?)) {
    //     Some(s) => {
    //         // if chain id or contract address changed from a previous run, reset state
    //         if s.chain_id != CHAIN_ID || s.contract_address != KNS_ADDRESS {
    //             println!("resetting state because runtime contract address or chain ID changed");
    //             State {
    //                 chain_id: CHAIN_ID,
    //                 contract_address: KNS_ADDRESS.to_string(),
    //                 names: HashMap::new(),
    //                 nodes: HashMap::new(),
    //                 block: KNS_FIRST_BLOCK,
    //             }
    //         } else {
    //             println!("loading in {} persisted PKI entries", s.nodes.len());
    //             s
    //         }
    //     }
    //     None => State {
    //         chain_id: CHAIN_ID,
    //         contract_address: KNS_ADDRESS.to_string(),
    //         names: HashMap::new(),
    //         nodes: HashMap::new(),
    //         block: KNS_FIRST_BLOCK,
    //     },
    // };
    let state = State {
        chain_id: CHAIN_ID,
        contract_address: KNS_ADDRESS.to_string(),
        names: HashMap::new(),
        nodes: HashMap::new(),
        block: KNS_FIRST_BLOCK,
    };

    match main(our, state) {
        Ok(_) => {}
        Err(e) => {
            println!("error: {:?}", e);
        }
    }
}

fn main(our: Address, mut state: State) -> anyhow::Result<()> {
    let filter = eth::Filter::new()
        .address(state.contract_address.parse::<eth::Address>().unwrap())
        .to_block(eth::BlockNumberOrTag::Latest)
        .events(vec![
            "NodeRegistered(bytes32,bytes)",
            "KeyUpdate(bytes32,bytes32)",
            "IpUpdate(bytes32,uint128)",
            "WsUpdate(bytes32,uint16)",
            "TcpUpdate(bytes32,uint16)",
            "RoutingUpdate(bytes32,bytes32[])",
        ]);

    // 60s timeout -- these calls can take a long time
    // if they do time out, we try them again
    let eth_provider = eth::Provider::new(state.chain_id, 60);

    println!(
        "subscribing, state.block: {}, chain_id: {}",
        state.block - 1,
        state.chain_id
    );

    subscribe_to_logs(&eth_provider, filter.clone());

    // if block in state is < current_block, get logs from that part.
    loop {
        match eth_provider.get_logs(&filter.clone().from_block(state.block - 1)) {
            Ok(logs) => {
                for log in logs {
                    match handle_log(&our, &mut state, &log) {
                        Ok(_) => {}
                        Err(e) => {
                            println!("log-handling error! {e:?}");
                        }
                    }
                }
                break;
            }
            Err(e) => {
                println!(
                    "got eth error while fetching logs: {:?}, trying again in 5s...",
                    e
                );
                std::thread::sleep(std::time::Duration::from_secs(5));
                continue;
            }
        }
    }

    // shove initial state into net::net
    // Request::new()
    //     .target((&our.node, "net", "distro", "sys"))
    //     .body(rmp_serde::to_vec(&net::NetAction::KnsBatchUpdate(
    //         state.nodes.values().cloned().collect::<Vec<_>>(),
    //     ))?)
    //     .send()?;

    // set_state(&bincode::serialize(&state)?);

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
                IndexerRequests::NamehashToName(NamehashToNameRequest { ref hash, block }) => {
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
                IndexerRequests::NodeInfo(NodeInfoRequest { ref name, block }) => {
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
                IndexerRequests::GetState(GetStateRequest { block }) => {
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
                match handle_log(our, state, &log) {
                    Ok(_) => {}
                    Err(e) => {
                        println!("log-handling error! {e:?}");
                    }
                }
            }
        }
        Err(_e) => {
            println!("got eth subscription error");
            subscribe_to_logs(&eth_provider, filter.clone());
        }
    }

    // check the pending_requests btreemap to see if there are any requests that
    // can be handled now that the state block has been updated
    let mut blocks_to_remove = vec![];
    for (block, requests) in pending_requests.iter() {
        if *block <= state.block {
            for request in requests.iter() {
                match request {
                    IndexerRequests::NamehashToName(NamehashToNameRequest { hash, .. }) => {
                        Response::new()
                            .body(serde_json::to_vec(&state.names.get(hash))?)
                            .send()
                            .unwrap();
                    }
                    IndexerRequests::NodeInfo(NodeInfoRequest { name, .. }) => {
                        Response::new()
                            .body(serde_json::to_vec(&state.nodes.get(name))?)
                            .send()
                            .unwrap();
                    }
                    IndexerRequests::GetState(GetStateRequest { .. }) => {
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

    // set_state(&bincode::serialize(state)?);
    Ok(())
}

fn handle_log(our: &Address, state: &mut State, log: &eth::Log) -> anyhow::Result<()> {
    let node_id = log.topics()[1];

    let name = match state.names.entry(node_id.to_string()) {
        Entry::Occupied(o) => o.into_mut(),
        Entry::Vacant(v) => v.insert(get_name(&log)?),
    };

    let node = state
        .nodes
        .entry(name.to_string())
        .or_insert_with(|| net::KnsUpdate {
            name: name.to_string(),
            owner: "".to_string(),
            node: node_id.to_string(),
            public_key: "".to_string(),
            ips: vec![],
            ports: BTreeMap::new(),
            routers: vec![],
        });

    let mut send = true;

    match log.topics()[0] {
        KeyUpdate::SIGNATURE_HASH => {
            node.public_key = KeyUpdate::decode_log_data(log.data(), true)
                .unwrap()
                .key
                .to_string();
        }
        IpUpdate::SIGNATURE_HASH => {
            let ip = IpUpdate::decode_log_data(log.data(), true).unwrap().ip;
            node.ips = vec![format!(
                "{}.{}.{}.{}",
                (ip >> 24) & 0xFF,
                (ip >> 16) & 0xFF,
                (ip >> 8) & 0xFF,
                ip & 0xFF
            )];
            // when we get ip data, we should delete any router data,
            // since the assignment of ip indicates an direct node
            node.routers = vec![];
        }
        WsUpdate::SIGNATURE_HASH
        | TcpUpdate::SIGNATURE_HASH
        | WtUpdate::SIGNATURE_HASH
        | UdpUpdate::SIGNATURE_HASH => {
            match log.topics()[0] {
                WsUpdate::SIGNATURE_HASH => node.ports.insert(
                    "ws".to_string(),
                    WsUpdate::decode_log_data(log.data(), true).unwrap().port,
                ),
                TcpUpdate::SIGNATURE_HASH => node.ports.insert(
                    "tcp".to_string(),
                    TcpUpdate::decode_log_data(log.data(), true).unwrap().port,
                ),
                WtUpdate::SIGNATURE_HASH => node.ports.insert(
                    "wt".to_string(),
                    WtUpdate::decode_log_data(log.data(), true).unwrap().port,
                ),
                UdpUpdate::SIGNATURE_HASH => node.ports.insert(
                    "udp".to_string(),
                    UdpUpdate::decode_log_data(log.data(), true).unwrap().port,
                ),
                _ => None,
            };
            // when we get port data, we should delete any router data,
            // since the assignment of port indicates an direct node
            node.routers = vec![];
        }
        RoutingUpdate::SIGNATURE_HASH => {
            node.routers = RoutingUpdate::decode_log_data(log.data(), true)
                .unwrap()
                .routers
                .iter()
                .map(|r| r.to_string())
                .collect::<Vec<String>>();
            // when we get routing data, we should delete any ws/ip data,
            // since the assignment of routers indicates an indirect node
            node.ips = vec![];
            node.ports.clear();
        }
        _ => {
            send = false;
        }
    }

    if node.public_key != ""
        && ((!node.ips.is_empty() && !node.ports.is_empty()) || node.routers.len() > 0)
        && send
    {
        Request::new()
            .target((&our.node, "net", "distro", "sys"))
            .body(rmp_serde::to_vec(&net::NetAction::KnsUpdate(node.clone()))?)
            .send()?;
    }

    // if new block is > 100 from last block, save state
    // let block = log.block_number.expect("expect");
    // if block > state.block + 100 {
    //     kinode_process_lib::print_to_terminal(
    //         1,
    //         &format!(
    //             "persisting {} PKI entries at block {}",
    //             state.nodes.len(),
    //             block
    //         ),
    //     );
    //     state.block = block;
    //     set_state(&bincode::serialize(state)?);
    // }
    Ok(())
}

fn get_name(log: &eth::Log) -> anyhow::Result<String> {
    let decoded = NodeRegistered::decode_log_data(log.data(), false).map_err(|_e| {
        anyhow::anyhow!(
            "got event other than NodeRegistered without knowing about existing node name"
        )
    })?;
    net::dnswire_decode(&decoded.name).map_err(|e| anyhow::anyhow!(e))
}
