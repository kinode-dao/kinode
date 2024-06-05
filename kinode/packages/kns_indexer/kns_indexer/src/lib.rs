use crate::kinode::process::kns_indexer::{
    GetStateRequest, IndexerRequests, NamehashToNameRequest, NodeInfoRequest,
};
use alloy_sol_types::{sol, SolEvent};
use kinode_process_lib::{await_message, call_init, eth, println, Address, Message, Response};
use serde::{Deserialize, Serialize};
use std::collections::{hash_map::HashMap, BTreeMap};

wit_bindgen::generate!({
    path: "target/wit",
    world: "kns-indexer-sys-v0",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize],
});

#[cfg(not(feature = "simulation-mode"))]
const KIMAP_ADDRESS: &'static str = "0xca5b5811c0c40aab3295f932b1b5112eb7bb4bd6"; // optimism
#[cfg(feature = "simulation-mode")]
const KIMAP_ADDRESS: &'static str = "0x5FC8d32690cc91D4c39d9d3abcBD16989F875707"; // local

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
    // temporary hash->name mapping
    hashes: HashMap<String, String>,
    // notehash->note mapping
    // note, do not need this here, adding relevant notes directly to KNS rn.
    // notes: HashMap<String, Note>,
    // NOTE: wip knsUpdates not 1-1 rn
    nodes: HashMap<String, Node>,
    // last block we have an update from
    block: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Node {
    pub name: String, // actual username / domain name
    pub hash: String, // hex namehash of node
    // pub tba: String, can query for this as events come in too.
    pub parent_hash: String, // hex namehash of parent node, top level = 0x0?
    pub public_key: Option<String>,
    pub ips: Vec<String>,
    pub ports: BTreeMap<String, u16>,
    pub routers: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Note {
    pub name: String,      // note full name
    pub hash: String,      // hex namehash of note (in key already?)
    pub node_hash: String, // hex namehash of node
    pub value: String,     // note value, hex/bytes instead?
}

sol! {
    // Kimap events
    event Name(bytes32 indexed parent, bytes32 indexed child, bytes label);
    event NewNote(bytes32 indexed node, bytes32 indexed notehash, bytes note, bytes data);
    event UpdateNote(bytes32 indexed note, bytes data);
    event Zeroth(address zerotba);
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
        names: HashMap::new(),
        hashes: HashMap::new(),
        nodes: HashMap::new(),
        // notes: HashMap::new(),
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
        .from_block(state.block - 1)
        .to_block(eth::BlockNumberOrTag::Latest)
        .events(vec![
            "Name(bytes32,bytes32,bytes)",
            "NewNote(bytes32,bytes32,bytes,bytes)",
            "UpdateNote(bytes32,bytes)",
            "Zeroth(address)",
        ]);

    // 60s timeout -- these calls can take a long time
    // if they do time out, we try them again
    let eth_provider = eth::Provider::new(state.chain_id, 60);

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

fn get_full_name(state: &mut State, label: &str, parent_hash: &str) -> String {
    let mut current_hash = parent_hash;
    let mut full_name = label.to_string();

    // Traverse up the hierarchy by following the node hash to find its parent name
    while let Some(parent_name) = state.names.get(current_hash) {
        full_name = format!("{}.{}", full_name, parent_name);
        // Update current_hash to the parent's hash for the next iteration
        if let Some(new_parent_hash) = state.hashes.get(parent_name) {
            current_hash = new_parent_hash;
        } else {
            break;
        }
    }

    full_name
}

fn handle_log(_our: &Address, state: &mut State, log: &eth::Log) -> anyhow::Result<()> {
    match log.topics()[0] {
        Name::SIGNATURE_HASH => {
            let decoded = Name::decode_log_data(log.data(), true).unwrap();

            let label = String::from_utf8(decoded.label.to_vec())?;
            let parent_hash = decoded.parent.to_string();
            let node_hash = decoded.child.to_string();

            println!(
                "got name, node_hash, parent_node: {:?}, {:?}, {:?}",
                label, node_hash, parent_hash
            );

            let full_name = get_full_name(state, &label, &parent_hash);

            println!("got full hierarchical name: {:?}", full_name);
            state.names.insert(node_hash.clone(), full_name);
            state.hashes.insert(node_hash, label);
        }
        NewNote::SIGNATURE_HASH => {
            let decoded = NewNote::decode_log_data(log.data(), true).unwrap();

            let note = String::from_utf8(decoded.note.to_vec())?;
            let _notehash: String = decoded.notehash.to_string();
            let node = decoded.node.to_string();

            let full_note_name = get_full_name(state, &note, &node);

            println!("got full note name: {:?}", full_note_name);

            let note_value = String::from_utf8(decoded.data.to_vec())?;

            println!("got note: {:?}", note);
            println!("got note value: {:?}", note_value);

            // generalize, cleaner system
            match note.as_str() {
                "~wsport" => {
                    state.nodes.entry(node.clone()).and_modify(|node| {
                        node.ports
                            .insert("ws".to_string(), note_value.parse().unwrap());
                    });
                }
                "~networkingkey" => {
                    state.nodes.entry(node.clone()).and_modify(|node| {
                        node.public_key = Some(note_value.clone());
                    });
                }
                "~routers" => {
                    state.nodes.entry(node.clone()).and_modify(|node| {
                        node.routers.push(note_value.clone());
                    });
                }
                "~ip" => {
                    state.nodes.entry(node.clone()).and_modify(|node| {
                        node.ips.push(note_value.clone());
                    });
                }
                _ => {}
            }

            // todo: update corresponding node info at right time and send to KNS.
        }
        UpdateNote::SIGNATURE_HASH => {
            let _decoded = UpdateNote::decode_log_data(log.data(), true).unwrap();

            println!("got updated note!");
            // state.notes.entry(note_hash).and_modify(|note| {
            //     note.value = note_data.clone();
            // });
        }
        Zeroth::SIGNATURE_HASH => {
            // println!("got zeroth log: {:?}", log);
        }
        _ => {
            println!("got other log: {:?}", log);
        }
    }

    Ok(())
}
