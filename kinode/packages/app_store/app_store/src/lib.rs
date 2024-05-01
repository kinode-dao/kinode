use kinode_process_lib::http::{
    bind_http_path, bind_ws_path, send_ws_push, serve_ui, HttpServerRequest, WsMessageType,
};
use kinode_process_lib::kernel_types as kt;
use kinode_process_lib::*;
use kinode_process_lib::{call_init, println};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

wit_bindgen::generate!({
    path: "wit",
    world: "process",
});

mod api;
mod http_api;
use api::*;
mod types;
use types::*;
mod ft_worker_lib;
use ft_worker_lib::{
    spawn_receive_transfer, spawn_transfer, FTWorkerCommand, FTWorkerResult, FileTransferContext,
};

/// App Store:
/// acts as both a local package manager and a protocol to share packages across the network.
/// packages are apps; apps are packages. we use an onchain app listing contract to determine
/// what apps are available to download and what node(s) to download them from.
///
/// once we know that list, we can request a package from a node and download it locally.
/// (we can also manually download an "untracked" package if we know its name and distributor node)
/// packages that are downloaded can then be installed!
///
/// installed packages can be managed:
/// - given permissions (necessary to complete install)
/// - uninstalled + deleted
/// - set to automatically update if a new version is available

const ICON: &str = include_str!("icon");

const CHAIN_ID: u64 = 10; // optimism
const CONTRACT_ADDRESS: &str = "0x52185B6a6017E6f079B994452F234f7C2533787B"; // optimism
const CONTRACT_FIRST_BLOCK: u64 = 118_590_088;

const EVENTS: [&str; 3] = [
    "AppRegistered(uint256,string,bytes,string,bytes32)",
    "AppMetadataUpdated(uint256,string,bytes32)",
    "Transfer(address,address,uint256)",
];

// internal types

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)] // untagged as a meta-type for all incoming requests
pub enum Req {
    LocalRequest(LocalRequest),
    RemoteRequest(RemoteRequest),
    FTWorkerCommand(FTWorkerCommand),
    FTWorkerResult(FTWorkerResult),
    Eth(eth::EthSubResult),
    Http(HttpServerRequest),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)] // untagged as a meta-type for all incoming responses
pub enum Resp {
    LocalResponse(LocalResponse),
    RemoteResponse(RemoteResponse),
    FTWorkerResult(FTWorkerResult),
}

fn fetch_logs(eth_provider: &eth::Provider, filter: &eth::Filter) -> Vec<eth::Log> {
    #[cfg(not(feature = "simulation-mode"))]
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
    #[cfg(feature = "simulation-mode")] // TODO use local testnet, provider_chainId: 31337
    vec![]
}

#[allow(unused_variables)]
fn subscribe_to_logs(eth_provider: &eth::Provider, filter: eth::Filter) {
    #[cfg(not(feature = "simulation-mode"))]
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
    #[cfg(not(feature = "simulation-mode"))]
    println!("subscribed to logs successfully");
}

call_init!(init);
fn init(our: Address) {
    println!("started");

    for path in [
        "/apps",
        "/apps/listed",
        "/apps/:id",
        "/apps/listed/:id",
        "/apps/:id/caps",
        "/apps/:id/mirror",
        "/apps/:id/auto-update",
        "/apps/rebuild-index",
    ] {
        bind_http_path(path, true, false).expect("failed to bind http path");
    }
    serve_ui(
        &our,
        "ui",
        true,
        false,
        vec!["/", "/my-apps", "/app-details/:id", "/publish"],
    )
    .expect("failed to serve static UI");

    bind_ws_path("/", true, true).expect("failed to bind ws path");

    // add ourselves to the homepage
    Request::to(("our", "homepage", "homepage", "sys"))
        .body(
            serde_json::json!({
                "Add": {
                    "label": "App Store",
                    "icon": ICON,
                    "path": "/" // just our root
                }
            })
            .to_string()
            .as_bytes()
            .to_vec(),
        )
        .send()
        .unwrap();

    // load in our saved state or initalize a new one if none exists
    let mut state = get_typed_state(|bytes| Ok(bincode::deserialize(bytes)?))
        .unwrap_or(State::new(CONTRACT_ADDRESS.to_string()).unwrap());

    if state.contract_address != CONTRACT_ADDRESS {
        println!("warning: contract address mismatch--overwriting saved state");
        state = State::new(CONTRACT_ADDRESS.to_string()).unwrap();
    }

    println!("indexing on contract address {}", state.contract_address);

    // create new provider for sepolia with request-timeout of 60s
    // can change, log requests can take quite a long time.
    let eth_provider = eth::Provider::new(CHAIN_ID, 60);

    let mut requested_packages: HashMap<PackageId, RequestedPackage> = HashMap::new();

    // get past logs, subscribe to new ones.
    let filter = eth::Filter::new()
        .address(eth::Address::from_str(&state.contract_address).unwrap())
        .from_block(state.last_saved_block - 1)
        .to_block(eth::BlockNumberOrTag::Latest)
        .events(EVENTS);

    for log in fetch_logs(&eth_provider, &filter) {
        if let Err(e) = state.ingest_listings_contract_event(&our, log) {
            println!("error ingesting log: {e:?}");
        };
    }
    subscribe_to_logs(&eth_provider, filter);

    // websocket channel to send errors/updates to UI
    let channel_id: u32 = 154869;

    loop {
        match await_message() {
            Err(send_error) => {
                // TODO handle these based on what they are triggered by
                println!("got network error: {send_error}");
            }
            Ok(message) => {
                if let Err(e) = handle_message(
                    &our,
                    &mut state,
                    &eth_provider,
                    &mut requested_packages,
                    &message,
                ) {
                    println!("error handling message: {:?}", e);
                    send_ws_push(
                        channel_id,
                        WsMessageType::Text,
                        LazyLoadBlob {
                            mime: Some("application/json".to_string()),
                            bytes: serde_json::json!({
                                "kind": "error",
                                "data": e.to_string(),
                            })
                            .to_string()
                            .as_bytes()
                            .to_vec(),
                        },
                    )
                }
            }
        }
    }
}

/// message router: parse into our Req and Resp types, then pass to
/// function defined for each kind of message. check whether the source
/// of the message is allowed to send that kind of message to us.
/// finally, fire a response if expected from a request.
fn handle_message(
    our: &Address,
    mut state: &mut State,
    eth_provider: &eth::Provider,
    mut requested_packages: &mut HashMap<PackageId, RequestedPackage>,
    message: &Message,
) -> anyhow::Result<()> {
    match message {
        Message::Request {
            source,
            expects_response,
            body,
            ..
        } => match serde_json::from_slice::<Req>(&body)? {
            Req::LocalRequest(local_request) => {
                if our.node != source.node {
                    return Err(anyhow::anyhow!("local request from non-local node"));
                }
                let resp = handle_local_request(
                    &our,
                    &local_request,
                    &mut state,
                    eth_provider,
                    &mut requested_packages,
                );
                if expects_response.is_some() {
                    Response::new().body(serde_json::to_vec(&resp)?).send()?;
                }
            }
            Req::RemoteRequest(remote_request) => {
                let resp = handle_remote_request(&our, &source, &remote_request, &mut state);
                if expects_response.is_some() {
                    Response::new().body(serde_json::to_vec(&resp)?).send()?;
                }
            }
            Req::FTWorkerResult(FTWorkerResult::ReceiveSuccess(name)) => {
                handle_receive_download(&our, &mut state, &name, &mut requested_packages)?;
            }
            Req::FTWorkerCommand(_) => {
                spawn_receive_transfer(&our, &body)?;
            }
            Req::FTWorkerResult(r) => {
                println!("got weird ft_worker result: {r:?}");
            }
            Req::Eth(eth_result) => {
                if source.node() != our.node() || source.process != "eth:distro:sys" {
                    return Err(anyhow::anyhow!("eth sub event from weird addr: {source}"));
                }
                if let Ok(eth::EthSub { result, .. }) = eth_result {
                    handle_eth_sub_event(our, &mut state, result)?;
                } else {
                    println!("got eth subscription error");
                    // attempt to resubscribe
                    subscribe_to_logs(
                        &eth_provider,
                        eth::Filter::new()
                            .address(eth::Address::from_str(&state.contract_address).unwrap())
                            .from_block(state.last_saved_block - 1)
                            .to_block(eth::BlockNumberOrTag::Latest)
                            .events(EVENTS),
                    );
                }
            }
            Req::Http(incoming) => {
                if source.node() != our.node()
                    || &source.process.to_string() != "http_server:distro:sys"
                {
                    return Err(anyhow::anyhow!("http_server from non-local node"));
                }
                if let HttpServerRequest::Http(req) = incoming {
                    http_api::handle_http_request(
                        our,
                        &mut state,
                        eth_provider,
                        requested_packages,
                        &req,
                    )?;
                }
            }
        },
        Message::Response { body, context, .. } => {
            // the only kind of response we care to handle here!
            let Some(context) = context else {
                return Err(anyhow::anyhow!("missing context"));
            };
            handle_ft_worker_result(body, context)?;
        }
    }
    Ok(())
}

/// so far just fielding requests to download packages from us
fn handle_remote_request(
    our: &Address,
    source: &Address,
    request: &RemoteRequest,
    state: &mut State,
) -> Resp {
    match request {
        RemoteRequest::Download {
            package_id,
            desired_version_hash,
        } => {
            let Some(package_state) = state.get_downloaded_package(package_id) else {
                return Resp::RemoteResponse(RemoteResponse::DownloadDenied(
                    ReasonDenied::NoPackage,
                ));
            };
            if !package_state.mirroring {
                return Resp::RemoteResponse(RemoteResponse::DownloadDenied(
                    ReasonDenied::NotMirroring,
                ));
            }
            if let Some(hash) = desired_version_hash {
                if &package_state.our_version != hash {
                    return Resp::RemoteResponse(RemoteResponse::DownloadDenied(
                        ReasonDenied::HashMismatch {
                            requested: hash.clone(),
                            have: package_state.our_version.clone(),
                        },
                    ));
                }
            }
            let file_name = format!("/{}.zip", package_id);
            // get the .zip from VFS and attach as blob to response
            let file_path = format!("/{}/pkg/{}.zip", package_id, package_id);
            let Ok(Ok(_)) = Request::to(("our", "vfs", "distro", "sys"))
                .body(
                    serde_json::to_vec(&vfs::VfsRequest {
                        path: file_path,
                        action: vfs::VfsAction::Read,
                    })
                    .unwrap(),
                )
                .send_and_await_response(5)
            else {
                return Resp::RemoteResponse(RemoteResponse::DownloadDenied(
                    ReasonDenied::FileNotFound,
                ));
            };
            // transfer will *inherit* the blob bytes we receive from VFS
            match spawn_transfer(&our, &file_name, None, 60, &source) {
                Ok(()) => Resp::RemoteResponse(RemoteResponse::DownloadApproved),
                Err(_e) => Resp::RemoteResponse(RemoteResponse::DownloadDenied(
                    ReasonDenied::WorkerSpawnFailed,
                )),
            }
        }
    }
}

/// only `our.node` can call this
fn handle_local_request(
    our: &Address,
    request: &LocalRequest,
    state: &mut State,
    eth_provider: &eth::Provider,
    requested_packages: &mut HashMap<PackageId, RequestedPackage>,
) -> LocalResponse {
    match request {
        LocalRequest::NewPackage { package, mirror } => {
            let Some(blob) = get_blob() else {
                return LocalResponse::NewPackageResponse(NewPackageResponse::Failure);
            };
            // set the version hash for this new local package
            let our_version = generate_version_hash(&blob.bytes);

            let package_state = PackageState {
                mirrored_from: Some(our.node.clone()),
                our_version,
                installed: false,
                verified: true, // side loaded apps are implicitly verified because there is no "source" to verify against
                caps_approved: true, // TODO see if we want to auto-approve local installs
                manifest_hash: None, // generated in the add fn
                mirroring: *mirror,
                auto_update: false, // can't auto-update a local package
                metadata: None,     // TODO
            };
            let Ok(()) = state.add_downloaded_package(package, package_state, Some(blob.bytes))
            else {
                return LocalResponse::NewPackageResponse(NewPackageResponse::Failure);
            };
            LocalResponse::NewPackageResponse(NewPackageResponse::Success)
        }
        LocalRequest::Download {
            package: package_id,
            download_from,
            mirror,
            auto_update,
            desired_version_hash,
        } => LocalResponse::DownloadResponse(start_download(
            our,
            requested_packages,
            package_id,
            download_from,
            *mirror,
            *auto_update,
            desired_version_hash,
        )),
        LocalRequest::Install(package) => match handle_install(our, state, package) {
            Ok(()) => LocalResponse::InstallResponse(InstallResponse::Success),
            Err(_) => LocalResponse::InstallResponse(InstallResponse::Failure),
        },
        LocalRequest::Uninstall(package) => match state.uninstall(package) {
            Ok(()) => LocalResponse::UninstallResponse(UninstallResponse::Success),
            Err(_) => LocalResponse::UninstallResponse(UninstallResponse::Failure),
        },
        LocalRequest::StartMirroring(package) => match state.start_mirroring(package) {
            true => LocalResponse::MirrorResponse(MirrorResponse::Success),
            false => LocalResponse::MirrorResponse(MirrorResponse::Failure),
        },
        LocalRequest::StopMirroring(package) => match state.stop_mirroring(package) {
            true => LocalResponse::MirrorResponse(MirrorResponse::Success),
            false => LocalResponse::MirrorResponse(MirrorResponse::Failure),
        },
        LocalRequest::StartAutoUpdate(package) => match state.start_auto_update(package) {
            true => LocalResponse::AutoUpdateResponse(AutoUpdateResponse::Success),
            false => LocalResponse::AutoUpdateResponse(AutoUpdateResponse::Failure),
        },
        LocalRequest::StopAutoUpdate(package) => match state.stop_auto_update(package) {
            true => LocalResponse::AutoUpdateResponse(AutoUpdateResponse::Success),
            false => LocalResponse::AutoUpdateResponse(AutoUpdateResponse::Failure),
        },
        LocalRequest::RebuildIndex => rebuild_index(our, state, eth_provider),
    }
}

pub fn rebuild_index(
    our: &Address,
    state: &mut State,
    eth_provider: &eth::Provider,
) -> LocalResponse {
    *state = State::new(CONTRACT_ADDRESS.to_string()).unwrap();
    // kill our old subscription and build a new one.
    eth_provider
        .unsubscribe(1)
        .expect("app_store: failed to unsub from eth events!");

    let filter = eth::Filter::new()
        .address(eth::Address::from_str(&state.contract_address).unwrap())
        .from_block(state.last_saved_block - 1)
        .events(EVENTS);

    for log in fetch_logs(&eth_provider, &filter) {
        if let Err(e) = state.ingest_listings_contract_event(our, log) {
            println!("error ingesting log: {e:?}");
        };
    }
    subscribe_to_logs(&eth_provider, filter);
    LocalResponse::RebuiltIndex
}

pub fn start_download(
    our: &Address,
    requested_packages: &mut HashMap<PackageId, RequestedPackage>,
    package_id: &PackageId,
    download_from: &NodeId,
    mirror: bool,
    auto_update: bool,
    desired_version_hash: &Option<String>,
) -> DownloadResponse {
    match Request::to((download_from.as_str(), our.process.clone()))
        .inherit(true)
        .body(
            serde_json::to_vec(&RemoteRequest::Download {
                package_id: package_id.clone(),
                desired_version_hash: desired_version_hash.clone(),
            })
            .unwrap(),
        )
        .send_and_await_response(5)
    {
        Ok(Ok(Message::Response { body, .. })) => match serde_json::from_slice::<Resp>(&body) {
            Ok(Resp::RemoteResponse(RemoteResponse::DownloadApproved)) => {
                requested_packages.insert(
                    package_id.clone(),
                    RequestedPackage {
                        from: download_from.clone(),
                        mirror,
                        auto_update,
                        desired_version_hash: desired_version_hash.clone(),
                    },
                );
                DownloadResponse::Started
            }
            _ => DownloadResponse::Failure,
        },
        _ => DownloadResponse::Failure,
    }
}

fn handle_receive_download(
    our: &Address,
    state: &mut State,
    package_name: &str,
    requested_packages: &mut HashMap<PackageId, RequestedPackage>,
) -> anyhow::Result<()> {
    // remove leading / and .zip from file name to get package ID
    let package_name = package_name[1..].trim_end_matches(".zip");
    let Ok(package_id) = package_name.parse::<PackageId>() else {
        return Err(anyhow::anyhow!(
            "bad package filename fron download: {package_name}"
        ));
    };
    println!("successfully received {}", package_id);
    // only save the package if we actually requested it
    let Some(requested_package) = requested_packages.remove(&package_id) else {
        return Err(anyhow::anyhow!("received unrequested package--rejecting!"));
    };
    let Some(blob) = get_blob() else {
        return Err(anyhow::anyhow!("received download but found no blob"));
    };
    // check the version hash for this download against requested!!
    // for now we can reject if it's not latest.
    let download_hash = generate_version_hash(&blob.bytes);
    let mut verified = false;
    match requested_package.desired_version_hash {
        Some(hash) => {
            if download_hash != hash {
                if hash.is_empty() {
                    println!(
                        "\x1b[33mwarning: downloaded package has no version hashes--cannot verify code integrity, proceeding anyways\x1b[0m"
                    );
                } else {
                    return Err(anyhow::anyhow!(
                        "downloaded package is not desired version--rejecting download! download hash: {download_hash}, desired hash: {hash}"
                    ));
                }
            } else {
                verified = true;
            }
        }
        None => {
            // check against `metadata.properties.current_version`
            let Some(package_listing) = state.get_listing(&package_id) else {
                return Err(anyhow::anyhow!(
                    "downloaded package cannot be found in manager--rejecting download!"
                ));
            };
            let Some(metadata) = &package_listing.metadata else {
                return Err(anyhow::anyhow!(
                    "downloaded package has no metadata to check validity against!"
                ));
            };
            let Some(latest_hash) = metadata
                .properties
                .code_hashes
                .get(&metadata.properties.current_version)
            else {
                return Err(anyhow::anyhow!(
                    "downloaded package has no versions in manager--rejecting download!"
                ));
            };
            if &download_hash != latest_hash {
                if latest_hash.is_empty() {
                    println!(
                        "\x1b[33mwarning: downloaded package has no version hashes--cannot verify code integrity, proceeding anyways\x1b[0m"
                    );
                } else {
                    return Err(anyhow::anyhow!(
                        "downloaded package is not latest version--rejecting download! download hash: {download_hash}, latest hash: {latest_hash}"
                    ));
                }
            } else {
                verified = true;
            }
        }
    }

    let old_manifest_hash = match state.downloaded_packages.get(&package_id) {
        Some(package_state) => package_state
            .manifest_hash
            .clone()
            .unwrap_or("OLD".to_string()),
        _ => "OLD".to_string(),
    };

    state.add_downloaded_package(
        &package_id,
        PackageState {
            mirrored_from: Some(requested_package.from),
            our_version: download_hash,
            installed: false,
            verified,
            caps_approved: false,
            manifest_hash: None, // generated in the add fn
            mirroring: requested_package.mirror,
            auto_update: requested_package.auto_update,
            metadata: None, // TODO
        },
        Some(blob.bytes),
    )?;

    let new_manifest_hash = match state.downloaded_packages.get(&package_id) {
        Some(package_state) => package_state
            .manifest_hash
            .clone()
            .unwrap_or("NEW".to_string()),
        _ => "NEW".to_string(),
    };

    // lastly, if auto_update is true, AND the caps_hash has NOT changed,
    // trigger install!
    if requested_package.auto_update && old_manifest_hash == new_manifest_hash {
        handle_install(our, state, &package_id)?;
    }
    Ok(())
}

fn handle_ft_worker_result(body: &[u8], context: &[u8]) -> anyhow::Result<()> {
    if let Ok(Resp::FTWorkerResult(ft_worker_result)) = serde_json::from_slice::<Resp>(body) {
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
        } else {
            return Err(anyhow::anyhow!("failed to share app"));
        }
    }
    Ok(())
}

fn handle_eth_sub_event(
    our: &Address,
    state: &mut State,
    event: eth::SubscriptionResult,
) -> anyhow::Result<()> {
    let eth::SubscriptionResult::Log(log) = event else {
        return Err(anyhow::anyhow!("got non-log event"));
    };
    state.ingest_listings_contract_event(our, *log)
}

fn fetch_package_manifest(package: &PackageId) -> anyhow::Result<Vec<kt::PackageManifestEntry>> {
    let drive_path = format!("/{}/pkg", package);
    Request::to(("our", "vfs", "distro", "sys"))
        .body(serde_json::to_vec(&vfs::VfsRequest {
            path: format!("{}/manifest.json", drive_path),
            action: vfs::VfsAction::Read,
        })?)
        .send_and_await_response(5)??;
    let Some(blob) = get_blob() else {
        return Err(anyhow::anyhow!("no blob"));
    };
    let manifest = String::from_utf8(blob.bytes)?;
    Ok(serde_json::from_str::<Vec<kt::PackageManifestEntry>>(
        &manifest,
    )?)
}

/// the steps to take an existing package on disk and install/start it
/// make sure you have reviewed and approved caps in manifest before calling this
pub fn handle_install(
    our: &Address,
    state: &mut State,
    package_id: &PackageId,
) -> anyhow::Result<()> {
    let drive_path = format!("/{package_id}/pkg");
    let manifest = fetch_package_manifest(package_id)?;
    // always grant read/write to their drive, which we created for them
    let Some(read_cap) = get_capability(
        &Address::new(&our.node, ("vfs", "distro", "sys")),
        &serde_json::to_string(&serde_json::json!({
            "kind": "read",
            "drive": drive_path,
        }))?,
    ) else {
        return Err(anyhow::anyhow!("no read cap"));
    };
    let Some(write_cap) = get_capability(
        &Address::new(&our.node, ("vfs", "distro", "sys")),
        &serde_json::to_string(&serde_json::json!({
            "kind": "write",
            "drive": drive_path,
        }))?,
    ) else {
        return Err(anyhow::anyhow!("no write cap"));
    };
    let Some(networking_cap) = get_capability(
        &Address::new(&our.node, ("kernel", "distro", "sys")),
        &"\"network\"".to_string(),
    ) else {
        return Err(anyhow::anyhow!("no net cap"));
    };
    // first, for each process in manifest, initialize it
    // then, once all have been initialized, grant them requested caps
    // and finally start them.
    for entry in &manifest {
        let wasm_path = if entry.process_wasm_path.starts_with("/") {
            entry.process_wasm_path.clone()
        } else {
            format!("/{}", entry.process_wasm_path)
        };
        let wasm_path = format!("{}{}", drive_path, wasm_path);
        let process_id = format!("{}:{}", entry.process_name, package_id);
        let Ok(parsed_new_process_id) = process_id.parse::<ProcessId>() else {
            return Err(anyhow::anyhow!("invalid process id!"));
        };
        // kill process if it already exists
        Request::to(("our", "kernel", "distro", "sys"))
            .body(serde_json::to_vec(&kt::KernelCommand::KillProcess(
                parsed_new_process_id.clone(),
            ))?)
            .send()?;

        if let Ok(vfs::VfsResponse::Err(_)) = serde_json::from_slice(
            Request::to(("our", "vfs", "distro", "sys"))
                .body(serde_json::to_vec(&vfs::VfsRequest {
                    path: wasm_path.clone(),
                    action: vfs::VfsAction::Read,
                })?)
                .send_and_await_response(5)??
                .body(),
        ) {
            return Err(anyhow::anyhow!("failed to read process file"));
        };

        let Ok(kt::KernelResponse::InitializedProcess) = serde_json::from_slice(
            Request::new()
                .target(("our", "kernel", "distro", "sys"))
                .body(serde_json::to_vec(&kt::KernelCommand::InitializeProcess {
                    id: parsed_new_process_id.clone(),
                    wasm_bytes_handle: wasm_path,
                    wit_version: None,
                    on_exit: entry.on_exit.clone(),
                    initial_capabilities: HashSet::new(),
                    public: entry.public,
                })?)
                .inherit(true)
                .send_and_await_response(5)??
                .body(),
        ) else {
            return Err(anyhow::anyhow!("failed to initialize process"));
        };
        // build initial caps
        let mut requested_capabilities: Vec<kt::Capability> = vec![];
        for value in &entry.request_capabilities {
            match value {
                serde_json::Value::String(process_name) => {
                    if let Ok(parsed_process_id) = process_name.parse::<ProcessId>() {
                        requested_capabilities.push(kt::Capability {
                            issuer: Address {
                                node: our.node.clone(),
                                process: parsed_process_id.clone(),
                            },
                            params: "\"messaging\"".into(),
                        });
                    } else {
                        println!(
                            "app-store: invalid cap: {} for {} to request!",
                            value.to_string(),
                            package_id
                        );
                    }
                }
                serde_json::Value::Object(map) => {
                    if let Some(process_name) = map.get("process") {
                        if let Ok(parsed_process_id) = process_name
                            .as_str()
                            .unwrap_or_default()
                            .parse::<ProcessId>()
                        {
                            if let Some(params) = map.get("params") {
                                requested_capabilities.push(kt::Capability {
                                    issuer: Address {
                                        node: our.node.clone(),
                                        process: parsed_process_id.clone(),
                                    },
                                    params: params.to_string(),
                                });
                            } else {
                                println!(
                                    "app-store: invalid cap: {} for {} to request!",
                                    value.to_string(),
                                    package_id
                                );
                            }
                        }
                    }
                }
                _ => {
                    continue;
                }
            }
        }
        if entry.request_networking {
            requested_capabilities.push(kt::de_wit_capability(networking_cap.clone()));
        }
        requested_capabilities.push(kt::de_wit_capability(read_cap.clone()));
        requested_capabilities.push(kt::de_wit_capability(write_cap.clone()));
        Request::new()
            .target(("our", "kernel", "distro", "sys"))
            .body(serde_json::to_vec(&kt::KernelCommand::GrantCapabilities {
                target: parsed_new_process_id.clone(),
                capabilities: requested_capabilities,
            })?)
            .send()?;
    }
    // THEN, *after* all processes have been initialized, grant caps in manifest
    // TODO for both grants and requests: make the vector of caps
    // and then do one GrantCapabilities message at the end. much faster.
    for entry in &manifest {
        let process_id = format!("{}:{}", entry.process_name, package_id);
        let Ok(parsed_new_process_id) = process_id.parse::<ProcessId>() else {
            return Err(anyhow::anyhow!("invalid process id!"));
        };
        for value in &entry.grant_capabilities {
            match value {
                serde_json::Value::String(process_name) => {
                    if let Ok(parsed_process_id) = process_name.parse::<ProcessId>() {
                        Request::to(("our", "kernel", "distro", "sys"))
                            .body(
                                serde_json::to_vec(&kt::KernelCommand::GrantCapabilities {
                                    target: parsed_process_id,
                                    capabilities: vec![kt::Capability {
                                        issuer: Address {
                                            node: our.node.clone(),
                                            process: parsed_new_process_id.clone(),
                                        },
                                        params: "\"messaging\"".into(),
                                    }],
                                })
                                .unwrap(),
                            )
                            .send()?;
                    }
                }
                serde_json::Value::Object(map) => {
                    if let Some(process_name) = map.get("process") {
                        if let Ok(parsed_process_id) = process_name
                            .as_str()
                            .unwrap_or_default()
                            .parse::<ProcessId>()
                        {
                            if let Some(params) = map.get("params") {
                                Request::to(("our", "kernel", "distro", "sys"))
                                    .body(serde_json::to_vec(
                                        &kt::KernelCommand::GrantCapabilities {
                                            target: parsed_process_id,
                                            capabilities: vec![kt::Capability {
                                                issuer: Address {
                                                    node: our.node.clone(),
                                                    process: parsed_new_process_id.clone(),
                                                },
                                                params: params.to_string(),
                                            }],
                                        },
                                    )?)
                                    .send()?;
                            }
                        }
                    }
                }
                _ => {
                    continue;
                }
            }
        }
        let Ok(kt::KernelResponse::StartedProcess) = serde_json::from_slice(
            Request::to(("our", "kernel", "distro", "sys"))
                .body(serde_json::to_vec(&kt::KernelCommand::RunProcess(
                    parsed_new_process_id,
                ))?)
                .send_and_await_response(5)??
                .body(),
        ) else {
            return Err(anyhow::anyhow!("failed to start process"));
        };
    }
    // finally set the package as installed
    state.update_downloaded_package(package_id, |package_state| {
        package_state.installed = true;
    });
    Ok(())
}
