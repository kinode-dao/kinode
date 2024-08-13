#![feature(let_chains)]
//! chain:app_store:sys
//! manages indexing relevant packages and their versions from the kimap.
//! keeps eth subscriptions open, keeps data updated.
//!
use std::{collections::HashMap, str::FromStr};

use crate::kinode::process::chain::{
    Chains, GetAppResponse, OnChainApp, OnChainMetadata, OnChainProperties,
};
use crate::kinode::process::main::Error;
use alloy_primitives::keccak256;
use alloy_sol_types::SolEvent;
use kinode_process_lib::{
    await_message, call_init, eth, get_blob, get_state, http, kernel_types as kt, kimap,
    print_to_terminal, println, Address, Message, PackageId, Response,
};

use serde::{Deserialize, Serialize};

wit_bindgen::generate!({
    path: "target/wit",
    generate_unused_types: true,
    world: "app-store-sys-v0",
    additional_derives: [serde::Deserialize, serde::Serialize],
});

#[cfg(not(feature = "simulation-mode"))]
const CHAIN_ID: u64 = kimap::KIMAP_CHAIN_ID;
#[cfg(feature = "simulation-mode")]
const CHAIN_ID: u64 = 31337; // local

const CHAIN_TIMEOUT: u64 = 60; // 60s

#[cfg(not(feature = "simulation-mode"))]
const KIMAP_ADDRESS: &str = kimap::KIMAP_ADDRESS;
#[cfg(feature = "simulation-mode")]
const KIMAP_ADDRESS: &str = "0xEce71a05B36CA55B895427cD9a440eEF7Cf3669D";

#[cfg(not(feature = "simulation-mode"))]
const KIMAP_FIRST_BLOCK: u64 = kimap::KIMAP_FIRST_BLOCK;
#[cfg(feature = "simulation-mode")]
const KIMAP_FIRST_BLOCK: u64 = 1;

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
    // auto-update, maybe... -> means, when you get a new thing on this packge,
    // tell download to go fetch it, and tell main to go install it
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)] // untagged as a meta-type for all incoming requests
pub enum Req {
    Eth(eth::EthSubResult),
    Request(Chains),
}

call_init!(init);
fn init(our: Address) {
    println!("started, indexing on contract address {}", KIMAP_ADDRESS);
    // create new provider with request-timeout of 60s
    // can change, log requests can take quite a long time.
    let eth_provider: eth::Provider = eth::Provider::new(CHAIN_ID, CHAIN_TIMEOUT);

    let mut state = fetch_state(eth_provider);
    fetch_and_subscribe_logs(&mut state);

    loop {
        match await_message() {
            Err(send_error) => {
                // TODO handle these based on what they are triggered by
                println!("got network error: {send_error}");
            }
            Ok(message) => {
                if let Err(e) = handle_message(&our, &mut state, &message) {
                    println!("error handling message: {:?}", e);
                    let _ = Response::new()
                        .body(
                            serde_json::to_vec(&Error {
                                reason: e.to_string(),
                            })
                            .unwrap(),
                        )
                        .send();
                }
            }
        }
    }
}

fn handle_message(our: &Address, state: &mut State, message: &Message) -> anyhow::Result<()> {
    if message.is_request() {
        let req: Req = serde_json::from_slice(message.body())?;
        match req {
            Req::Eth(eth_result) => {
                if !message.is_local(our) || message.source().process != "eth:distro:sys" {
                    return Err(anyhow::anyhow!(
                        "eth sub event from unexpected address: {}",
                        message.source()
                    ));
                }

                if let Ok(eth::EthSub { result, .. }) = eth_result {
                    if let eth::SubscriptionResult::Log(log) = result {
                        handle_eth_log(state, *log)?;
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
                handle_local_request(our, state, chains)?;
            }
        }
    } else {
        return Err(anyhow::anyhow!("not a request"));
    }

    Ok(())
}

fn handle_local_request(our: &Address, state: &mut State, chains: Chains) -> anyhow::Result<()> {
    match chains {
        Chains::GetApp(package_id) => {
            let onchain_app =
                state
                    .listings
                    .get(&package_id.to_process_lib())
                    .map(|app| OnChainApp {
                        metadata_uri: app.metadata_uri.clone(),
                        metadata_hash: app.metadata_hash.clone(),
                        metadata: app.metadata.as_ref().map(|m| m.clone().into()),
                    });
            let response = GetAppResponse { app: onchain_app };
            Response::new()
                .body(serde_json::to_vec(&response)?)
                .send()?;
        }
        Chains::StartAutoUpdate(package_id) => {
            state
                .listings
                .get_mut(&package_id.to_process_lib())
                .ok_or(anyhow::anyhow!("package not found"))?
                .auto_update = true;
            // send responses in these too.
        }
        Chains::StopAutoUpdate(package_id) => {
            state
                .listings
                .get_mut(&package_id.to_process_lib())
                .ok_or(anyhow::anyhow!("package not found"))?
                .auto_update = false;
        }
    }
    Ok(())
}

fn handle_eth_log(state: &mut State, log: eth::Log) -> anyhow::Result<()> {
    let block_number: u64 = log.block_number.ok_or(anyhow::anyhow!("blocknumbaerror"))?;
    let note: kimap::Note =
        kimap::decode_note_log(&log).map_err(|e| anyhow::anyhow!("decodelogerror: {e:?}"))?;

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
    //

    let metadata_uri = String::from_utf8_lossy(&note.data).to_string();

    let (tba, metadata_hash) = {
        // generate ~metadata-hash full-path
        let hash_note = format!("~metadata-hash.{}", note.parent_path);

        // owner can change which we don't track (yet?) so don't save, need to get when desired
        let (tba, _owner, data) = state.kimap.get(&hash_note).map_err(|e| {
            println!("Couldn't find {hash_note}: {e:?}");
            anyhow::anyhow!("metadata hash mismatch")
        })?;

        let Some(hash_note) = data else {
            return Err(anyhow::anyhow!("metadata not found"));
        };

        (tba, String::from_utf8_lossy(&hash_note).to_string())
    };

    // fetch metadata from the URI (currently only handling HTTP(S) URLs!)
    // assert that the metadata hash matches the fetched data
    let metadata = fetch_metadata_from_url(&metadata_uri, &metadata_hash, 30)?;

    match state.listings.entry(package_id) {
        std::collections::hash_map::Entry::Occupied(mut listing) => {
            let listing = listing.get_mut();
            listing.metadata_uri = metadata_uri;
            listing.tba = tba;
            listing.metadata_hash = metadata_hash;
            listing.metadata = Some(metadata);
        }
        std::collections::hash_map::Entry::Vacant(listing) => {
            listing.insert(PackageListing {
                tba,
                metadata_uri,
                metadata_hash,
                metadata: Some(metadata),
                auto_update: false,
            });
        }
    }
    state.last_saved_block = block_number;

    Ok(())
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
pub fn fetch_and_subscribe_logs(state: &mut State) {
    let filter = app_store_filter(state);
    // get past logs, subscribe to new ones.
    // subscribe first so we don't miss any logs
    state.kimap.provider.subscribe_loop(1, filter.clone());
    for log in fetch_logs(
        &state.kimap.provider,
        &filter.from_block(state.last_saved_block),
    ) {
        if let Err(e) = handle_eth_log(state, log) {
            print_to_terminal(1, &format!("error ingesting log: {e}"));
        };
    }
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

impl From<kt::Erc721Metadata> for OnChainMetadata {
    fn from(erc: kt::Erc721Metadata) -> Self {
        OnChainMetadata {
            name: erc.name,
            description: erc.description,
            image: erc.image,
            external_url: erc.external_url,
            animation_url: erc.animation_url,
            properties: OnChainProperties {
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
