#![feature(let_chains)]
//! App Store:
//! acts as both a local package manager and a protocol to share packages across the network.
//! packages are apps; apps are packages. we use an onchain app listing contract to determine
//! what apps are available to download and what node(s) to download them from.
//!
//! once we know that list, we can request a package from a node and download it locally.
//! (we can also manually download an "untracked" package if we know its name and distributor node)
//! packages that are downloaded can then be installed!
//!
//! installed packages can be managed:
//! - given permissions (necessary to complete install)
//! - uninstalled + deleted
//! - set to automatically update if a new version is available
use crate::kinode::process::main::{
    ApisResponse, AutoUpdateResponse, DownloadRequest, DownloadResponse, GetApiResponse,
    HashMismatch, InstallResponse, LocalRequest, LocalResponse, MirrorResponse, NewPackageRequest,
    NewPackageResponse, Reason, RebuildIndexResponse, RemoteDownloadRequest, RemoteRequest,
    RemoteResponse, UninstallResponse,
};
use ft_worker_lib::{
    spawn_receive_transfer, spawn_transfer, FTWorkerCommand, FTWorkerResult, FileTransferContext,
};
use kinode_process_lib::{
    await_message, call_init, eth, get_blob, http, kimap, println, vfs, Address, LazyLoadBlob,
    Message, PackageId, Request, Response,
};
use serde::{Deserialize, Serialize};
use state::{AppStoreLogError, PackageState, RequestedPackage, State};
use utils::{fetch_and_subscribe_logs, fetch_state};

wit_bindgen::generate!({
    path: "target/wit",
    generate_unused_types: true,
    world: "app-store-sys-v0",
    additional_derives: [serde::Deserialize, serde::Serialize],
});

mod ft_worker_lib;
mod http_api;
pub mod state;
pub mod utils;

#[cfg(not(feature = "simulation-mode"))]
const CHAIN_ID: u64 = kimap::KIMAP_CHAIN_ID;
#[cfg(feature = "simulation-mode")]
const CHAIN_ID: u64 = 31337; // local

const CHAIN_TIMEOUT: u64 = 60; // 60s
pub const VFS_TIMEOUT: u64 = 5; // 5s
pub const APP_SHARE_TIMEOUT: u64 = 120; // 120s

#[cfg(not(feature = "simulation-mode"))]
const KIMAP_ADDRESS: &str = kimap::KIMAP_ADDRESS;
#[cfg(feature = "simulation-mode")]
const KIMAP_ADDRESS: &str = "0x0165878A594ca255338adfa4d48449f69242Eb8F"; // note temp kimap address!

#[cfg(not(feature = "simulation-mode"))]
const KIMAP_FIRST_BLOCK: u64 = kimap::KIMAP_FIRST_BLOCK;
#[cfg(feature = "simulation-mode")]
const KIMAP_FIRST_BLOCK: u64 = 1;

// internal types

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)] // untagged as a meta-type for all incoming requests
pub enum Req {
    LocalRequest(LocalRequest),
    RemoteRequest(RemoteRequest),
    FTWorkerCommand(FTWorkerCommand),
    FTWorkerResult(FTWorkerResult),
    Eth(eth::EthSubResult),
    Http(http::HttpServerRequest),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)] // untagged as a meta-type for all incoming responses
pub enum Resp {
    LocalResponse(LocalResponse),
    RemoteResponse(RemoteResponse),
    FTWorkerResult(FTWorkerResult),
    HttpClient(Result<http::HttpClientResponse, http::HttpClientError>),
}

call_init!(init);
fn init(our: Address) {
    println!("started");

    http_api::init_frontend(&our);

    println!("indexing on contract address {}", KIMAP_ADDRESS);

    // create new provider with request-timeout of 60s
    // can change, log requests can take quite a long time.
    let eth_provider = eth::Provider::new(CHAIN_ID, CHAIN_TIMEOUT);

    let mut state = fetch_state(our, eth_provider);
    fetch_and_subscribe_logs(&mut state);

    loop {
        match await_message() {
            Err(send_error) => {
                // TODO handle these based on what they are triggered by
                println!("got network error: {send_error}");
            }
            Ok(message) => {
                if let Err(e) = handle_message(&mut state, &message) {
                    println!("error handling message: {:?}", e);
                }
            }
        }
    }
}

/// message router: parse into our Req and Resp types, then pass to
/// function defined for each kind of message. check whether the source
/// of the message is allowed to send that kind of message to us.
/// finally, fire a response if expected from a request.
fn handle_message(state: &mut State, message: &Message) -> anyhow::Result<()> {
    if message.is_request() {
        match serde_json::from_slice::<Req>(message.body())? {
            Req::LocalRequest(local_request) => {
                if !message.is_local(&state.our) {
                    return Err(anyhow::anyhow!("local request from non-local node"));
                }
                let (body, blob) = handle_local_request(state, local_request);
                let response = Response::new().body(serde_json::to_vec(&body)?);
                if let Some(blob) = blob {
                    response.blob(blob).send()?;
                } else {
                    response.send()?;
                }
            }
            Req::RemoteRequest(remote_request) => {
                let resp = handle_remote_request(state, message.source(), remote_request);
                Response::new().body(serde_json::to_vec(&resp)?).send()?;
            }
            Req::FTWorkerResult(FTWorkerResult::ReceiveSuccess(name)) => {
                handle_receive_download(state, &name)?;
            }
            Req::FTWorkerCommand(_) => {
                spawn_receive_transfer(&state.our, message.body())?;
            }
            Req::FTWorkerResult(r) => {
                println!("got weird ft_worker result: {r:?}");
            }
            Req::Eth(eth_result) => {
                if !message.is_local(&state.our) || message.source().process != "eth:distro:sys" {
                    return Err(anyhow::anyhow!(
                        "eth sub event from weird addr: {}",
                        message.source()
                    ));
                }
                if let Ok(eth::EthSub { result, .. }) = eth_result {
                    handle_eth_sub_event(state, result)?;
                } else {
                    // attempt to resubscribe
                    state
                        .kimap
                        .provider
                        .subscribe_loop(1, utils::app_store_filter(state));
                }
            }
            Req::Http(incoming) => {
                if !message.is_local(&state.our)
                    || message.source().process != "http_server:distro:sys"
                {
                    return Err(anyhow::anyhow!("http_server from non-local node"));
                }
                if let http::HttpServerRequest::Http(req) = incoming {
                    http_api::handle_http_request(state, &req)?;
                }
            }
        }
    } else {
        match serde_json::from_slice::<Resp>(message.body())? {
            Resp::HttpClient(resp) => {
                let name = match message.context() {
                    Some(context) => std::str::from_utf8(context).unwrap_or_default(),
                    None => return Err(anyhow::anyhow!("http_client response without context")),
                };
                if let Ok(http::HttpClientResponse::Http(http::HttpResponse { status, .. })) = resp
                {
                    if status == 200 {
                        handle_receive_download(state, &name)?;
                    }
                } else {
                    println!("got http_client error: {resp:?}");
                }
            }
            Resp::FTWorkerResult(ft_worker_result) => {
                handle_ft_worker_result(ft_worker_result, message.context().unwrap_or(&vec![]))?;
            }
            Resp::LocalResponse(_) | Resp::RemoteResponse(_) => {
                // don't need to handle these at the moment
            }
        }
    }
    Ok(())
}

/// fielding requests to download packages and APIs from us
fn handle_remote_request(state: &mut State, source: &Address, request: RemoteRequest) -> Resp {
    let (package_id, desired_version_hash) = match request {
        RemoteRequest::Download(RemoteDownloadRequest {
            package_id,
            desired_version_hash,
        }) => (package_id.to_process_lib(), desired_version_hash),
    };

    let Some(listing) = state.packages.get(&package_id) else {
        return Resp::RemoteResponse(RemoteResponse::DownloadDenied(Reason::NoPackage));
    };

    let Some(ref package_state) = listing.state else {
        return Resp::RemoteResponse(RemoteResponse::DownloadDenied(Reason::NoPackage));
    };

    if !package_state.mirroring {
        return Resp::RemoteResponse(RemoteResponse::DownloadDenied(Reason::NotMirroring));
    }

    if let Some(hash) = desired_version_hash {
        if package_state.our_version_hash != hash {
            return Resp::RemoteResponse(RemoteResponse::DownloadDenied(Reason::HashMismatch(
                HashMismatch {
                    requested: hash,
                    have: package_state.our_version_hash.clone(),
                },
            )));
        }
    }

    let file_name = format!("/{package_id}.zip");

    // get the .zip from VFS and attach as blob to response
    let Ok(Ok(_)) = utils::vfs_request(
        format!("/{package_id}/pkg{file_name}"),
        vfs::VfsAction::Read,
    )
    .send_and_await_response(VFS_TIMEOUT) else {
        return Resp::RemoteResponse(RemoteResponse::DownloadDenied(Reason::FileNotFound));
    };

    // transfer will *inherit* the blob bytes we receive from VFS
    match spawn_transfer(&state.our, &file_name, None, APP_SHARE_TIMEOUT, &source) {
        Ok(()) => Resp::RemoteResponse(RemoteResponse::DownloadApproved),
        Err(_e) => Resp::RemoteResponse(RemoteResponse::DownloadDenied(Reason::WorkerSpawnFailed)),
    }
}

/// only `our.node` can call this
fn handle_local_request(
    state: &mut State,
    request: LocalRequest,
) -> (LocalResponse, Option<LazyLoadBlob>) {
    match request {
        LocalRequest::NewPackage(NewPackageRequest {
            package_id,
            metadata,
            mirror,
        }) => {
            let Some(blob) = get_blob() else {
                return (
                    LocalResponse::NewPackageResponse(NewPackageResponse::NoBlob),
                    None,
                );
            };
            (
                match utils::new_package(
                    &package_id.to_process_lib(),
                    state,
                    metadata.to_erc721_metadata(),
                    mirror,
                    blob.bytes,
                ) {
                    Ok(()) => LocalResponse::NewPackageResponse(NewPackageResponse::Success),
                    Err(_) => LocalResponse::NewPackageResponse(NewPackageResponse::InstallFailed),
                },
                None,
            )
        }
        LocalRequest::Download(DownloadRequest {
            package_id,
            download_from,
            mirror,
            auto_update,
            desired_version_hash,
        }) => (
            LocalResponse::DownloadResponse(start_download(
                state,
                package_id.to_process_lib(),
                download_from,
                mirror,
                auto_update,
                desired_version_hash,
            )),
            None,
        ),
        LocalRequest::Install(package_id) => (
            match handle_install(state, &package_id.to_process_lib()) {
                Ok(()) => LocalResponse::InstallResponse(InstallResponse::Success),
                Err(e) => {
                    println!("error installing package: {e}");
                    LocalResponse::InstallResponse(InstallResponse::Failure)
                }
            },
            None,
        ),
        LocalRequest::Uninstall(package_id) => (
            match state.uninstall(&package_id.to_process_lib()) {
                Ok(()) => LocalResponse::UninstallResponse(UninstallResponse::Success),
                Err(_) => LocalResponse::UninstallResponse(UninstallResponse::Failure),
            },
            None,
        ),
        LocalRequest::StartMirroring(package_id) => (
            match state.start_mirroring(&package_id.to_process_lib()) {
                true => LocalResponse::MirrorResponse(MirrorResponse::Success),
                false => LocalResponse::MirrorResponse(MirrorResponse::Failure),
            },
            None,
        ),
        LocalRequest::StopMirroring(package_id) => (
            match state.stop_mirroring(&package_id.to_process_lib()) {
                true => LocalResponse::MirrorResponse(MirrorResponse::Success),
                false => LocalResponse::MirrorResponse(MirrorResponse::Failure),
            },
            None,
        ),
        LocalRequest::StartAutoUpdate(package_id) => (
            match state.start_auto_update(&package_id.to_process_lib()) {
                true => LocalResponse::AutoUpdateResponse(AutoUpdateResponse::Success),
                false => LocalResponse::AutoUpdateResponse(AutoUpdateResponse::Failure),
            },
            None,
        ),
        LocalRequest::StopAutoUpdate(package_id) => (
            match state.stop_auto_update(&package_id.to_process_lib()) {
                true => LocalResponse::AutoUpdateResponse(AutoUpdateResponse::Success),
                false => LocalResponse::AutoUpdateResponse(AutoUpdateResponse::Failure),
            },
            None,
        ),
        LocalRequest::RebuildIndex => (rebuild_index(state), None),
        LocalRequest::Apis => (list_apis(state), None),
        LocalRequest::GetApi(package_id) => get_api(state, &package_id.to_process_lib()),
    }
}

pub fn get_api(state: &mut State, package_id: &PackageId) -> (LocalResponse, Option<LazyLoadBlob>) {
    if !state.downloaded_apis.contains(package_id) {
        return (LocalResponse::GetApiResponse(GetApiResponse::Failure), None);
    }
    let Ok(Ok(_)) = utils::vfs_request(format!("/{package_id}/pkg/api.zip"), vfs::VfsAction::Read)
        .send_and_await_response(VFS_TIMEOUT)
    else {
        return (LocalResponse::GetApiResponse(GetApiResponse::Failure), None);
    };
    let Some(blob) = get_blob() else {
        return (LocalResponse::GetApiResponse(GetApiResponse::Failure), None);
    };
    (
        LocalResponse::GetApiResponse(GetApiResponse::Success),
        Some(LazyLoadBlob {
            mime: Some("application/json".to_string()),
            bytes: blob.bytes,
        }),
    )
}

pub fn list_apis(state: &mut State) -> LocalResponse {
    LocalResponse::ApisResponse(ApisResponse {
        apis: state
            .downloaded_apis
            .clone()
            .into_iter()
            .map(|id| crate::kinode::process::main::PackageId::from_process_lib(id))
            .collect(),
    })
}

pub fn rebuild_index(state: &mut State) -> LocalResponse {
    // kill our old subscription and build a new one.
    let _ = state.kimap.provider.unsubscribe(1);

    let eth_provider = eth::Provider::new(CHAIN_ID, CHAIN_TIMEOUT);
    *state = State::new(state.our.clone(), eth_provider).expect("state creation failed");

    fetch_and_subscribe_logs(state);
    LocalResponse::RebuildIndexResponse(RebuildIndexResponse::Success)
}

/// `from`: the node OR url to download from
pub fn start_download(
    state: &mut State,
    package_id: PackageId,
    from: String,
    mirror: bool,
    auto_update: bool,
    desired_version_hash: Option<String>,
) -> DownloadResponse {
    let download_request = RemoteDownloadRequest {
        package_id: crate::kinode::process::main::PackageId::from_process_lib(package_id.clone()),
        desired_version_hash: desired_version_hash.clone(),
    };
    // if `from` is a node, send a request to it
    // but if it is a url, use http_client to GET it
    if from.starts_with("http") {
        // use http_client to GET it
        Request::to(("our", "http_client", "distro", "sys"))
            .body(
                serde_json::to_vec(&http::HttpClientAction::Http(http::OutgoingHttpRequest {
                    method: "GET".to_string(),
                    version: None,
                    url: from.clone(),
                    headers: std::collections::HashMap::new(),
                }))
                .unwrap(),
            )
            .context(package_id.to_string().as_bytes())
            .expects_response(60)
            .send()
            .unwrap();

        return DownloadResponse::Started;
    } else {
        if let Ok(Ok(Message::Response { body, .. })) =
            Request::to((from.as_str(), state.our.process.clone()))
                .body(serde_json::to_vec(&RemoteRequest::Download(download_request)).unwrap())
                .send_and_await_response(VFS_TIMEOUT)
        {
            if let Ok(Resp::RemoteResponse(RemoteResponse::DownloadApproved)) =
                serde_json::from_slice::<Resp>(&body)
            {
                state.requested_packages.insert(
                    package_id,
                    RequestedPackage {
                        from,
                        mirror,
                        auto_update,
                        desired_version_hash,
                    },
                );
                return DownloadResponse::Started;
            }
        }
    }
    DownloadResponse::BadResponse
}

fn handle_receive_download(state: &mut State, package_name: &str) -> anyhow::Result<()> {
    // remove leading / and .zip from file name to get package ID
    let package_name = package_name
        .trim_start_matches("/")
        .trim_end_matches(".zip");
    let Ok(package_id) = package_name.parse::<PackageId>() else {
        return Err(anyhow::anyhow!(
            "bad package ID from download: {package_name}"
        ));
    };
    handle_receive_download_package(state, &package_id)
}

fn handle_receive_download_package(
    state: &mut State,
    package_id: &PackageId,
) -> anyhow::Result<()> {
    println!("successfully received {}", package_id);
    // only save the package if we actually requested it
    let Some(requested_package) = state.requested_packages.remove(package_id) else {
        return Err(anyhow::anyhow!("received unrequested package--rejecting!"));
    };
    let Some(blob) = get_blob() else {
        return Err(anyhow::anyhow!("received download but found no blob"));
    };
    // check the version hash for this download against requested!
    let download_hash = utils::sha_256_hash(&blob.bytes);
    let verified = match requested_package.desired_version_hash {
        Some(hash) => {
            if download_hash != hash {
                return Err(anyhow::anyhow!(
                    "downloaded package is not desired version--rejecting download! \
                    download hash: {download_hash}, desired hash: {hash}"
                ));
            }
            true
        }
        None => match state.packages.get(package_id) {
            None => {
                println!("downloaded package cannot be found onchain, proceeding with unverified download");
                false
            }
            Some(package_listing) => {
                if let Some(metadata) = &package_listing.metadata {
                    let latest_hash = metadata
                        .properties
                        .code_hashes
                        .get(&metadata.properties.current_version);
                    if Some(&download_hash) != latest_hash {
                        println!(
                            "downloaded package is not latest version \
                            download hash: {download_hash}, latest hash: {latest_hash:?} \
                            proceeding with unverified download"
                        );
                    }
                    false
                } else {
                    println!("downloaded package has no metadata to check validity against, proceeding with unverified download");
                    false
                }
            }
        },
    };

    let old_manifest_hash = match state.packages.get(package_id) {
        Some(listing) => listing
            .state
            .as_ref()
            .and_then(|state| state.manifest_hash.clone())
            .unwrap_or("OLD".to_string()),
        _ => "OLD".to_string(),
    };

    state.add_downloaded_package(
        package_id,
        PackageState {
            mirrored_from: Some(requested_package.from),
            our_version_hash: download_hash,
            installed: false,
            verified,
            caps_approved: false,
            manifest_hash: None, // generated in the add fn
            mirroring: requested_package.mirror,
            auto_update: requested_package.auto_update,
        },
        Some(blob.bytes),
    )?;

    let new_manifest_hash = match state.packages.get(package_id) {
        Some(listing) => listing
            .state
            .as_ref()
            .and_then(|state| state.manifest_hash.clone())
            .unwrap_or("NEW".to_string()),
        _ => "NEW".to_string(),
    };

    // lastly, if auto_update is true, AND the manifest has NOT changed,
    // trigger install!
    if requested_package.auto_update && old_manifest_hash == new_manifest_hash {
        handle_install(state, package_id)?;
    }
    Ok(())
}

fn handle_ft_worker_result(ft_worker_result: FTWorkerResult, context: &[u8]) -> anyhow::Result<()> {
    let context = serde_json::from_slice::<FileTransferContext>(context)?;
    if let FTWorkerResult::SendSuccess = ft_worker_result {
        println!(
            "successfully shared {} in {:.4}s",
            context.file_name,
            std::time::SystemTime::now()
                .duration_since(context.start_time)
                .unwrap()
                .as_secs_f64(),
        );
        Ok(())
    } else {
        Err(anyhow::anyhow!("failed to share app"))
    }
}

fn handle_eth_sub_event(
    state: &mut State,
    event: eth::SubscriptionResult,
) -> Result<(), AppStoreLogError> {
    let eth::SubscriptionResult::Log(log) = event else {
        return Err(AppStoreLogError::DecodeLogError);
    };
    state.ingest_contract_event(*log, true)
}

/// the steps to take an existing package on disk and install/start it
/// make sure you have reviewed and approved caps in manifest before calling this
pub fn handle_install(state: &mut State, package_id: &PackageId) -> anyhow::Result<()> {
    // wit version will default to the latest if not specified
    let metadata = &state
        .packages
        .get(package_id)
        .ok_or_else(|| anyhow::anyhow!("package not found in manager"))?
        .metadata;

    let wit_version = match metadata {
        Some(metadata) => metadata.properties.wit_version,
        None => Some(0),
    };

    utils::install(package_id, &state.our.node, wit_version)?;

    // finally set the package as installed
    state.update_downloaded_package(package_id, |package_state| {
        package_state.installed = true;
    });
    Ok(())
}
