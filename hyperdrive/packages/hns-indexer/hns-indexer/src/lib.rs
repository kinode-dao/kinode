use crate::hyperware::process::hns_indexer::{
    IndexerRequest, IndexerResponse, NamehashToNameRequest, NodeInfoRequest, ResetError,
    ResetResult, WitHnsUpdate, WitState,
};
use alloy_primitives::keccak256;
use alloy_sol_types::SolEvent;
use hyperware::process::standard::clear_state;
use hyperware_process_lib::{
    await_message, call_init, eth, get_state, hypermap, net, print_to_terminal, println, set_state,
    timer, Address, Capability, Message, Request, Response,
};
use std::{
    collections::{BTreeMap, HashMap},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    str::FromStr,
};

wit_bindgen::generate!({
    path: "target/wit",
    world: "hns-indexer-sys-v0",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize, process_macros::SerdeJsonInto],
});

const HYPERMAP_ADDRESS: &'static str = hypermap::HYPERMAP_ADDRESS;

#[cfg(not(feature = "simulation-mode"))]
const CHAIN_ID: u64 = hypermap::HYPERMAP_CHAIN_ID; // base
#[cfg(feature = "simulation-mode")]
const CHAIN_ID: u64 = 31337; // local

#[cfg(not(feature = "simulation-mode"))]
const HYPERMAP_FIRST_BLOCK: u64 = hypermap::HYPERMAP_FIRST_BLOCK; // base
#[cfg(feature = "simulation-mode")]
const HYPERMAP_FIRST_BLOCK: u64 = 1; // local

const MAX_PENDING_ATTEMPTS: u8 = 3;
const SUBSCRIPTION_TIMEOUT: u64 = 60;
const DELAY_MS: u64 = 1_000; // 1s
const CHECKPOINT_MS: u64 = 300_000; // 5 minutes

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct State {
    /// the chain id we are indexing
    chain_id: u64,
    /// what contract this state pertains to
    contract_address: eth::Address,
    /// namehash to human readable name
    names: HashMap<String, String>,
    /// human readable name to most recent on-chain routing information as json
    nodes: HashMap<String, net::HnsUpdate>,
    /// last saved checkpoint block
    last_checkpoint_block: u64,
}

impl State {
    fn new() -> Self {
        State {
            chain_id: CHAIN_ID,
            contract_address: eth::Address::from_str(HYPERMAP_ADDRESS).unwrap(),
            names: HashMap::new(),
            nodes: HashMap::new(),
            last_checkpoint_block: HYPERMAP_FIRST_BLOCK,
        }
    }

    fn load() -> Self {
        match get_state() {
            None => Self::new(),
            Some(state_bytes) => match rmp_serde::from_slice(&state_bytes) {
                Ok(state) => state,
                Err(e) => {
                    println!("failed to deserialize saved state: {e:?}");
                    Self::new()
                }
            },
        }
    }

    /// Reset by removing the checkpoint and reloading fresh state
    fn reset(&self) {
        clear_state();
    }

    /// Saves a checkpoint, serializes to the current block
    fn save(&mut self, block: u64) {
        self.last_checkpoint_block = block;
        match rmp_serde::to_vec(self) {
            Ok(state_bytes) => set_state(&state_bytes),
            Err(e) => println!("failed to serialize state: {e:?}"),
        }
    }

    /// loops through saved nodes, and sends them to net
    /// called upon bootup
    fn send_nodes(&self) -> anyhow::Result<()> {
        for node in self.nodes.values() {
            Request::to(("our", "net", "distro", "sys"))
                .body(rmp_serde::to_vec(&net::NetAction::HnsUpdate(node.clone()))?)
                .send()?;
        }
        Ok(())
    }
}

impl From<net::HnsUpdate> for WitHnsUpdate {
    fn from(k: net::HnsUpdate) -> Self {
        WitHnsUpdate {
            name: k.name,
            public_key: k.public_key,
            ips: k.ips,
            ports: k.ports.into_iter().map(|(k, v)| (k, v)).collect::<Vec<_>>(),
            routers: k.routers,
        }
    }
}

impl From<WitHnsUpdate> for net::HnsUpdate {
    fn from(k: WitHnsUpdate) -> Self {
        net::HnsUpdate {
            name: k.name,
            public_key: k.public_key,
            ips: k.ips,
            ports: BTreeMap::from_iter(k.ports),
            routers: k.routers,
        }
    }
}

impl From<State> for WitState {
    fn from(s: State) -> Self {
        let contract_address: [u8; 20] = s.contract_address.into();
        WitState {
            chain_id: s.chain_id,
            contract_address: contract_address.to_vec(),
            names: s.names.into_iter().map(|(k, v)| (k, v)).collect::<Vec<_>>(),
            nodes: s
                .nodes
                .into_iter()
                .map(|(k, v)| (k, v.into()))
                .collect::<Vec<_>>(),
            last_block: s.last_checkpoint_block,
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum HnsError {
    #[error("Parent node for note not found")]
    NoParentError,
}

call_init!(init);
fn init(our: Address) {
    println!("started");

    // state is checkpointed regularly (default every 5 minutes if new events are found)
    let mut state = State::load();

    loop {
        if let Err(e) = main(&our, &mut state) {
            println!("fatal error: {e}");
            break;
        }
    }
}

fn main(our: &Address, state: &mut State) -> anyhow::Result<()> {
    #[cfg(feature = "simulation-mode")]
    add_temp_hardcoded_tlzs(state);

    // loop through checkpointed values and send to net
    if let Err(e) = state.send_nodes() {
        // todo change verbosity
        println!("failed to send nodes to net: {e}");
    }

    // current block is only saved to state upon checkpoints, we use this to keep track of last events
    // set to checkpoint-1
    let mut last_block = state.last_checkpoint_block.saturating_sub(1);

    // sub_id: 1
    // listen to all mint events in hypermap
    let mints_filter = eth::Filter::new()
        .address(state.contract_address)
        .from_block(last_block)
        .to_block(eth::BlockNumberOrTag::Latest)
        .event("Mint(bytes32,bytes32,bytes,bytes)");

    // sub_id: 2
    // listen to all note events that are relevant to the HNS protocol within hypermap
    let notes_filter = eth::Filter::new()
        .address(state.contract_address)
        .from_block(last_block)
        .to_block(eth::BlockNumberOrTag::Latest)
        .event("Note(bytes32,bytes32,bytes,bytes,bytes)")
        .topic3(vec![
            keccak256("~ws-port"),
            keccak256("~tcp-port"),
            keccak256("~net-key"),
            keccak256("~routers"),
            keccak256("~ip"),
        ]);

    // 60s timeout -- these calls can take a long time
    // if they do time out, we try them again
    let eth_provider: eth::Provider = eth::Provider::new(state.chain_id, SUBSCRIPTION_TIMEOUT);

    // subscribe to logs first, so no logs are missed
    eth_provider.subscribe_loop(1, mints_filter.clone(), 2, 0);
    eth_provider.subscribe_loop(2, notes_filter.clone(), 2, 0);

    // if subscription results come back in the wrong order, we store them here
    // until the right block is reached.

    // pending_requests temporarily on timeout.
    // very naughty.
    // let mut pending_requests: BTreeMap<u64, Vec<IndexerRequest>> = BTreeMap::new();
    let mut pending_notes: BTreeMap<u64, Vec<(hypermap::contract::Note, u8)>> = BTreeMap::new();

    // if block in state is < current_block, get logs from that part.
    print_to_terminal(2, &format!("syncing old logs from block: {}", last_block));
    fetch_and_process_logs(
        &eth_provider,
        state,
        mints_filter.clone(),
        &mut pending_notes,
        &mut last_block,
    );
    fetch_and_process_logs(
        &eth_provider,
        state,
        notes_filter.clone(),
        &mut pending_notes,
        &mut last_block,
    );

    // set a timer tick so any pending logs will be processed
    timer::set_timer(DELAY_MS, None);

    // set a timer tick for checkpointing
    timer::set_timer(CHECKPOINT_MS, Some(b"checkpoint".to_vec()));

    print_to_terminal(2, "done syncing old logs.");

    loop {
        let Ok(message) = await_message() else {
            continue;
        };

        // if true, time to go check current block number and handle pending notes.
        let tick = message.is_local() && message.source().process == "timer:distro:sys";
        let checkpoint = message.is_local()
            && message.source().process == "timer:distro:sys"
            && message.context() == Some(b"checkpoint");

        let Message::Request {
            source,
            body,
            capabilities,
            expects_response,
            ..
        } = message
        else {
            if tick {
                handle_eth_message(
                    state,
                    &eth_provider,
                    tick,
                    checkpoint,
                    &mut pending_notes,
                    &[],
                    &mints_filter,
                    &notes_filter,
                    &mut last_block,
                )?;
            }
            continue;
        };

        if source.node() == our.node() && source.process == "eth:distro:sys" {
            handle_eth_message(
                state,
                &eth_provider,
                tick,
                checkpoint,
                &mut pending_notes,
                &body,
                &mints_filter,
                &notes_filter,
                &mut last_block,
            )?;
        } else {
            let response_body = match serde_json::from_slice(&body)? {
                IndexerRequest::NamehashToName(NamehashToNameRequest { ref hash, .. }) => {
                    // TODO: make sure we've seen the whole block, while actually
                    // sending a response to the proper place.
                    IndexerResponse::Name(state.names.get(hash).cloned())
                }
                IndexerRequest::NodeInfo(NodeInfoRequest { ref name, .. }) => {
                    // if we don't have the node in our state, before sending a response,
                    // try a hypermap get to see if it exists onchain and the indexer missed it.
                    match state.nodes.get(name) {
                        Some(node) => IndexerResponse::NodeInfo(Some(node.clone().into())),
                        None => {
                            let mut response = IndexerResponse::NodeInfo(None);
                            if let Some(timeout) = expects_response {
                                if let Some(hns_update) = fetch_node(timeout, name, state) {
                                    response =
                                        IndexerResponse::NodeInfo(Some(hns_update.clone().into()));
                                    // save the node to state
                                    state.nodes.insert(name.clone(), hns_update.clone());
                                    // produce namehash and save in names map
                                    state.names.insert(hypermap::namehash(name), name.clone());
                                    // send the node to net
                                    Request::to(("our", "net", "distro", "sys"))
                                        .body(rmp_serde::to_vec(&net::NetAction::HnsUpdate(
                                            hns_update,
                                        ))?)
                                        .send()?;
                                }
                            }
                            response
                        }
                    }
                }
                IndexerRequest::Reset => {
                    // check for root capability
                    let root_cap = Capability::new(our.clone(), "{\"root\":true}");
                    if source.package_id() != our.package_id() && !capabilities.contains(&root_cap)
                    {
                        IndexerResponse::Reset(ResetResult::Err(ResetError::NoRootCap))
                    } else {
                        // reload state fresh - this will create new db
                        state.reset();
                        IndexerResponse::Reset(ResetResult::Success)
                    }
                }
                IndexerRequest::GetState(_) => IndexerResponse::GetState(state.clone().into()),
            };

            if let IndexerResponse::Reset(ResetResult::Success) = response_body {
                println!("resetting state");
                if expects_response.is_some() {
                    Response::new()
                        .body(IndexerResponse::Reset(ResetResult::Success))
                        .send()?;
                }
                return Ok(());
            } else {
                if expects_response.is_some() {
                    Response::new().body(response_body).send()?;
                }
            }
        }
    }
}

fn handle_eth_message(
    state: &mut State,
    eth_provider: &eth::Provider,
    tick: bool,
    checkpoint: bool,
    pending_notes: &mut BTreeMap<u64, Vec<(hypermap::contract::Note, u8)>>,
    body: &[u8],
    mints_filter: &eth::Filter,
    notes_filter: &eth::Filter,
    last_block: &mut u64,
) -> anyhow::Result<()> {
    match serde_json::from_slice::<eth::EthSubResult>(body) {
        Ok(Ok(eth::EthSub { result, .. })) => {
            if let Ok(eth::SubscriptionResult::Log(log)) =
                serde_json::from_value::<eth::SubscriptionResult>(result)
            {
                if let Err(e) = handle_log(state, pending_notes, &log, last_block) {
                    print_to_terminal(1, &format!("log-handling error! {e:?}"));
                }
            }
        }
        Ok(Err(e)) => {
            println!("got eth subscription error ({e:?}), resubscribing");
            if e.id == 1 {
                eth_provider.subscribe_loop(1, mints_filter.clone(), 2, 0);
            } else if e.id == 2 {
                eth_provider.subscribe_loop(2, notes_filter.clone(), 2, 0);
            }
        }
        _ => {}
    }

    if tick {
        let block_number = eth_provider.get_block_number();
        if let Ok(block_number) = block_number {
            print_to_terminal(2, &format!("new block: {}", block_number));
            *last_block = block_number;
            if checkpoint {
                state.save(block_number);
            }
        }
    }
    handle_pending_notes(state, pending_notes, last_block)?;

    if !pending_notes.is_empty() {
        timer::set_timer(DELAY_MS, None);
    }

    Ok(())
}

fn handle_pending_notes(
    state: &mut State,
    pending_notes: &mut BTreeMap<u64, Vec<(hypermap::contract::Note, u8)>>,
    last_block: &mut u64,
) -> anyhow::Result<()> {
    if pending_notes.is_empty() {
        return Ok(());
    }
    let mut blocks_to_remove = vec![];

    for (block, notes) in pending_notes.iter_mut() {
        if block < last_block {
            let mut keep_notes = Vec::new();
            for (note, attempt) in notes.drain(..) {
                if attempt >= MAX_PENDING_ATTEMPTS {
                    // skip notes that have exceeded max attempts
                    continue;
                }
                if let Err(e) = handle_note(state, &note) {
                    match e.downcast_ref::<HnsError>() {
                        None => {
                            print_to_terminal(1, &format!("pending note handling error: {e:?}"))
                        }
                        Some(HnsError::NoParentError) => {
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

fn handle_note(state: &mut State, note: &hypermap::contract::Note) -> anyhow::Result<()> {
    let note_label = String::from_utf8(note.label.to_vec())?;
    let node_hash = note.parenthash.to_string();

    if !hypermap::valid_note(&note_label) {
        return Err(anyhow::anyhow!("skipping invalid note: {note_label}"));
    }

    let Some(node_name) = state.names.get(&node_hash) else {
        return Err(HnsError::NoParentError.into());
    };

    match note_label.as_str() {
        "~ws-port" => {
            let ws = bytes_to_port(&note.data)?;
            if let Some(node) = state.nodes.get_mut(node_name) {
                node.ports.insert("ws".to_string(), ws);
                // port defined, -> direct
                node.routers = vec![];
            }
        }
        "~tcp-port" => {
            let tcp = bytes_to_port(&note.data)?;
            if let Some(node) = state.nodes.get_mut(node_name) {
                node.ports.insert("tcp".to_string(), tcp);
                // port defined, -> direct
                node.routers = vec![];
            }
        }
        "~net-key" => {
            if note.data.len() != 32 {
                return Err(anyhow::anyhow!("invalid net-key length"));
            }
            if let Some(node) = state.nodes.get_mut(node_name) {
                node.public_key = hex::encode(&note.data);
            }
        }
        "~routers" => {
            let routers = decode_routers(&note.data, state);
            if let Some(node) = state.nodes.get_mut(node_name) {
                node.routers = routers;
                // -> indirect
                node.ports = BTreeMap::new();
                node.ips = vec![];
            }
        }
        "~ip" => {
            let ip = bytes_to_ip(&note.data)?;
            if let Some(node) = state.nodes.get_mut(node_name) {
                node.ips = vec![ip.to_string()];
                // -> direct
                node.routers = vec![];
            }
        }
        _other => {
            // Ignore unknown notes
        }
    }

    // only send an update if we have a *full* set of data for networking:
    // a node name, plus either <routers> or <ip, port(s)>
    if let Some(node_info) = state.nodes.get(node_name) {
        if !node_info.public_key.is_empty()
            && ((!node_info.ips.is_empty() && !node_info.ports.is_empty())
                || node_info.routers.len() > 0)
        {
            Request::to(("our", "net", "distro", "sys"))
                .body(rmp_serde::to_vec(&net::NetAction::HnsUpdate(
                    node_info.clone(),
                ))?)
                .send()?;
        }
    }

    Ok(())
}

fn handle_log(
    state: &mut State,
    pending_notes: &mut BTreeMap<u64, Vec<(hypermap::contract::Note, u8)>>,
    log: &eth::Log,
    last_block: &mut u64,
) -> anyhow::Result<()> {
    if let Some(block) = log.block_number {
        *last_block = block;
    }

    match log.topics()[0] {
        hypermap::contract::Mint::SIGNATURE_HASH => {
            let decoded = hypermap::contract::Mint::decode_log_data(log.data(), true).unwrap();
            let parent_hash = decoded.parenthash.to_string();
            let child_hash = decoded.childhash.to_string();
            let name = String::from_utf8(decoded.label.to_vec())?;

            if !hypermap::valid_name(&name) {
                return Err(anyhow::anyhow!("skipping invalid name: {name}"));
            }

            let full_name = match state.names.get(&parent_hash) {
                Some(parent_name) => format!("{name}.{parent_name}"),
                None => name,
            };

            state.names.insert(child_hash.clone(), full_name.clone());
            state.nodes.insert(
                full_name.clone(),
                net::HnsUpdate {
                    name: full_name.clone(),
                    public_key: String::new(),
                    ips: Vec::new(),
                    ports: BTreeMap::new(),
                    routers: Vec::new(),
                },
            );
        }
        hypermap::contract::Note::SIGNATURE_HASH => {
            let decoded = hypermap::contract::Note::decode_log_data(log.data(), true).unwrap();
            let note: String = String::from_utf8(decoded.label.to_vec())?;

            if !hypermap::valid_note(&note) {
                return Err(anyhow::anyhow!("skipping invalid note: {note}"));
            }
            // handle note: if it precedes parent mint event, add it to pending_notes
            if let Err(e) = handle_note(state, &decoded) {
                if let Some(HnsError::NoParentError) = e.downcast_ref::<HnsError>() {
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

/// Get logs for a filter then process them while taking pending notes into account.
fn fetch_and_process_logs(
    eth_provider: &eth::Provider,
    state: &mut State,
    filter: eth::Filter,
    pending_notes: &mut BTreeMap<u64, Vec<(hypermap::contract::Note, u8)>>,
    last_block: &mut u64,
) {
    loop {
        match eth_provider.get_logs(&filter) {
            Ok(logs) => {
                print_to_terminal(2, &format!("log len: {}", logs.len()));
                for log in logs {
                    if let Err(e) = handle_log(state, pending_notes, &log, last_block) {
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

fn fetch_node(timeout: u64, name: &str, state: &State) -> Option<net::HnsUpdate> {
    let hypermap = hypermap::Hypermap::default(timeout - 1);
    if let Ok((_tba, _owner, _data)) = hypermap.get(name) {
        let Ok(Some(public_key_bytes)) = hypermap
            .get(&format!("~net-key.{name}"))
            .map(|(_, _, data)| data)
        else {
            return None;
        };

        let maybe_ip = hypermap
            .get(&format!("~ip.{name}"))
            .map(|(_, _, data)| data.map(|b| bytes_to_ip(&b)));

        let maybe_tcp_port = hypermap
            .get(&format!("~tcp-port.{name}"))
            .map(|(_, _, data)| data.map(|b| bytes_to_port(&b)));

        let maybe_ws_port = hypermap
            .get(&format!("~ws-port.{name}"))
            .map(|(_, _, data)| data.map(|b| bytes_to_port(&b)));

        let maybe_routers = hypermap
            .get(&format!("~routers.{name}"))
            .map(|(_, _, data)| data.map(|b| decode_routers(&b, state)));

        let mut ports = BTreeMap::new();
        if let Ok(Some(Ok(tcp_port))) = maybe_tcp_port {
            ports.insert("tcp".to_string(), tcp_port);
        }
        if let Ok(Some(Ok(ws_port))) = maybe_ws_port {
            ports.insert("ws".to_string(), ws_port);
        }

        Some(net::HnsUpdate {
            name: name.to_string(),
            public_key: hex::encode(public_key_bytes),
            ips: if let Ok(Some(Ok(ip))) = maybe_ip {
                vec![ip.to_string()]
            } else {
                vec![]
            },
            ports,
            routers: if let Ok(Some(routers)) = maybe_routers {
                routers
            } else {
                vec![]
            },
        })
    } else {
        None
    }
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

/// Decodes bytes under ~routers in hypermap into an array of keccak256 hashes (32 bytes each)
/// and returns the associated node identities.
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

/// convert IP address stored at ~ip in hypermap to IpAddr
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

/// convert port stored at ~[protocol]-port in hypermap to u16
pub fn bytes_to_port(bytes: &[u8]) -> anyhow::Result<u16> {
    match bytes.len() {
        2 => Ok(u16::from_be_bytes([bytes[0], bytes[1]])),
        _ => Err(anyhow::anyhow!("Invalid byte length for port")),
    }
}
