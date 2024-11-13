#![feature(let_chains)]
//! chain:app-store:sys
//! This process manages the on-chain interactions for the App Store system in the Kinode ecosystem.
//! It is responsible for indexing and tracking app metadata stored on the blockchain.
//!
//! ## Responsibilities:
//!
//! 1. Index and track app metadata from the blockchain.
//! 2. Manage subscriptions to relevant blockchain events.
//! 3. Provide up-to-date information about available apps and their metadata.
//! 4. Handle auto-update settings for apps.
//!
//! ## Key Components:
//!
//! - `handle_eth_log`: Processes blockchain events related to app metadata updates.
//! - `fetch_and_subscribe_logs`: Initializes and maintains blockchain event subscriptions.
//!
//! ## Interaction Flow:
//!
//! 1. The process subscribes to relevant blockchain events on startup.
//! 2. When new events are received, they are processed to update the local state.
//! 3. Other processes (like main) can request information about apps.
//! 4. The chain process responds with the most up-to-date information from its local state.
//!
//! Note: This process does not handle app binaries or installation. It focuses solely on
//! metadata management and providing information about available apps.
//!
use crate::kinode::process::chain::{
    ChainError, ChainRequests, OnchainApp, OnchainMetadata, OnchainProperties,
};
use crate::kinode::process::downloads::{AutoUpdateRequest, DownloadRequests};
use alloy_primitives::keccak256;
use alloy_sol_types::SolEvent;
use kinode::process::chain::ChainResponses;
use kinode_process_lib::{
    await_message, call_init, eth, get_blob, get_state, http, kernel_types as kt, kimap,
    print_to_terminal, println, timer, Address, Message, PackageId, Request, Response,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
};

wit_bindgen::generate!({
    path: "target/wit",
    generate_unused_types: true,
    world: "app-store-sys-v1",
    additional_derives: [serde::Deserialize, serde::Serialize, process_macros::SerdeJsonInto],
});

#[cfg(not(feature = "simulation-mode"))]
const CHAIN_ID: u64 = kimap::KIMAP_CHAIN_ID;
#[cfg(feature = "simulation-mode")]
const CHAIN_ID: u64 = 31337; // local

const CHAIN_TIMEOUT: u64 = 60; // 60s

#[cfg(not(feature = "simulation-mode"))]
const KIMAP_ADDRESS: &'static str = kimap::KIMAP_ADDRESS; // optimism
#[cfg(feature = "simulation-mode")]
const KIMAP_ADDRESS: &str = "0x9CE8cCD2932DC727c70f9ae4f8C2b68E6Abed58C";

const DELAY_MS: u64 = 1_000; // 1s

#[derive(Debug, Serialize, Deserialize)]
pub struct State {
    /// the kimap helper we are using
    pub kimap: kimap::Kimap,
    /// the last block at which we saved the state of the listings to disk.
    /// when we boot, we can read logs starting from this block and
    /// rebuild latest state.
    pub last_saved_block: u64,
    /// onchain listings
    pub listings: HashMap<PackageId, PackageListing>,
    /// set of packages that we have published
    pub published: HashSet<PackageId>,
}

/// listing information derived from metadata hash in listing event
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PackageListing {
    pub tba: eth::Address,
    pub metadata_uri: String,
    pub metadata_hash: String,
    // should this even be optional?
    // relegate to only valid apps maybe?
    pub metadata: Option<kt::Erc721Metadata>,
    pub auto_update: bool,
}

#[derive(Debug, Serialize, Deserialize, process_macros::SerdeJsonInto)]
#[serde(untagged)] // untagged as a meta-type for all incoming requests
pub enum Req {
    Eth(eth::EthSubResult),
    Request(ChainRequests),
}

call_init!(init);
fn init(our: Address) {
    println!(
        "chain started, indexing on contract address {}",
        KIMAP_ADDRESS
    );
    // create new provider with request-timeout of 60s
    // can change, log requests can take quite a long time.
    let eth_provider: eth::Provider = eth::Provider::new(CHAIN_ID, CHAIN_TIMEOUT);

    let mut state = fetch_state(eth_provider);
    fetch_and_subscribe_logs(&our, &mut state);

    loop {
        match await_message() {
            Err(send_error) => {
                print_to_terminal(1, &format!("chain: got network error: {send_error}"));
            }
            Ok(message) => {
                if let Err(e) = handle_message(&our, &mut state, &message) {
                    print_to_terminal(1, &format!("chain: error handling message: {:?}", e));
                }
            }
        }
    }
}

fn handle_message(our: &Address, state: &mut State, message: &Message) -> anyhow::Result<()> {
    if !message.is_request() {
        if message.is_local(&our) && message.source().process == "timer:distro:sys" {
            // handling of ETH RPC subscriptions delayed by DELAY_MS
            // to allow kns to have a chance to process block: handle now
            let Some(context) = message.context() else {
                return Err(anyhow::anyhow!("foo"));
            };
            let log = serde_json::from_slice(context)?;
            handle_eth_log(our, state, log, false)?;
            return Ok(());
        }
    } else {
        match message.body().try_into()? {
            Req::Eth(eth_result) => {
                if !message.is_local(our) || message.source().process != "eth:distro:sys" {
                    return Err(anyhow::anyhow!(
                        "eth sub event from unexpected address: {}",
                        message.source()
                    ));
                }

                if let Ok(eth::EthSub { result, .. }) = eth_result {
                    if let eth::SubscriptionResult::Log(ref log) = result {
                        // delay handling of ETH RPC subscriptions by DELAY_MS
                        // to allow kns to have a chance to process block
                        timer::set_timer(DELAY_MS, Some(serde_json::to_vec(log)?));
                    }
                } else {
                    // attempt to resubscribe
                    state
                        .kimap
                        .provider
                        .subscribe_loop(1, app_store_filter(state));
                }
            }
            Req::Request(chains) => {
                handle_local_request(state, chains)?;
            }
        }
    }

    Ok(())
}

fn handle_local_request(state: &mut State, req: ChainRequests) -> anyhow::Result<()> {
    match req {
        ChainRequests::GetApp(package_id) => {
            let onchain_app = state
                .listings
                .get(&package_id.clone().to_process_lib())
                .map(|app| OnchainApp {
                    package_id: package_id,
                    tba: app.tba.to_string(),
                    metadata_uri: app.metadata_uri.clone(),
                    metadata_hash: app.metadata_hash.clone(),
                    metadata: app.metadata.as_ref().map(|m| m.clone().into()),
                    auto_update: app.auto_update,
                });
            let response = ChainResponses::GetApp(onchain_app);
            Response::new().body(&response).send()?;
        }
        ChainRequests::GetApps => {
            let apps: Vec<OnchainApp> = state
                .listings
                .iter()
                .map(|(id, listing)| listing.to_onchain_app(id))
                .collect();

            let response = ChainResponses::GetApps(apps);
            Response::new().body(&response).send()?;
        }
        ChainRequests::GetOurApps => {
            let apps: Vec<OnchainApp> = state
                .published
                .iter()
                .filter_map(|id| {
                    state
                        .listings
                        .get(id)
                        .map(|listing| listing.to_onchain_app(id))
                })
                .collect();

            let response = ChainResponses::GetOurApps(apps);
            Response::new().body(&response).send()?;
        }
        ChainRequests::StartAutoUpdate(package_id) => {
            if let Some(listing) = state.listings.get_mut(&package_id.to_process_lib()) {
                listing.auto_update = true;
                let response = ChainResponses::AutoUpdateStarted;
                Response::new().body(&response).send()?;
            } else {
                let error_response = ChainResponses::Err(ChainError::NoPackage);
                Response::new().body(&error_response).send()?;
            }
        }
        ChainRequests::StopAutoUpdate(package_id) => {
            if let Some(listing) = state.listings.get_mut(&package_id.to_process_lib()) {
                listing.auto_update = false;
                let response = ChainResponses::AutoUpdateStopped;
                Response::new().body(&response).send()?;
            } else {
                let error_response = ChainResponses::Err(ChainError::NoPackage);
                Response::new().body(&error_response).send()?;
            }
        }
    }
    Ok(())
}

fn handle_eth_log(
    our: &Address,
    state: &mut State,
    log: eth::Log,
    startup: bool,
) -> anyhow::Result<()> {
    let block_number: u64 = log
        .block_number
        .ok_or(anyhow::anyhow!("log missing block number"))?;
    let Ok(note) = kimap::decode_note_log(&log) else {
        // ignore invalid logs here -- they're not actionable
        return Ok(());
    };

    let package_id = note
        .parent_path
        .split_once('.')
        .ok_or(anyhow::anyhow!("invalid publisher name"))
        .and_then(|(package, publisher)| {
            if package.is_empty() || publisher.is_empty() {
                Err(anyhow::anyhow!("invalid publisher name"))
            } else {
                Ok(PackageId::new(&package, &publisher))
            }
        })?;

    // the app store exclusively looks for ~metadata-uri postings: if one is
    // observed, we then *query* for ~metadata-hash to verify the content
    // at the URI.

    let metadata_uri = String::from_utf8_lossy(&note.data).to_string();
    let is_our_package = &package_id.publisher() == &our.node();

    let (tba, metadata_hash) = if !startup {
        // generate ~metadata-hash full-path
        let hash_note = format!("~metadata-hash.{}", note.parent_path);

        // owner can change which we don't track (yet?) so don't save, need to get when desired
        let (tba, _owner, data) = match state.kimap.get(&hash_note) {
            Ok(gr) => Ok(gr),
            Err(e) => match e {
                eth::EthError::RpcError(_) => {
                    // retry on RpcError after DELAY_MS sleep
                    // sleep here rather than with, e.g., a message to
                    //  `timer:distro:sys` so that events are processed in
                    //  order of receipt
                    std::thread::sleep(std::time::Duration::from_millis(DELAY_MS));
                    state.kimap.get(&hash_note)
                }
                _ => Err(e),
            },
        }
        .map_err(|e| anyhow::anyhow!("Couldn't find {hash_note}: {e:?}"))?;

        match data {
            None => {
                // if ~metadata-uri is also empty, this is an unpublish action!
                if metadata_uri.is_empty() {
                    state.published.remove(&package_id);
                    state.listings.remove(&package_id);
                    return Ok(());
                }
                return Err(anyhow::anyhow!(
                    "metadata hash not found: {package_id}, {metadata_uri}"
                ));
            }
            Some(hash_note) => (tba, String::from_utf8_lossy(&hash_note).to_string()),
        }
    } else {
        (eth::Address::ZERO, String::new())
    };

    if is_our_package {
        state.published.insert(package_id.clone());
    }

    // if this is a startup event, we don't need to fetch metadata from the URI --
    // we'll loop over all listings after processing all logs and fetch them as needed.
    // fetch metadata from the URI (currently only handling HTTP(S) URLs!)
    // assert that the metadata hash matches the fetched data
    let metadata = if !startup {
        Some(fetch_metadata_from_url(&metadata_uri, &metadata_hash, 30)?)
    } else {
        None
    };

    match state.listings.entry(package_id.clone()) {
        std::collections::hash_map::Entry::Occupied(mut listing) => {
            let listing = listing.get_mut();
            listing.metadata_uri = metadata_uri;
            listing.tba = tba;
            listing.metadata_hash = metadata_hash;
            listing.metadata = metadata.clone();
        }
        std::collections::hash_map::Entry::Vacant(listing) => {
            listing.insert(PackageListing {
                tba,
                metadata_uri,
                metadata_hash,
                metadata: metadata.clone(),
                auto_update: false,
            });
        }
    }

    if !startup {
        // if auto_update is enabled, send a message to downloads to kick off the update.
        if let Some(listing) = state.listings.get(&package_id) {
            if listing.auto_update {
                print_to_terminal(0, &format!("kicking off auto-update for: {}", package_id));
                Request::to(("our", "downloads", "app-store", "sys"))
                    .body(&DownloadRequests::AutoUpdate(AutoUpdateRequest {
                        package_id: crate::kinode::process::main::PackageId::from_process_lib(
                            package_id,
                        ),
                        metadata: metadata.unwrap().into(),
                    }))
                    .send()
                    .unwrap();
            }
        }
    }

    state.last_saved_block = block_number;

    Ok(())
}

/// after startup, fetch metadata for all listings
/// we do this as a separate step to not repeatedly fetch outdated metadata
/// as we process logs.
fn update_all_metadata(state: &mut State) {
    state.listings.retain(|package_id, listing| {
        let (tba, metadata_hash) = {
            // generate ~metadata-hash full-path
            let hash_note = format!(
                "~metadata-hash.{}.{}",
                package_id.package(),
                package_id.publisher()
            );

            // owner can change which we don't track (yet?) so don't save, need to get when desired
            let Ok((tba, _owner, data)) = (match state.kimap.get(&hash_note) {
                Ok(gr) => Ok(gr),
                Err(e) => match e {
                    eth::EthError::RpcError(_) => {
                        // retry on RpcError after DELAY_MS sleep
                        // sleep here rather than with, e.g., a message to
                        //  `timer:distro:sys` so that events are processed in
                        //  order of receipt
                        std::thread::sleep(std::time::Duration::from_millis(DELAY_MS));
                        state.kimap.get(&hash_note)
                    }
                    _ => Err(e),
                },
            }) else {
                return false;
            };

            match data {
                None => {
                    // if ~metadata-uri is also empty, this is an unpublish action!
                    if listing.metadata_uri.is_empty() {
                        state.published.remove(package_id);
                    }
                    return false;
                }
                Some(hash_note) => (tba, String::from_utf8_lossy(&hash_note).to_string()),
            }
        };
        listing.tba = tba;
        listing.metadata_hash = metadata_hash;
        let metadata =
            fetch_metadata_from_url(&listing.metadata_uri, &listing.metadata_hash, 30).ok();
        listing.metadata = metadata.clone();
        if listing.auto_update {
            print_to_terminal(0, &format!("kicking off auto-update for: {}", package_id));
            Request::to(("our", "downloads", "app-store", "sys"))
                .body(&DownloadRequests::AutoUpdate(AutoUpdateRequest {
                    package_id: crate::kinode::process::main::PackageId::from_process_lib(
                        package_id.clone(),
                    ),
                    metadata: metadata.unwrap().into(),
                }))
                .send()
                .unwrap();
        }
        true
    });
}

/// create the filter used for app store getLogs and subscription.
/// the app store exclusively looks for ~metadata-uri postings: if one is
/// observed, we then *query* for ~metadata-hash to verify the content
/// at the URI.
///
/// this means that ~metadata-hash should be *posted before or at the same time* as ~metadata-uri!
pub fn app_store_filter(state: &State) -> eth::Filter {
    let notes = vec![keccak256("~metadata-uri")];

    eth::Filter::new()
        .address(*state.kimap.address())
        .events([kimap::contract::Note::SIGNATURE])
        .topic3(notes)
}

/// create a filter to fetch app store event logs from chain and subscribe to new events
pub fn fetch_and_subscribe_logs(our: &Address, state: &mut State) {
    let filter = app_store_filter(state);
    // get past logs, subscribe to new ones.
    // subscribe first so we don't miss any logs
    println!("subscribing...");
    state.kimap.provider.subscribe_loop(1, filter.clone());
    for log in fetch_logs(
        &state.kimap.provider,
        &filter.from_block(state.last_saved_block),
    ) {
        if let Err(e) = handle_eth_log(our, state, log, true) {
            print_to_terminal(1, &format!("error ingesting log: {e}"));
        };
    }
    update_all_metadata(state);
}

/// fetch logs from the chain with a given filter
fn fetch_logs(eth_provider: &eth::Provider, filter: &eth::Filter) -> Vec<eth::Log> {
    loop {
        match eth_provider.get_logs(filter) {
            Ok(res) => return res,
            Err(_) => {
                println!("failed to fetch logs! trying again in 5s...");
                std::thread::sleep(std::time::Duration::from_secs(5));
                continue;
            }
        }
    }
}

/// fetch metadata from url and verify it matches metadata_hash
pub fn fetch_metadata_from_url(
    metadata_url: &str,
    metadata_hash: &str,
    timeout: u64,
) -> Result<kt::Erc721Metadata, anyhow::Error> {
    if let Ok(url) = url::Url::parse(metadata_url) {
        if let Ok(_) =
            http::client::send_request_await_response(http::Method::GET, url, None, timeout, vec![])
        {
            if let Some(body) = get_blob() {
                let hash = keccak_256_hash(&body.bytes);
                if &hash == metadata_hash {
                    return Ok(serde_json::from_slice::<kt::Erc721Metadata>(&body.bytes)
                        .map_err(|_| anyhow::anyhow!("metadata not found"))?);
                } else {
                    return Err(anyhow::anyhow!("metadata hash mismatch"));
                }
            }
        }
    }
    Err(anyhow::anyhow!("metadata not found"))
}

/// generate a Keccak-256 hash string (with 0x prefix) of the metadata bytes
pub fn keccak_256_hash(bytes: &[u8]) -> String {
    use sha3::{Digest, Keccak256};
    let mut hasher = Keccak256::new();
    hasher.update(bytes);
    format!("0x{:x}", hasher.finalize())
}

/// fetch state from disk or create a new one if that fails
pub fn fetch_state(provider: eth::Provider) -> State {
    if let Some(state_bytes) = get_state() {
        match serde_json::from_slice::<State>(&state_bytes) {
            Ok(state) => {
                if state.kimap.address().to_string() == KIMAP_ADDRESS {
                    return state;
                } else {
                    println!(
                        "state contract address mismatch. rebuilding state! expected {}, got {}",
                        KIMAP_ADDRESS,
                        state.kimap.address().to_string()
                    );
                }
            }
            Err(e) => println!("failed to deserialize saved state, rebuilding: {e}"),
        }
    }
    State {
        kimap: kimap::Kimap::new(provider, eth::Address::from_str(KIMAP_ADDRESS).unwrap()),
        last_saved_block: 0,
        listings: HashMap::new(),
        published: HashSet::new(),
    }
}

// quite annoyingly, we must convert from our gen'd version of PackageId
// to the process_lib's gen'd version. this is in order to access custom
// Impls that we want to use
impl crate::kinode::process::main::PackageId {
    pub fn to_process_lib(self) -> PackageId {
        PackageId {
            package_name: self.package_name,
            publisher_node: self.publisher_node,
        }
    }
    pub fn from_process_lib(package_id: PackageId) -> Self {
        Self {
            package_name: package_id.package_name,
            publisher_node: package_id.publisher_node,
        }
    }
}

impl PackageListing {
    pub fn to_onchain_app(&self, package_id: &PackageId) -> OnchainApp {
        OnchainApp {
            package_id: crate::kinode::process::main::PackageId::from_process_lib(
                package_id.clone(),
            ),
            tba: self.tba.to_string(),
            metadata_uri: self.metadata_uri.clone(),
            metadata_hash: self.metadata_hash.clone(),
            metadata: self.metadata.as_ref().map(|m| m.clone().into()),
            auto_update: self.auto_update,
        }
    }
}

impl From<kt::Erc721Metadata> for OnchainMetadata {
    fn from(erc: kt::Erc721Metadata) -> Self {
        OnchainMetadata {
            name: erc.name,
            description: erc.description,
            image: erc.image,
            external_url: erc.external_url,
            animation_url: erc.animation_url,
            properties: OnchainProperties {
                package_name: erc.properties.package_name,
                publisher: erc.properties.publisher,
                current_version: erc.properties.current_version,
                mirrors: erc.properties.mirrors,
                code_hashes: erc.properties.code_hashes.into_iter().collect(),
                license: erc.properties.license,
                screenshots: erc.properties.screenshots,
                wit_version: erc.properties.wit_version,
                dependencies: erc.properties.dependencies,
            },
        }
    }
}
