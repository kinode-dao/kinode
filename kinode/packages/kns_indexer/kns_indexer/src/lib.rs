use crate::kinode::process::kns_indexer::{
    GetStateRequest, IndexerRequests, NamehashToNameRequest, NodeInfoRequest,
};
use alloy_primitives::keccak256;
use alloy_sol_types::{sol, SolEvent};
use kinode_process_lib::{
    await_message, call_init, eth, net, print_to_terminal, println, Address, Message, Request,
    Response,
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
const KIMAP_ADDRESS: &'static str = "0x7290Aa297818d0b9660B2871Bb87f85a3f9B4559"; // optimism
#[cfg(feature = "simulation-mode")]
const KIMAP_ADDRESS: &'static str = "0x0165878A594ca255338adfa4d48449f69242Eb8F"; // local

#[cfg(not(feature = "simulation-mode"))]
const CHAIN_ID: u64 = 10; // optimism
#[cfg(feature = "simulation-mode")]
const CHAIN_ID: u64 = 31337; // local

#[cfg(not(feature = "simulation-mode"))]
const KIMAP_FIRST_BLOCK: u64 = 114_923_786; // optimism
#[cfg(feature = "simulation-mode")]
const KIMAP_FIRST_BLOCK: u64 = 1; // local

#[derive(Clone, Debug, Serialize, Deserialize)]
struct State {
    chain_id: u64,
    // what contract this state pertains to
    contract_address: String,
    // namehash to human readable name
    names: HashMap<String, String>,
    // human readable name to most recent on-chain routing information as json
    // TODO: optional params knsUpdate? also include tba.
    nodes: HashMap<String, net::KnsUpdate>,
    // last block we have an update from
    block: u64,
}

// note: not defined in wit api right now like IndexerRequests.
#[derive(Clone, Debug, Serialize, Deserialize)]
enum IndexerResponses {
    Name(Option<String>),
    NodeInfo(Option<net::KnsUpdate>),
    GetState(State),
}

sol! {
    event Mint(bytes32 indexed parenthash, bytes32 indexed childhash,bytes indexed labelhash, bytes name);
    event Note(bytes32 indexed nodehash, bytes32 indexed notehash, bytes indexed labelhash, bytes note, bytes data);

    function get (
        bytes32 node
    ) external view returns (
        address tokenBoundAccount,
        address tokenOwner,
        bytes memory note
    );
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
        contract_address: KIMAP_ADDRESS.to_string(),
        nodes: HashMap::new(),
        names: HashMap::new(),
        block: KIMAP_FIRST_BLOCK,
    };

    if let Err(e) = main(our, state) {
        println!("fatal error: {e}");
    }
}

fn main(our: Address, mut state: State) -> anyhow::Result<()> {
    #[cfg(feature = "simulation-mode")]
    add_temp_hardcoded_tlzs(&mut state);

    let notes = vec![
        keccak256("~net-key"),
        keccak256("~ws-port"),
        keccak256("~routers"),
        keccak256("~tcp-port"),
        keccak256("~ip"),
    ];

    // sub_id: 1
    let mints_filter = eth::Filter::new()
        .address(state.contract_address.parse::<eth::Address>().unwrap())
        .to_block(eth::BlockNumberOrTag::Latest)
        .event("Mint(bytes32,bytes32,bytes,bytes)");

    // sub_id: 2
    let notes_filter = eth::Filter::new()
        .address(state.contract_address.parse::<eth::Address>().unwrap())
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
            state.block - 1,
            state.chain_id
        ),
    );

    subscribe_to_logs(&eth_provider, mints_filter.clone(), 1);
    subscribe_to_logs(&eth_provider, notes_filter.clone(), 2);
    println!("subscribed to logs successfully");

    // if block in state is < current_block, get logs from that part.
    fetch_and_process_logs(&eth_provider, &our, &mut state, mints_filter.clone());
    fetch_and_process_logs(&eth_provider, &our, &mut state, notes_filter.clone());

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
                    ref block,
                    ref hash,
                }) => {
                    if *block <= state.block {
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
                    if block <= state.block {
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
                    if block <= state.block {
                        Response::new()
                            .body(serde_json::to_vec(&state.clone())?)
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
    eth_provider: &eth::Provider,
    pending_requests: &mut BTreeMap<u64, Vec<IndexerRequests>>,
    body: &[u8],
    mints_filter: &eth::Filter,
    notes_filter: &eth::Filter,
) -> anyhow::Result<()> {
    let Ok(eth_result) = serde_json::from_slice::<eth::EthSubResult>(body) else {
        return Err(anyhow::anyhow!("got invalid message"));
    };

    match eth_result {
        Ok(eth::EthSub { result, .. }) => {
            if let eth::SubscriptionResult::Log(log) = result {
                if let Err(e) = handle_log(our, state, &log) {
                    // print errors at verbosity=1
                    print_to_terminal(1, &format!("log-handling error! {e:?}"));
                }
            }
        }
        Err(e) => {
            println!("got eth subscription error ({e:?}), resubscribing");
            if e.id == 1 {
                subscribe_to_logs(&eth_provider, mints_filter.clone(), 1);
            } else if e.id == 2 {
                subscribe_to_logs(&eth_provider, notes_filter.clone(), 2);
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
    let mut node: Option<String> = None;
    match log.topics()[0] {
        Mint::SIGNATURE_HASH => {
            let decoded = Mint::decode_log_data(log.data(), true).unwrap();
            let parent_hash = decoded.parenthash.to_string();
            let child_hash = decoded.childhash.to_string();
            let label = String::from_utf8(decoded.name.to_vec())?;

            let name = get_full_name(state, &label, &parent_hash);

            state.names.insert(child_hash.clone(), name.clone());
            // println!("got mint, name: {name}, child_hash: {child_hash}, tba: {tba}",);
            state
                .nodes
                .entry(name.clone())
                .or_insert_with(|| net::KnsUpdate {
                    name: name.clone(),
                    public_key: String::new(),
                    ips: Vec::new(),
                    ports: BTreeMap::new(),
                    routers: Vec::new(),
                });

            node = Some(name);
        }
        Note::SIGNATURE_HASH => {
            let decoded = Note::decode_log_data(log.data(), true).unwrap();

            let note = String::from_utf8(decoded.note.to_vec())?;
            let _note_hash: String = decoded.notehash.to_string();
            let node_hash = decoded.nodehash.to_string();

            let name = get_node_name(state, &node_hash);

            // println!("got note, from name: {name}, note: {note}, note_hash: {node_hash}",);
            match note.as_str() {
                "~ws-port" => {
                    let ws = bytes_to_port(&decoded.data)?;
                    state.nodes.entry(name.clone()).and_modify(|node| {
                        node.ports.insert("ws".to_string(), ws);
                        // port defined, -> direct
                        node.routers = vec![];
                    });
                    node = Some(name.clone());
                }
                "~tcp-port" => {
                    let tcp = bytes_to_port(&decoded.data)?;
                    state.nodes.entry(name.clone()).and_modify(|node| {
                        node.ports.insert("tcp".to_string(), tcp);
                        // port defined, -> direct
                        node.routers = vec![];
                    });
                    node = Some(name.clone());
                }
                "~net-key" => {
                    if decoded.data.len() != 32 {
                        return Err(anyhow::anyhow!("invalid net-key length"));
                    }
                    state.nodes.entry(name.clone()).and_modify(|node| {
                        node.public_key = decoded.data.to_string();
                    });
                    node = Some(name);
                }
                "~routers" => {
                    let routers = decode_routers(&decoded.data)?;
                    state.nodes.entry(name.clone()).and_modify(|node| {
                        node.routers = routers;
                        // -> indirect
                        node.ports = BTreeMap::new();
                        node.ips = vec![];
                    });
                    node = Some(name.clone());
                }
                "~ip" => {
                    let ip = bytes_to_ip(&decoded.data)?;
                    state.nodes.entry(name.clone()).and_modify(|node| {
                        node.ips.push(ip.to_string());
                        // -> direct
                        node.routers = vec![];
                    });
                    node = Some(name.clone());
                }
                _ => {}
            }
        }
        _ => {}
    }

    if let Some(block) = log.block_number {
        state.block = block;
    }

    if let Some(node) = node {
        if let Some(node_info) = state.nodes.get(&node) {
            if node_info.public_key != ""
                && ((!node_info.ips.is_empty() && !node_info.ports.is_empty())
                    || node_info.routers.len() > 0)
            {
                Request::new()
                    .target((&our.node, "net", "distro", "sys"))
                    .body(rmp_serde::to_vec(&net::NetAction::KnsUpdate(
                        node_info.clone(),
                    ))?)
                    .send()?;
            }
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
    let filter = filter.from_block(state.block - 1);
    loop {
        match eth_provider.get_logs(&filter) {
            Ok(logs) => {
                for log in logs {
                    if let Err(e) = handle_log(our, state, &log) {
                        print_to_terminal(1, &format!("log-handling error! {e:?}"));
                    }
                }
                return ();
            }
            Err(e) => {
                println!("got eth error while fetching logs: {e:?}, trying again in 5s...");
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        }
    }
}

fn get_node_name(state: &mut State, parent_hash: &str) -> String {
    let mut current_hash = parent_hash;
    let mut components = Vec::new(); // Collect components in a vector
    let mut visited_hashes = std::collections::HashSet::new();

    while let Some(parent_name) = state.names.get(current_hash) {
        if !visited_hashes.insert(current_hash) {
            break;
        }

        components.push(parent_name.clone());

        // Update current_hash to the parent's hash for the next iteration
        if let Some(new_parent_hash) = state.names.get(parent_name) {
            current_hash = new_parent_hash;
        } else {
            break;
        }
    }

    components.reverse();
    components.join(".")
}

/// note, unlike get_node_name, includes the label.
/// e.g label "testing" with parenthash_resolved = "parent.os" would return "testing.parent.os"
fn get_full_name(state: &mut State, label: &str, parent_hash: &str) -> String {
    let mut current_hash = parent_hash;
    let mut full_name = label.to_string();
    let mut visited_hashes = std::collections::HashSet::new();

    while let Some(parent_name) = state.names.get(current_hash) {
        if !visited_hashes.insert(current_hash) {
            break;
        }

        full_name = format!("{}.{}", full_name, parent_name);
        // Update current_hash to the parent's hash for the next iteration
        if let Some(new_parent_hash) = state.names.get(parent_name) {
            current_hash = new_parent_hash;
        } else {
            break;
        }
    }

    full_name
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

/// Decodes bytes into an array of node identities, expecting UTF-8 encoded strings separated by newlines.
fn decode_routers(data: &[u8]) -> anyhow::Result<Vec<String>> {
    let data_str = std::str::from_utf8(data)?;
    let routers = data_str.split(',').map(str::to_owned).collect();
    Ok(routers)
}

pub fn bytes_to_ip(bytes: &[u8]) -> anyhow::Result<IpAddr> {
    match bytes.len() {
        16 => {
            let ip_num = u128::from_be_bytes(bytes.try_into().unwrap());
            if ip_num < (1u128 << 32) {
                // IPv4
                Ok(IpAddr::V4(Ipv4Addr::from(ip_num as u32)))
            } else {
                // IPv6
                Ok(IpAddr::V6(Ipv6Addr::from(ip_num)))
            }
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

fn subscribe_to_logs(eth_provider: &eth::Provider, filter: eth::Filter, sub_id: u64) {
    loop {
        match eth_provider.subscribe(sub_id, filter.clone()) {
            Ok(()) => break,
            Err(_) => {
                println!("failed to subscribe to chain! trying again in 5s...");
                std::thread::sleep(std::time::Duration::from_secs(5));
                continue;
            }
        }
    }
}
