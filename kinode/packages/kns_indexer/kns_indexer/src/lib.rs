use crate::kinode::process::kns_indexer::{
    GetStateRequest, IndexerRequests, NamehashToNameRequest, NodeInfoRequest,
};
use alloy_primitives::keccak256;
use alloy_sol_types::SolEvent;
use kinode_process_lib::{
    await_message, call_init, eth, kimap, net, print_to_terminal, println, Address, Message,
    Request, Response,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{hash_map::HashMap, BTreeMap},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};

wit_bindgen::generate!({
    path: "target/wit",
    world: "kns-indexer-sys-v0",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize],
});

#[cfg(not(feature = "simulation-mode"))]
const KIMAP_ADDRESS: &'static str = kimap::KIMAP_ADDRESS; // optimism
#[cfg(feature = "simulation-mode")]
const KIMAP_ADDRESS: &'static str = "0xEce71a05B36CA55B895427cD9a440eEF7Cf3669D"; // local

#[cfg(not(feature = "simulation-mode"))]
const CHAIN_ID: u64 = kimap::KIMAP_CHAIN_ID; // optimism
#[cfg(feature = "simulation-mode")]
const CHAIN_ID: u64 = 31337; // local

#[cfg(not(feature = "simulation-mode"))]
const KIMAP_FIRST_BLOCK: u64 = kimap::KIMAP_FIRST_BLOCK; // optimism
#[cfg(feature = "simulation-mode")]
const KIMAP_FIRST_BLOCK: u64 = 1; // local

#[derive(Clone, Debug, Serialize, Deserialize)]
struct State {
    chain_id: u64,
    // what contract this state pertains to
    contract_address: eth::Address,
    // namehash to human readable name
    names: HashMap<String, String>,
    // human readable name to most recent on-chain routing information as json
    // TODO: optional params knsUpdate? also include tba.
    nodes: HashMap<String, net::KnsUpdate>,
    // last block we have an update from
    last_block: u64,
}

// note: not defined in wit api right now like IndexerRequests.
#[derive(Clone, Debug, Serialize, Deserialize)]
enum IndexerResponses {
    Name(Option<String>),
    NodeInfo(Option<net::KnsUpdate>),
    GetState(State),
}

call_init!(init);
fn init(our: Address) {
    println!("indexing on contract address {KIMAP_ADDRESS}");

    // we **can** persist PKI state between boots but with current size, it's
    // more robust just to reload the whole thing. the new contracts will allow
    // us to quickly verify we have the updated mapping with root hash, but right
    // now it's tricky to recover from missed events.

    let state = State {
        chain_id: CHAIN_ID,
        contract_address: KIMAP_ADDRESS.parse::<eth::Address>().unwrap(),
        nodes: HashMap::new(),
        names: HashMap::new(),
        last_block: KIMAP_FIRST_BLOCK,
    };

    if let Err(e) = main(our, state) {
        println!("fatal error: {e}");
    }
}

fn main(our: Address, mut state: State) -> anyhow::Result<()> {
    #[cfg(feature = "simulation-mode")]
    add_temp_hardcoded_tlzs(&mut state);

    // sub_id: 1
    let mints_filter = eth::Filter::new()
        .address(state.contract_address)
        .to_block(eth::BlockNumberOrTag::Latest)
        .event("Mint(bytes32,bytes32,bytes,bytes)");

    let notes = vec![
        keccak256("~ws-port"),
        keccak256("~tcp-port"),
        keccak256("~net-key"),
        keccak256("~routers"),
        keccak256("~ip"),
    ];

    // sub_id: 2
    let notes_filter = eth::Filter::new()
        .address(state.contract_address)
        .to_block(eth::BlockNumberOrTag::Latest)
        .event("Note(bytes32,bytes32,bytes,bytes,bytes)")
        .topic3(notes);

    // 60s timeout -- these calls can take a long time
    // if they do time out, we try them again
    let eth_provider: eth::Provider = eth::Provider::new(state.chain_id, 60);

    print_to_terminal(
        1,
        &format!(
            "subscribing, state.block: {}, chain_id: {}",
            state.last_block - 1,
            state.chain_id
        ),
    );

    // subscribe to logs first, so no logs are missed
    println!("subscribing to new logs...");
    eth_provider.subscribe_loop(1, mints_filter.clone());
    eth_provider.subscribe_loop(2, notes_filter.clone());
    listen_to_new_blocks(); // sub_id: 3

    // if block in state is < current_block, get logs from that part.
    println!("syncing old logs...");
    fetch_and_process_logs(&eth_provider, &our, &mut state, mints_filter.clone());
    fetch_and_process_logs(&eth_provider, &our, &mut state, notes_filter.clone());
    println!("done syncing old logs.");

    let mut pending_requests: BTreeMap<u64, Vec<IndexerRequests>> = BTreeMap::new();

    loop {
        let Ok(message) = await_message() else {
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
                &mints_filter,
                &notes_filter,
            )?;
        } else {
            let request = serde_json::from_slice(&body)?;

            match request {
                IndexerRequests::NamehashToName(NamehashToNameRequest {
                    ref hash,
                    ref block,
                }) => {
                    // make sure we've seen the whole block
                    if *block < state.last_block {
                        Response::new()
                            .body(serde_json::to_vec(&IndexerResponses::Name(
                                state.names.get(hash).cloned(),
                            ))?)
                            .send()?;
                    } else {
                        pending_requests
                            .entry(*block)
                            .or_insert(vec![])
                            .push(request);
                    }
                }
                IndexerRequests::NodeInfo(NodeInfoRequest { ref name, block }) => {
                    // make sure we've seen the whole block
                    if block < state.last_block {
                        Response::new()
                            .body(serde_json::to_vec(&IndexerResponses::NodeInfo(
                                state.nodes.get(name).cloned(),
                            ))?)
                            .send()?;
                    } else {
                        pending_requests
                            .entry(block)
                            .or_insert(vec![])
                            .push(request);
                    }
                }
                IndexerRequests::GetState(GetStateRequest { block }) => {
                    // make sure we've seen the whole block
                    if block < state.last_block {
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
    mints_filter: &eth::Filter,
    notes_filter: &eth::Filter,
) -> anyhow::Result<()> {
    match serde_json::from_slice::<eth::EthSubResult>(body) {
        Ok(Ok(eth::EthSub { result, .. })) => {
            if let eth::SubscriptionResult::Log(log) = result {
                if let Err(e) = handle_log(our, state, &log) {
                    // print errors at verbosity=1
                    print_to_terminal(1, &format!("log-handling error! {e:?}"));
                }
            } else if let eth::SubscriptionResult::Header(header) = result {
                if let Some(block) = header.number {
                    // risque..
                    state.last_block = block;
                }
            }
        }
        Ok(Err(e)) => {
            println!("got eth subscription error ({e:?}), resubscribing");
            if e.id == 1 {
                eth_provider.subscribe_loop(1, mints_filter.clone());
            } else if e.id == 2 {
                eth_provider.subscribe_loop(2, notes_filter.clone());
            } else if e.id == 3 {
                listen_to_new_blocks();
            }
        }
        Err(e) => {
            return Err(e.into());
        }
    }

    handle_pending_requests(state, pending_requests)?;

    // set_state(&bincode::serialize(state)?);
    Ok(())
}

fn handle_pending_requests(
    state: &mut State,
    pending_requests: &mut BTreeMap<u64, Vec<IndexerRequests>>,
) -> anyhow::Result<()> {
    // check the pending_requests btreemap to see if there are any requests that
    // can be handled now that the state block has been updated
    if pending_requests.is_empty() {
        return Ok(());
    }
    let mut blocks_to_remove = vec![];
    for (block, requests) in pending_requests.iter() {
        // make sure we've seen the whole block
        if *block < state.last_block {
            for request in requests.iter() {
                match request {
                    IndexerRequests::NamehashToName(NamehashToNameRequest { hash, .. }) => {
                        Response::new()
                            .body(serde_json::to_vec(&IndexerResponses::Name(
                                state.names.get(hash).cloned(),
                            ))?)
                            .send()
                            .unwrap();
                    }
                    IndexerRequests::NodeInfo(NodeInfoRequest { name, .. }) => {
                        Response::new()
                            .body(serde_json::to_vec(&IndexerResponses::NodeInfo(
                                state.nodes.get(name).cloned(),
                            ))?)
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
    Ok(())
}

fn handle_log(our: &Address, state: &mut State, log: &eth::Log) -> anyhow::Result<()> {
    let node_name = match log.topics()[0] {
        kimap::contract::Mint::SIGNATURE_HASH => {
            let decoded = kimap::contract::Mint::decode_log_data(log.data(), true).unwrap();
            let parent_hash = decoded.parenthash.to_string();
            let child_hash = decoded.childhash.to_string();
            let name = String::from_utf8(decoded.label.to_vec())?;

            if !kimap::valid_name(&name, false) {
                return Err(anyhow::anyhow!("skipping invalid entry"));
            }

            let full_name = match get_parent_name(&state.names, &parent_hash) {
                Some(parent_name) => format!("{name}.{parent_name}"),
                None => name,
            };

            state.names.insert(child_hash.clone(), full_name.clone());
            state.nodes.insert(
                full_name.clone(),
                net::KnsUpdate {
                    name: full_name.clone(),
                    public_key: String::new(),
                    ips: Vec::new(),
                    ports: BTreeMap::new(),
                    routers: Vec::new(),
                },
            );
            full_name
        }
        kimap::contract::Note::SIGNATURE_HASH => {
            let decoded = kimap::contract::Note::decode_log_data(log.data(), true).unwrap();

            let note = String::from_utf8(decoded.label.to_vec())?;
            let node_hash = decoded.parenthash.to_string();

            let Some(node_name) = get_parent_name(&state.names, &node_hash) else {
                return Err(anyhow::anyhow!("parent node for note not found"));
            };

            match note.as_str() {
                "~ws-port" => {
                    let ws = bytes_to_port(&decoded.data)?;
                    if let Some(node) = state.nodes.get_mut(&node_name) {
                        node.ports.insert("ws".to_string(), ws);
                        // port defined, -> direct
                        node.routers = vec![];
                    }
                }
                "~tcp-port" => {
                    let tcp = bytes_to_port(&decoded.data)?;
                    if let Some(node) = state.nodes.get_mut(&node_name) {
                        node.ports.insert("tcp".to_string(), tcp);
                        // port defined, -> direct
                        node.routers = vec![];
                    }
                }
                "~net-key" => {
                    if decoded.data.len() != 32 {
                        return Err(anyhow::anyhow!("invalid net-key length"));
                    }
                    if let Some(node) = state.nodes.get_mut(&node_name) {
                        node.public_key = decoded.data.to_string();
                    }
                }
                "~routers" => {
                    let routers = decode_routers(&decoded.data, &state);
                    if let Some(node) = state.nodes.get_mut(&node_name) {
                        node.routers = routers;
                        // -> indirect
                        node.ports = BTreeMap::new();
                        node.ips = vec![];
                    };
                }
                "~ip" => {
                    let ip = bytes_to_ip(&decoded.data)?;
                    if let Some(node) = state.nodes.get_mut(&node_name) {
                        node.ips = vec![ip.to_string()];
                        // -> direct
                        node.routers = vec![];
                    };
                }
                _other => {
                    // println!("unknown note: {other}");
                }
            }
            node_name
        }
        _log => {
            // println!("unknown log: {log:?}");
            return Ok(());
        }
    };

    if let Some(block) = log.block_number {
        state.last_block = block;
    }

    // only send an update if we have a *full* set of data for networking:
    // a node name, plus either <routers> or <ip, port(s)>

    if let Some(node_info) = state.nodes.get(&node_name) {
        if !node_info.public_key.is_empty()
            && ((!node_info.ips.is_empty() && !node_info.ports.is_empty())
                || node_info.routers.len() > 0)
        {
            Request::to((&our.node, "net", "distro", "sys"))
                .body(rmp_serde::to_vec(&net::NetAction::KnsUpdate(
                    node_info.clone(),
                ))?)
                .send()?;
        }
    }
    Ok(())
}

// helpers

fn fetch_and_process_logs(
    eth_provider: &eth::Provider,
    our: &Address,
    state: &mut State,
    filter: eth::Filter,
) {
    let filter = filter.from_block(KIMAP_FIRST_BLOCK);
    loop {
        match eth_provider.get_logs(&filter) {
            Ok(logs) => {
                for log in logs {
                    if let Err(e) = handle_log(our, state, &log) {
                        print_to_terminal(1, &format!("log-handling error! {e:?}"));
                    }
                }
                return;
            }
            Err(e) => {
                println!("got eth error while fetching logs: {e:?}, trying again in 5s...");
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        }
    }
}

fn get_parent_name(names: &HashMap<String, String>, parent_hash: &str) -> Option<String> {
    let mut current_hash = parent_hash;
    let mut components = Vec::new(); // Collect components in a vector
    let mut visited_hashes = std::collections::HashSet::new();

    while let Some(parent_name) = names.get(current_hash) {
        if !visited_hashes.insert(current_hash) {
            break;
        }

        if !parent_name.is_empty() {
            components.push(parent_name.clone());
        }

        // Update current_hash to the parent's hash for the next iteration
        if let Some(new_parent_hash) = names.get(parent_name) {
            current_hash = new_parent_hash;
        } else {
            break;
        }
    }

    if components.is_empty() {
        return None;
    }

    components.reverse();
    Some(components.join("."))
}

// TEMP. Either remove when event reimitting working with anvil,
// or refactor into better structure(!)
#[cfg(feature = "simulation-mode")]
fn add_temp_hardcoded_tlzs(state: &mut State) {
    // add some hardcoded top level zones
    state.names.insert(
        "0xdeeac81ae11b64e7cab86d089c306e5d223552a630f02633ce170d2786ff1bbd".to_string(),
        "os".to_string(),
    );
    state.names.insert(
        "0x137d9e4cc0479164d40577620cb3b41b083c6e8dbf58f8523be76d207d6fd8ea".to_string(),
        "dev".to_string(),
    );
}

/// Decodes bytes into an array of keccak256 hashes (32 bytes each) and returns their full names.
fn decode_routers(data: &[u8], state: &State) -> Vec<String> {
    if data.len() % 32 != 0 {
        print_to_terminal(
            1,
            &format!("got invalid data length for router hashes: {}", data.len()),
        );
        return vec![];
    }

    let mut routers = Vec::new();
    for chunk in data.chunks(32) {
        let hash_str = format!("0x{}", hex::encode(chunk));

        match state.names.get(&hash_str) {
            Some(full_name) => routers.push(full_name.clone()),
            None => print_to_terminal(
                1,
                &format!("error: no name found for router hash {hash_str}"),
            ),
        }
    }

    routers
}

pub fn bytes_to_ip(bytes: &[u8]) -> anyhow::Result<IpAddr> {
    match bytes.len() {
        4 => {
            // IPv4 address
            let ip_num = u32::from_be_bytes(bytes.try_into().unwrap());
            Ok(IpAddr::V4(Ipv4Addr::from(ip_num)))
        }
        16 => {
            // IPv6 address
            let ip_num = u128::from_be_bytes(bytes.try_into().unwrap());
            Ok(IpAddr::V6(Ipv6Addr::from(ip_num)))
        }
        _ => Err(anyhow::anyhow!("Invalid byte length for IP address")),
    }
}

pub fn bytes_to_port(bytes: &[u8]) -> anyhow::Result<u16> {
    match bytes.len() {
        2 => Ok(u16::from_be_bytes([bytes[0], bytes[1]])),
        _ => Err(anyhow::anyhow!("Invalid byte length for port")),
    }
}

fn listen_to_new_blocks() {
    let eth_newheads_sub = eth::EthAction::SubscribeLogs {
        sub_id: 3,
        chain_id: CHAIN_ID,
        kind: eth::SubscriptionKind::NewHeads,
        params: eth::Params::Bool(false),
    };

    Request::to(("our", "eth", "distro", "sys"))
        .body(serde_json::to_vec(&eth_newheads_sub).unwrap())
        .send()
        .unwrap();
}
