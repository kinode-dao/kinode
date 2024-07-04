use crate::kinode::process::kimap_indexer::{
    GetStateRequest, IndexerRequests, NamehashToNameRequest, NodeInfoRequest,
};
use alloy_primitives::{keccak256, FixedBytes};
use alloy_sol_types::{sol, SolCall, SolEvent};
use kinode_process_lib::{
    await_message, call_init,
    eth::{self, Provider, TransactionInput, TransactionRequest},
    net, println, Address, Message, Request, Response,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{hash_map::HashMap, BTreeMap},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    str::FromStr,
};
wit_bindgen::generate!({
    path: "target/wit",
    world: "kimap-indexer-sys-v0",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize],
});

#[cfg(not(feature = "simulation-mode"))]
const KIMAP_ADDRESS: &'static str = "0xca5b5811c0c40aab3295f932b1b5112eb7bb4bd6"; // optimism
#[cfg(feature = "simulation-mode")]
const KIMAP_ADDRESS: &'static str = "0x0165878A594ca255338adfa4d48449f69242Eb8F"; // local

#[cfg(not(feature = "simulation-mode"))]
const CHAIN_ID: u64 = 10; // optimism
#[cfg(feature = "simulation-mode")]
const CHAIN_ID: u64 = 31337; // local

#[cfg(not(feature = "simulation-mode"))]
const KIMAP_FIRST_BLOCK: u64 = 114_923_786; // optimism, adjust
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

sol! {
    // Kimap events
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
    println!("indexing on contract address {}", KIMAP_ADDRESS);

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

    match main(our, state) {
        Ok(_) => {}
        Err(e) => {
            println!("error: {:?}", e);
        }
    }
}

fn main(our: Address, mut state: State) -> anyhow::Result<()> {
    #[cfg(feature = "simulation-mode")]
    add_temp_hardcoded_tlzs(&mut state);

    let _notes = vec![
        keccak256("~net-key"),
        keccak256("~ws-port"),
        keccak256("~routers"),
        keccak256("~tcp-port"),
        keccak256("~ip"),
    ];

    let filter = eth::Filter::new()
        .address(state.contract_address.parse::<eth::Address>().unwrap())
        .from_block(state.block - 1)
        .to_block(eth::BlockNumberOrTag::Latest)
        .events(vec![
            "Mint(bytes32,bytes32,bytes,bytes)",
            "Note(bytes32,bytes32,bytes,bytes,bytes)",
        ]);
    // .topic3(_notes);
    // TODO: potentially remove labelhash from Mint event, then we can filter Notes while getting all Mint events?

    // 60s timeout -- these calls can take a long time
    // if they do time out, we try them again
    let eth_provider: eth::Provider = eth::Provider::new(state.chain_id, 60);

    println!(
        "subscribing, state.block: {}, chain_id: {}",
        state.block - 1,
        state.chain_id
    );

    subscribe_to_logs(&eth_provider, state.block - 1, filter.clone());

    // if block in state is < current_block, get logs from that part.
    loop {
        match eth_provider.get_logs(&filter) {
            Ok(logs) => {
                for log in logs {
                    match handle_log(&our, &mut state, &log, &eth_provider) {
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
            let Ok(request) = serde_json::from_slice(&body) else {
                println!("got invalid message");
                continue;
            };

            match request {
                // IndexerRequests, especially NamehashToName, relevant anymore? if they're mostly queried from the net runtime?
                IndexerRequests::NamehashToName(NamehashToNameRequest { ref hash, block }) => {
                    // if block <= state.block {
                    //     Response::new()
                    //         .body(serde_json::to_vec(&state.names.get(hash))?)
                    //         .send()?;
                    // } else {
                    //     pending_requests
                    //         .entry(block)
                    //         .or_insert(vec![])
                    //         .push(request);
                    // }
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
                match handle_log(our, state, &log, eth_provider) {
                    Ok(_) => {}
                    Err(e) => {
                        println!("log-handling error! {e:?}");
                    }
                }
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

fn handle_log(
    our: &Address,
    state: &mut State,
    log: &eth::Log,
    eth_provider: &Provider,
) -> anyhow::Result<()> {
    let mut node: Option<String> = None;
    match log.topics()[0] {
        Mint::SIGNATURE_HASH => {
            let decoded = Mint::decode_log_data(log.data(), true).unwrap();
            let parent_hash = decoded.parenthash.to_string();
            let child_hash = decoded.childhash.to_string();
            let label = String::from_utf8(decoded.name.to_vec())?;

            let name = get_full_name(state, &label, &parent_hash);

            let get_call = getCall {
                node: FixedBytes::<32>::from_str(&child_hash).unwrap(),
            }
            .abi_encode();
            let get_tx = TransactionRequest::default()
                .to(state.contract_address.parse::<eth::Address>().unwrap())
                .input(TransactionInput::new(get_call.into()));
            let res = eth_provider
                .call(get_tx, None)
                .map_err(|e| anyhow::anyhow!("tba get_call error: {:?}", e))?;

            let get_return = getCall::abi_decode_returns(&res, false)?;
            let tba = get_return.tokenBoundAccount.to_string();
            state.names.insert(child_hash.clone(), name.clone());
            println!(
                "got mint, name: {}, child_hash: {}, tba: {}",
                name, child_hash, tba
            );
            state
                .nodes
                .entry(name.clone())
                .or_insert_with(|| net::KnsUpdate {
                    name: name.clone(),
                    // tbh owner should be a separate one from tba. (although we won't index transfers so won't be up to date)
                    owner: tba,
                    node: child_hash.clone(),
                    public_key: String::new(),
                    ips: Vec::new(),
                    ports: BTreeMap::new(),
                    routers: Vec::new(),
                });

            Request::new()
                .target((&our.node, "net", "distro", "sys"))
                .body(rmp_serde::to_vec(&net::NetAction::AddName(
                    child_hash,
                    name.clone(),
                ))?)
                .send()?;
            node = Some(name);
        }
        Note::SIGNATURE_HASH => {
            let decoded = Note::decode_log_data(log.data(), true).unwrap();

            let note = String::from_utf8(decoded.note.to_vec())?;
            let _note_hash: String = decoded.notehash.to_string();
            let node_hash = decoded.nodehash.to_string();

            let name = get_node_name(state, &node_hash);

            println!(
                "got note, from name: {}, note: {}, note_hash: {}",
                name, note, node_hash
            );
            match note.as_str() {
                "~ws-port" => {
                    let ws = bytes_to_port(&decoded.data);

                    if let Ok(ws) = ws {
                        state.nodes.entry(name.clone()).and_modify(|node| {
                            node.ports.insert("ws".to_string(), ws);
                            // port defined, -> direct
                            node.routers = vec![];
                        });
                        node = Some(name.clone());
                    }
                }
                "~tcp-port" => {
                    let tcp = bytes_to_port(&decoded.data);
                    if let Ok(tcp) = tcp {
                        state.nodes.entry(name.clone()).and_modify(|node| {
                            node.ports.insert("tcp".to_string(), tcp);
                            // port defined, -> direct
                            node.routers = vec![];
                        });
                        node = Some(name.clone());
                    }
                }
                "~net-key" => {
                    let netkey = std::str::from_utf8(&decoded.data);
                    // note silent errors here...
                    // print silently for debugging?
                    if let Ok(netkey) = netkey {
                        state.nodes.entry(name.clone()).and_modify(|node| {
                            let pubkey = hex::encode(netkey);
                            node.public_key = pubkey;
                        });
                        node = Some(name.clone());
                    }
                }
                "~routers" => {
                    state.nodes.entry(name.clone()).and_modify(|node| {
                        if let Ok(routers) = decode_routers(&decoded.data) {
                            node.routers = routers;
                            // -> indirect
                            node.ports = BTreeMap::new();
                            node.ips = vec![];
                        }
                    });
                    node = Some(name.clone());
                }
                "~ip" => {
                    let ip = bytes_to_ip(&decoded.data);
                    if let Ok(ip) = ip {
                        state.nodes.entry(name.clone()).and_modify(|node| {
                            node.ips.push(ip.to_string());
                            // -> direct
                            node.routers = vec![];
                        });
                        node = Some(name.clone());
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }

    if let Some(node) = node {
        if let Some(node_info) = state.nodes.get(&node) {
            if node_info.public_key != ""
                && ((!node_info.ips.is_empty() && !node_info.ports.is_empty())
                    || node_info.routers.len() > 0)
            {
                println!("sending kns update for node: {}", node_info.node);
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

pub fn bytes_to_ip(bytes: &[u8]) -> Result<IpAddr, String> {
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
        _ => Err("Invalid byte length for IP address".to_string()),
    }
}

pub fn bytes_to_port(bytes: &[u8]) -> Result<u16, String> {
    match bytes.len() {
        2 => Ok(u16::from_be_bytes([bytes[0], bytes[1]])),
        _ => Err("Invalid byte length for port".to_string()),
    }
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
