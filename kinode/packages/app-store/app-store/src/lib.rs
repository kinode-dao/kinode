#![feature(let_chains)]
//! main:app-store:sys
//!
//! This process serves as the primary interface for the App Store system in the Kinode ecosystem.
//! It coordinates between the http user interface, the chain process, and the downloads process.
//!
//! ## Responsibilities:
//!
//! 1. Handle user requests for app installation, uninstallation, and management.
//! 2. Coordinate with the chain process to get updated app metadata.
//! 3. Interact with the downloads process to manage app zip packages.
//! 4. Manage the local state of installed apps and their permissions & capabilities.
//! 5. Provide an HTTP API for frontend interactions.
//!
//! ## Key Components:
//!
//! - `state.rs`: Manages the local state of installed packages and their metadata.
//! - `http_api.rs`: Provides HTTP endpoints for frontend interactions.
//! - `utils.rs`: Utility functions for app management.
//!
//! ## Interaction Flow:
//!
//! 1. User initiates an action through the frontend (or terminal/other remote kinode).
//! 2. The HTTP API receives the request and translates it into an internal message.
//! 3. `handle_message` routes the message to the appropriate handler.
//! 4. The handler processes the request, often involving communication with the chain or downloads process.
//! 5. Results are sent back to the user through the HTTP API.
//!
//! Note: This process does not directly handle file transfers or on-chain operations.
//! It delegates these responsibilities to the downloads and chain processes respectively.
//!
use crate::kinode::process::downloads::{
    AutoDownloadCompleteRequest, DownloadCompleteRequest, DownloadResponses, ProgressUpdate,
};
use crate::kinode::process::main::{
    ApisResponse, GetApiResponse, InstallPackageRequest, InstallResponse, LocalRequest,
    LocalResponse, NewPackageRequest, NewPackageResponse, RemoteRequest, RemoteResponse,
    Response as AppStoreResponse, UninstallResponse,
};
use kinode_process_lib::{
    await_message, call_init, get_blob, http, print_to_terminal, println, vfs, Address,
    LazyLoadBlob, Message, PackageId, Response,
};
use serde::{Deserialize, Serialize};
use state::{State, UpdateInfo, Updates};

wit_bindgen::generate!({
    path: "target/wit",
    generate_unused_types: true,
    world: "app-store-sys-v1",
    additional_derives: [serde::Deserialize, serde::Serialize, process_macros::SerdeJsonInto],
});

mod http_api;
pub mod state;
pub mod utils;

const VFS_TIMEOUT: u64 = 10;

// internal types

#[derive(Debug, Serialize, Deserialize, process_macros::SerdeJsonInto)]
#[serde(untagged)] // untagged as a meta-type for all incoming requests
pub enum Req {
    LocalRequest(LocalRequest),
    RemoteRequest(RemoteRequest),
    Progress(ProgressUpdate),
    DownloadComplete(DownloadCompleteRequest),
    AutoDownloadComplete(AutoDownloadCompleteRequest),
    Http(http::server::HttpServerRequest),
}

#[derive(Debug, Serialize, Deserialize, process_macros::SerdeJsonInto)]
#[serde(untagged)] // untagged as a meta-type for all incoming responses
pub enum Resp {
    LocalResponse(LocalResponse),
    Download(DownloadResponses),
}

call_init!(init);
fn init(our: Address) {
    let mut http_server = http::server::HttpServer::new(5);
    http_api::init_frontend(&our, &mut http_server);

    // state = state built from the filesystem, installed packages
    // updates = state saved with get/set_state(), auto_update metadata.
    let mut state = State::load().expect("state loading failed");
    let mut updates = Updates::load();
    loop {
        match await_message() {
            Err(send_error) => {
                print_to_terminal(1, &format!("main: got network error: {send_error}"));
            }
            Ok(message) => {
                if let Err(e) =
                    handle_message(&our, &mut state, &mut updates, &mut http_server, &message)
                {
                    let error_message = format!("error handling message: {e:?}");
                    print_to_terminal(1, &error_message);
                    Response::new()
                        .body(AppStoreResponse::HandlingError(error_message))
                        .send()
                        .unwrap();
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
    state: &mut State,
    updates: &mut Updates,
    http_server: &mut http::server::HttpServer,
    message: &Message,
) -> anyhow::Result<()> {
    if message.is_request() {
        match message.body().try_into()? {
            Req::LocalRequest(local_request) => {
                if !message.is_local(our) {
                    return Err(anyhow::anyhow!("request from non-local node"));
                }
                let (body, blob) = handle_local_request(our, state, local_request);
                let response = Response::new().body(&body);
                if let Some(blob) = blob {
                    response.blob(blob).send()?;
                } else {
                    response.send()?;
                }
            }
            Req::RemoteRequest(remote_request) => match remote_request {
                RemoteRequest::Ping => {
                    Response::new().body(RemoteResponse::Ping).send()?;
                }
            },
            Req::Http(server_request) => {
                if !message.is_local(&our) || message.source().process != "http-server:distro:sys" {
                    return Err(anyhow::anyhow!("http-server from non-local node"));
                }
                http_server.handle_request(
                    server_request,
                    |incoming| http_api::handle_http_request(our, state, updates, &incoming),
                    |_channel_id, _message_type, _blob| {
                        // not expecting any websocket messages from FE currently
                    },
                );
            }
            Req::Progress(progress) => {
                if !message.is_local(&our) {
                    return Err(anyhow::anyhow!("http-server from non-local node"));
                }
                http_server.ws_push_all_channels(
                    "/",
                    http::server::WsMessageType::Text,
                    LazyLoadBlob {
                        mime: Some("application/json".to_string()),
                        bytes: serde_json::to_vec(&serde_json::json!({
                            "kind": "progress",
                            "data": {
                                "package_id": progress.package_id,
                                "version_hash": progress.version_hash,
                                "downloaded": progress.downloaded,
                                "total": progress.total,
                            }
                        }))
                        .unwrap(),
                    },
                );
            }
            Req::AutoDownloadComplete(req) => {
                if !message.is_local(&our) {
                    return Err(anyhow::anyhow!(
                        "auto download complete from non-local node"
                    ));
                }

                match req {
                    AutoDownloadCompleteRequest::Success(succ) => {
                        // auto_install case:
                        // the downloads process has given us the new package manifest's
                        // capability hashes, and the old package's capability hashes.
                        // we can use these to determine if the new package has the same
                        // capabilities as the old one, and if so, auto-install it.
                        let manifest_hash = succ.manifest_hash;
                        let package_id = succ.package_id;
                        let version_hash = succ.version_hash;

                        let process_lib_package_id = package_id.clone().to_process_lib();

                        // first, check if we have the package and get its manifest hash
                        let should_auto_install = state
                            .packages
                            .get(&process_lib_package_id)
                            .map(|package| package.manifest_hash == Some(manifest_hash.clone()))
                            .unwrap_or(false);

                        if should_auto_install {
                            if let Err(e) =
                                utils::install(&package_id, None, &version_hash, state, &our.node)
                            {
                                println!("error auto-installing package: {e}");
                                // Get or create the outer map for this package
                                updates
                                    .package_updates
                                    .entry(package_id.to_process_lib())
                                    .or_default()
                                    .insert(
                                        version_hash.clone(),
                                        UpdateInfo {
                                            errors: vec![],
                                            pending_manifest_hash: Some(manifest_hash.clone()),
                                        },
                                    );
                                updates.save();
                            } else {
                                println!(
                                    "auto-installed update for package: {process_lib_package_id}"
                                );
                            }
                        } else {
                            // TODO.
                            updates
                                .package_updates
                                .entry(package_id.to_process_lib())
                                .or_default()
                                .insert(
                                    version_hash.clone(),
                                    UpdateInfo {
                                        errors: vec![],
                                        pending_manifest_hash: Some(manifest_hash.clone()),
                                    },
                                );
                            updates.save();
                        }
                    }
                    AutoDownloadCompleteRequest::Err(err) => {
                        println!("error auto-downloading package: {err:?}");
                        updates
                            .package_updates
                            .entry(err.package_id.to_process_lib())
                            .or_default()
                            .insert(
                                err.version_hash.clone(),
                                UpdateInfo {
                                    errors: err.tries,
                                    pending_manifest_hash: None,
                                },
                            );
                        updates.save();
                    }
                }
            }
            Req::DownloadComplete(req) => {
                if !message.is_local(&our) {
                    return Err(anyhow::anyhow!("download complete from non-local node"));
                }

                http_server.ws_push_all_channels(
                    "/",
                    http::server::WsMessageType::Text,
                    LazyLoadBlob {
                        mime: Some("application/json".to_string()),
                        bytes: serde_json::to_vec(&serde_json::json!({
                            "kind": "complete",
                            "data": {
                                "package_id": req.package_id,
                                "version_hash": req.version_hash,
                                "error": req.err,
                            }
                        }))
                        .unwrap(),
                    },
                );
            }
        }
    } else {
        match message.body().try_into()? {
            Resp::LocalResponse(_) => {
                // don't need to handle these at the moment
            }
            _ => {}
        }
    }
    Ok(())
}

/// fielding requests to download packages and APIs from us
/// only `our.node` can call this
fn handle_local_request(
    our: &Address,
    state: &mut State,
    request: LocalRequest,
) -> (LocalResponse, Option<LazyLoadBlob>) {
    match request {
        LocalRequest::NewPackage(NewPackageRequest { package_id, mirror }) => {
            let Some(blob) = get_blob() else {
                return (
                    LocalResponse::NewPackageResponse(NewPackageResponse::NoBlob),
                    None,
                );
            };
            (
                match utils::new_package(package_id, mirror, blob.bytes) {
                    Ok(()) => LocalResponse::NewPackageResponse(NewPackageResponse::Success),
                    Err(_) => LocalResponse::NewPackageResponse(NewPackageResponse::InstallFailed),
                },
                None,
            )
        }
        LocalRequest::Install(InstallPackageRequest {
            package_id,
            metadata,
            version_hash,
        }) => (
            match utils::install(&package_id, metadata, &version_hash, state, &our.node) {
                Ok(()) => {
                    println!(
                        "successfully installed {}:{}",
                        package_id.package_name, package_id.publisher_node
                    );
                    LocalResponse::InstallResponse(InstallResponse::Success)
                }
                Err(e) => {
                    println!("error installing package: {e}");
                    LocalResponse::InstallResponse(InstallResponse::Failure)
                }
            },
            None,
        ),
        LocalRequest::Uninstall(package_id) => (
            match utils::uninstall(our, state, &package_id.clone().to_process_lib()) {
                Ok(()) => {
                    println!(
                        "successfully uninstalled package: {:?}",
                        &package_id.to_process_lib()
                    );
                    LocalResponse::UninstallResponse(UninstallResponse::Success)
                }
                Err(e) => {
                    println!(
                        "error uninstalling package: {:?}: {e}",
                        &package_id.to_process_lib()
                    );
                    LocalResponse::UninstallResponse(UninstallResponse::Failure)
                }
            },
            None,
        ),
        LocalRequest::Apis => (list_apis(state), None),
        LocalRequest::GetApi(package_id) => get_api(state, &package_id.to_process_lib()),
    }
}

pub fn get_api(state: &mut State, package_id: &PackageId) -> (LocalResponse, Option<LazyLoadBlob>) {
    if !state.installed_apis.contains(package_id) {
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
            .installed_apis
            .clone()
            .into_iter()
            .map(|id| crate::kinode::process::main::PackageId::from_process_lib(id))
            .collect(),
    })
}
