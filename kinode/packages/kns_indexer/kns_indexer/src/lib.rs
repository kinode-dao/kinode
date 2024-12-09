use crate::kinode::process::kns_indexer::{
    GetStateRequest, IndexerRequests, NamehashToNameRequest, NodeInfoRequest,
};
use alloy_primitives::keccak256;
use alloy_sol_types::SolEvent;
use kinode_process_lib::{
    await_message, call_init, eth, kimap,
    kv::{self, Kv},
    net, print_to_terminal, println, timer, Address, Message, Request, Response,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    str::FromStr,
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
const KIMAP_ADDRESS: &'static str = "0x9CE8cCD2932DC727c70f9ae4f8C2b68E6Abed58C"; // local

#[cfg(not(feature = "simulation-mode"))]
const CHAIN_ID: u64 = kimap::KIMAP_CHAIN_ID; // optimism
#[cfg(feature = "simulation-mode")]
const CHAIN_ID: u64 = 31337; // local

#[cfg(not(feature = "simulation-mode"))]
const KIMAP_FIRST_BLOCK: u64 = kimap::KIMAP_FIRST_BLOCK; // optimism
#[cfg(feature = "simulation-mode")]
const KIMAP_FIRST_BLOCK: u64 = 1; // local

const MAX_PENDING_ATTEMPTS: u8 = 3;
const SUBSCRIPTION_TIMEOUT: u64 = 60;
const DELAY_MS: u64 = 1_000; // 1s

#[derive(Clone, Debug, Serialize, Deserialize)]
struct State {
    // version of the state in kv
    version: u32,
    // last block we have an update from
    last_block: u64,
    // kv handle
    // includes keys and values for:
    // "meta:chain_id", "meta:version", "meta:last_block", "meta:contract_address",
    // "names:{namehash}" -> "{name}", "nodes:{name}" -> "{node_info}"
    kv: Kv<String, Vec<u8>>, // todo: maybe serialize directly into known enum of possible types?
}

impl State {
    fn load(our: &Address) -> Self {
        let kv: Kv<String, Vec<u8>> = match kv::open(our.package_id(), "kns_indexer", Some(10)) {
            Ok(kv) => kv,
            Err(e) => panic!("fatal: error opening kns_indexer key_value database: {e:?}"),
        };

        let mut state = Self {
            kv,
            version: 0,
            last_block: 0,
        };

        // load or initialize chain_id
        let chain_id = state.get_chain_id();
        if chain_id == 0 {
            state.set_chain_id(CHAIN_ID);
        }

        // load or initialize contract_address
        let contract_address = state.get_contract_address();
        if contract_address
            == eth::Address::from_str(KIMAP_ADDRESS)
                .expect("Failed to parse KIMAP_ADDRESS constant")
        {
            state.set_contract_address(contract_address);
        }

        // load or initialize last_block
        let last_block = state.get_last_block();
        if last_block == 0 {
            state.set_last_block(KIMAP_FIRST_BLOCK);
        }

        // load or initialize version
        let version = state.get_version();
        if version == 0 {
            state.set_version(1); // Start at version 1
        }

        // update state struct with final values
        state.version = state.get_version();
        state.last_block = state.get_last_block();

        println!(
            "kns_indexer: loaded state: version: {}, last_block: {}, chain_id: {}, kimap_address: {}",
            state.version,
            state.last_block,
            state.get_chain_id(),
            state.get_contract_address()
        );

        state
    }

    fn meta_version_key() -> &'static str {
        "meta:version"
    }
    fn meta_last_block_key() -> &'static str {
        "meta:last_block"
    }
    fn meta_chain_id_key() -> &'static str {
        "meta:chain_id"
    }
    fn meta_contract_address_key() -> &'static str {
        "meta:contract_address"
    }

    fn name_key(namehash: &str) -> String {
        format!("names:{namehash}")
    }

    fn node_key(name: &str) -> String {
        format!("nodes:{name}")
    }

    fn get_last_block(&self) -> u64 {
        self.kv
            .get(&Self::meta_last_block_key().to_string())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or(0)
    }

    fn set_last_block(&mut self, block: u64) {
        self.kv
            .set(
                &Self::meta_last_block_key().to_string(),
                &serde_json::to_vec(&block).unwrap(),
                None,
            )
            .unwrap();
        self.last_block = block;
    }

    fn get_version(&self) -> u32 {
        self.kv
            .get(&Self::meta_version_key().to_string())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or(0)
    }

    fn set_version(&mut self, version: u32) {
        self.kv
            .set(
                &Self::meta_version_key().to_string(),
                &serde_json::to_vec(&version).unwrap(),
                None,
            )
            .unwrap();
        self.version = version;
    }

    fn get_name(&self, namehash: &str) -> Option<String> {
        self.kv
            .get(&Self::name_key(namehash))
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
    }

    fn set_name(&mut self, namehash: &str, name: &str) {
        self.kv
            .set(&Self::name_key(namehash), &name.as_bytes().to_vec(), None)
            .unwrap();
    }

    fn get_node(&self, name: &str) -> Option<net::KnsUpdate> {
        self.kv
            .get(&Self::node_key(name))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
    }

    fn set_node(&mut self, name: &str, node: &net::KnsUpdate) {
        self.kv
            .set(
                &Self::node_key(name),
                &serde_json::to_vec(&node).unwrap(),
                None,
            )
            .unwrap();
    }

    fn get_chain_id(&self) -> u64 {
        self.kv
            .get(&Self::meta_chain_id_key().to_string())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or(CHAIN_ID)
    }

    fn set_chain_id(&mut self, chain_id: u64) {
        self.kv
            .set(
                &Self::meta_chain_id_key().to_string(),
                &serde_json::to_vec(&chain_id).unwrap(),
                None,
            )
            .unwrap();
    }

    fn get_contract_address(&self) -> eth::Address {
        match self.kv.get(&Self::meta_contract_address_key().to_string()) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(addr) => addr,
                Err(_) => eth::Address::from_str(KIMAP_ADDRESS)
                    .expect("Failed to parse KIMAP_ADDRESS constant"),
            },
            Err(_) => eth::Address::from_str(KIMAP_ADDRESS)
                .expect("Failed to parse KIMAP_ADDRESS constant"),
        }
    }

    fn set_contract_address(&mut self, contract_address: eth::Address) {
        if let Ok(bytes) = serde_json::to_vec(&contract_address) {
            self.kv
                .set(&Self::meta_contract_address_key().to_string(), &bytes, None)
                .expect("Failed to set contract address");
        }
    }
}

// note: not defined in wit api right now like IndexerRequests.
#[derive(Clone, Debug, Serialize, Deserialize)]
enum IndexerResponses {
    Name(Option<String>),
    NodeInfo(Option<net::KnsUpdate>),
    GetState(State),
}

#[derive(Debug, thiserror::Error)]
enum KnsError {
    #[error("Parent node for note not found")]
    NoParentError,
}

call_init!(init);
fn init(our: Address) {
    println!("indexing on contract address {KIMAP_ADDRESS}");

    // state is loaded from kv, and updated with the current block number and version.
    let state = State::load(&our);

    if let Err(e) = main(our, state) {
        println!("fatal error: {e}");
    }
}

fn main(our: Address, mut state: State) -> anyhow::Result<()> {
    #[cfg(feature = "simulation-mode")]
    add_temp_hardcoded_tlzs(&mut state);

    // sub_id: 1
    let mints_filter = eth::Filter::new()
        .address(state.get_contract_address())
        .from_block(state.get_last_block())
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
        .address(state.get_contract_address())
        .from_block(state.get_last_block())
        .to_block(eth::BlockNumberOrTag::Latest)
        .event("Note(bytes32,bytes32,bytes,bytes,bytes)")
        .topic3(notes);

    // 60s timeout -- these calls can take a long time
    // if they do time out, we try them again
    let eth_provider: eth::Provider =
        eth::Provider::new(state.get_chain_id(), SUBSCRIPTION_TIMEOUT);

    print_to_terminal(
        1,
        &format!(
            "subscribing, state.block: {}, chain_id: {}",
            state.get_last_block() - 1,
            state.get_chain_id()
        ),
    );

    // subscribe to logs first, so no logs are missed
    println!("subscribing to new logs...");
    eth_provider.subscribe_loop(1, mints_filter.clone());
    eth_provider.subscribe_loop(2, notes_filter.clone());

    // if subscription results come back in the wrong order, we store them here
    // until the right block is reached.

    // pending_requests temporarily on timeout.
    // very naughty.
    // let mut pending_requests: BTreeMap<u64, Vec<IndexerRequests>> = BTreeMap::new();
    let mut pending_notes: BTreeMap<u64, Vec<(kimap::contract::Note, u8)>> = BTreeMap::new();

    // if block in state is < current_block, get logs from that part.
    println!("syncing old logs...");
    fetch_and_process_logs(
        &eth_provider,
        &mut state,
        mints_filter.clone(),
        &mut pending_notes,
    );
    fetch_and_process_logs(
        &eth_provider,
        &mut state,
        notes_filter.clone(),
        &mut pending_notes,
    );
    // set a timer tick so any pending logs will be processed
    timer::set_timer(DELAY_MS, None);
    println!("done syncing old logs.");

    loop {
        let Ok(message) = await_message() else {
            continue;
        };
        // if true, time to go check current block number and handle pending notes.
        let tick = message.is_local(&our) && message.source().process == "timer:distro:sys";
        let Message::Request { source, body, .. } = message else {
            if tick {
                handle_eth_message(
                    &mut state,
                    &eth_provider,
                    tick,
                    &mut pending_notes,
                    &[],
                    &mints_filter,
                    &notes_filter,
                )?;
            }
            continue;
        };

        if source.process == "eth:distro:sys" {
            handle_eth_message(
                &mut state,
                &eth_provider,
                tick,
                &mut pending_notes,
                &body,
                &mints_filter,
                &notes_filter,
            )?;
        } else {
            let request = serde_json::from_slice(&body)?;

            match request {
                IndexerRequests::NamehashToName(NamehashToNameRequest { ref hash, .. }) => {
                    // TODO: make sure we've seen the whole block, while actually
                    // sending a response to the proper place.
                    Response::new()
                        .body(serde_json::to_vec(&IndexerResponses::Name(
                            state.get_name(hash),
                        ))?)
                        .send()?;
                }

                IndexerRequests::NodeInfo(NodeInfoRequest { ref name, .. }) => {
                    Response::new()
                        .body(serde_json::to_vec(&IndexerResponses::NodeInfo(
                            state.get_node(name),
                        ))?)
                        .send()?;
                }
                // note no longer relevant.
                // TODO: redo with iterator once available.
                IndexerRequests::GetState(GetStateRequest { .. }) => {
                    Response::new().body(serde_json::to_vec(&state)?).send()?;
                }
            }
        }
    }
}

fn handle_eth_message(
    state: &mut State,
    eth_provider: &eth::Provider,
    tick: bool,
    pending_notes: &mut BTreeMap<u64, Vec<(kimap::contract::Note, u8)>>,
    body: &[u8],
    mints_filter: &eth::Filter,
    notes_filter: &eth::Filter,
) -> anyhow::Result<()> {
    match serde_json::from_slice::<eth::EthSubResult>(body) {
        Ok(Ok(eth::EthSub { result, .. })) => {
            if let eth::SubscriptionResult::Log(log) = result {
                if let Err(e) = handle_log(state, pending_notes, &log) {
                    print_to_terminal(1, &format!("log-handling error! {e:?}"));
                }
            }
        }
        Ok(Err(e)) => {
            println!("got eth subscription error ({e:?}), resubscribing");
            if e.id == 1 {
                eth_provider.subscribe_loop(1, mints_filter.clone());
            } else if e.id == 2 {
                eth_provider.subscribe_loop(2, notes_filter.clone());
            }
        }
        _ => {}
    }
    if tick {
        let block_number = eth_provider.get_block_number();
        if let Ok(block_number) = block_number {
            print_to_terminal(2, &format!("new block: {}", block_number));
            state.set_last_block(block_number);
        }
    }
    handle_pending_notes(state, pending_notes)?;

    if !pending_notes.is_empty() {
        timer::set_timer(DELAY_MS, None);
    }

    Ok(())
}

fn handle_pending_notes(
    state: &mut State,
    pending_notes: &mut BTreeMap<u64, Vec<(kimap::contract::Note, u8)>>,
) -> anyhow::Result<()> {
    if pending_notes.is_empty() {
        return Ok(());
    }
    let mut blocks_to_remove = vec![];

    for (block, notes) in pending_notes.iter_mut() {
        if *block < state.last_block {
            let mut keep_notes = Vec::new();
            for (note, attempt) in notes.drain(..) {
                if attempt >= MAX_PENDING_ATTEMPTS {
                    // skip notes that have exceeded max attempts
                    // print_to_terminal(
                    //     1,
                    //     &format!("dropping note from block {block} after {attempt} attempts"),
                    // );
                    continue;
                }
                if let Err(e) = handle_note(state, &note) {
                    match e.downcast_ref::<KnsError>() {
                        None => {
                            print_to_terminal(1, &format!("pending note handling error: {e:?}"))
                        }
                        Some(KnsError::NoParentError) => {
                            keep_notes.push((note, attempt + 1));
                        }
                    }
                }
            }
            if keep_notes.is_empty() {
                blocks_to_remove.push(*block);
            } else {
                *notes = keep_notes;
            }
        }
    }

    // remove processed blocks
    for block in blocks_to_remove {
        pending_notes.remove(&block);
    }

    Ok(())
}

fn handle_note(state: &mut State, note: &kimap::contract::Note) -> anyhow::Result<()> {
    let note_label = String::from_utf8(note.label.to_vec())?;
    let node_hash = note.parenthash.to_string();

    if !kimap::valid_note(&note_label) {
        return Err(anyhow::anyhow!("skipping invalid note: {note_label}"));
    }

    let Some(node_name) = get_parent_name(&state, &node_hash) else {
        return Err(KnsError::NoParentError.into());
    };

    if let Some(mut node) = state.get_node(&node_name) {
        match note_label.as_str() {
            "~ws-port" => {
                let ws = bytes_to_port(&note.data)?;
                node.ports.insert("ws".to_string(), ws);
                node.routers = vec![]; // port defined, -> direct
            }
            "~tcp-port" => {
                let tcp = bytes_to_port(&note.data)?;
                node.ports.insert("tcp".to_string(), tcp);
                node.routers = vec![]; // port defined, -> direct
            }
            "~net-key" => {
                if note.data.len() != 32 {
                    return Err(anyhow::anyhow!("invalid net-key length"));
                }
                node.public_key = hex::encode(&note.data);
            }
            "~routers" => {
                let routers = decode_routers(&note.data, state);
                node.routers = routers;
                node.ports = BTreeMap::new(); // -> indirect
                node.ips = vec![];
            }
            "~ip" => {
                let ip = bytes_to_ip(&note.data)?;
                node.ips = vec![ip.to_string()];
                node.routers = vec![]; // -> direct
            }
            _other => {
                // Ignore unknown notes
            }
        }

        // Update the node in the state
        state.set_node(&node_name, &node);

        // Only send an update if we have a *full* set of data for networking
        if !node.public_key.is_empty()
            && ((!node.ips.is_empty() && !node.ports.is_empty()) || !node.routers.is_empty())
        {
            Request::to(("our", "net", "distro", "sys"))
                .body(rmp_serde::to_vec(&net::NetAction::KnsUpdate(node))?)
                .send()?;
        }
    }

    Ok(())
}

fn handle_log(
    state: &mut State,
    pending_notes: &mut BTreeMap<u64, Vec<(kimap::contract::Note, u8)>>,
    log: &eth::Log,
) -> anyhow::Result<()> {
    if let Some(block) = log.block_number {
        state.set_last_block(block);
    }

    match log.topics()[0] {
        kimap::contract::Mint::SIGNATURE_HASH => {
            let decoded = kimap::contract::Mint::decode_log_data(log.data(), true).unwrap();
            let parent_hash = decoded.parenthash.to_string();
            let child_hash = decoded.childhash.to_string();
            let name = String::from_utf8(decoded.label.to_vec())?;

            if !kimap::valid_name(&name) {
                return Err(anyhow::anyhow!("skipping invalid name: {name}"));
            }

            let full_name = match get_parent_name(&state, &parent_hash) {
                Some(parent_name) => format!("{name}.{parent_name}"),
                None => name,
            };

            state.set_name(&child_hash.clone(), &full_name.clone());
            state.set_node(
                &full_name.clone(),
                &net::KnsUpdate {
                    name: full_name.clone(),
                    public_key: String::new(),
                    ips: Vec::new(),
                    ports: BTreeMap::new(),
                    routers: Vec::new(),
                },
            );
        }
        kimap::contract::Note::SIGNATURE_HASH => {
            let decoded = kimap::contract::Note::decode_log_data(log.data(), true).unwrap();
            let note: String = String::from_utf8(decoded.label.to_vec())?;

            if !kimap::valid_note(&note) {
                return Err(anyhow::anyhow!("skipping invalid note: {note}"));
            }
            // handle note: if it precedes parent mint event, add it to pending_notes
            if let Err(e) = handle_note(state, &decoded) {
                if let Some(KnsError::NoParentError) = e.downcast_ref::<KnsError>() {
                    if let Some(block_number) = log.block_number {
                        // print_to_terminal(
                        //     1,
                        //     &format!("adding note to pending_notes for block {block_number}"),
                        // );
                        pending_notes
                            .entry(block_number)
                            .or_default()
                            .push((decoded, 0));
                    }
                }
            }
        }
        _log => {
            return Ok(());
        }
    };

    Ok(())
}

// helpers

fn fetch_and_process_logs(
    eth_provider: &eth::Provider,
    state: &mut State,
    filter: eth::Filter,
    pending_notes: &mut BTreeMap<u64, Vec<(kimap::contract::Note, u8)>>,
) {
    let filter = filter.from_block(KIMAP_FIRST_BLOCK);
    loop {
        match eth_provider.get_logs(&filter) {
            Ok(logs) => {
                for log in logs {
                    if let Err(e) = handle_log(state, pending_notes, &log) {
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

fn get_parent_name(state: &State, parent_hash: &str) -> Option<String> {
    let mut current_hash = parent_hash.to_string();
    let mut components = Vec::new(); // Collect components in a vector
    let mut visited_hashes = std::collections::HashSet::new();

    while let Some(parent_name) = state.get_name(&current_hash) {
        if !visited_hashes.insert(current_hash.clone()) {
            break;
        }

        if !parent_name.is_empty() {
            components.push(parent_name.clone());
        }

        // Update current_hash to the parent's hash for the next iteration
        if let Some(new_parent_hash) = state.get_name(&parent_name) {
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
    state.set_name(
        &"0xdeeac81ae11b64e7cab86d089c306e5d223552a630f02633ce170d2786ff1bbd".to_string(),
        &"os".to_string(),
    );
    state.set_name(
        &"0x137d9e4cc0479164d40577620cb3b41b083c6e8dbf58f8523be76d207d6fd8ea".to_string(),
        &"dev".to_string(),
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

        match state.get_name(&hash_str) {
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
