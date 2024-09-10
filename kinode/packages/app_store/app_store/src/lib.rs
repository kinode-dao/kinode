#![feature(let_chains)]
//! main:app_store:
//! acts as a manager for installed apps, and coordinator for http requests.
//!
//! the chain:app_store process takes care of on-chain indexing, while
//! the downloads:app_store process takes care of sharing and versioning.
//!
//! packages are apps; apps are packages. chain:app_store uses the kimap contract to determine
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
use crate::kinode::process::downloads::{
    DownloadCompleteRequest, DownloadResponses, ProgressUpdate,
};
use crate::kinode::process::main::{
    ApisResponse, GetApiResponse, InstallPackageRequest, InstallResponse, LocalRequest,
    LocalResponse, NewPackageRequest, NewPackageResponse, Response as AppStoreResponse,
    UninstallResponse,
};
use kinode_process_lib::{
    await_message, call_init, get_blob, http, print_to_terminal, println, vfs, Address,
    LazyLoadBlob, Message, PackageId, Response,
};
use serde::{Deserialize, Serialize};
use state::State;

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
    Progress(ProgressUpdate),
    DownloadComplete(DownloadCompleteRequest),
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
    println!("started");

    let mut http_server = http::server::HttpServer::new(5);
    http_api::init_frontend(&our, &mut http_server);

    let mut state = State::load().expect("state loading failed");

    loop {
        match await_message() {
            Err(send_error) => {
                print_to_terminal(1, &format!("main: got network error: {send_error}"));
            }
            Ok(message) => {
                if let Err(e) = handle_message(&our, &mut state, &mut http_server, &message) {
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
            Req::Http(server_request) => {
                if !message.is_local(&our) || message.source().process != "http_server:distro:sys" {
                    return Err(anyhow::anyhow!("http_server from non-local node"));
                }
                http_server.handle_request(
                    server_request,
                    |incoming| http_api::handle_http_request(our, state, &incoming),
                    |_channel_id, _message_type, _blob| {
                        // not expecting any websocket messages from FE currently
                    },
                );
            }
            Req::Progress(progress) => {
                if !message.is_local(&our) {
                    return Err(anyhow::anyhow!("http_server from non-local node"));
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

                // auto_install case:
                // the downloads process has given us the new package manifest's
                // capability hashes, and the old package's capability hashes.
                // we can use these to determine if the new package has the same
                // capabilities as the old one, and if so, auto-install it.
                if let Some(context) = message.context() {
                    let manifest_hash = String::from_utf8(context.to_vec())?;
                    if let Some(package) =
                        state.packages.get(&req.package_id.clone().to_process_lib())
                    {
                        if package.manifest_hash == Some(manifest_hash) {
                            print_to_terminal(1, "auto_install:main, manifest_hash match");
                            if let Err(e) = utils::install(
                                &req.package_id,
                                None,
                                &req.version_hash,
                                state,
                                &our.node,
                            ) {
                                print_to_terminal(
                                    1,
                                    &format!("error auto_installing package: {e}"),
                                );
                            } else {
                                println!(
                                    "auto_installed update for package: {:?}",
                                    &req.package_id.to_process_lib()
                                );
                            }
                        } else {
                            print_to_terminal(1, "auto_install:main, manifest_hash do not match");
                        }
                    }
                }
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
                        "successfully installed package: {:?}",
                        &package_id.to_process_lib()
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
            match utils::uninstall(state, &package_id.clone().to_process_lib()) {
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
