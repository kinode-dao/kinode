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
use crate::kinode::process::downloads::{DownloadResponse, ProgressUpdate};
use crate::kinode::process::main::{
    ApisResponse, GetApiResponse, InstallResponse, LocalRequest, LocalResponse, NewPackageRequest,
    NewPackageResponse, UninstallResponse,
};
use kinode_process_lib::{
    await_message, call_init, get_blob, http, println, vfs, Address, LazyLoadBlob, Message,
    PackageId, Response,
};
use serde::{Deserialize, Serialize};
use state::State; // REQUESTED PACKAGE

wit_bindgen::generate!({
    path: "target/wit",
    generate_unused_types: true,
    world: "app-store-sys-v0",
    additional_derives: [serde::Deserialize, serde::Serialize],
});

mod http_api;
pub mod state;
pub mod utils;

const VFS_TIMEOUT: u64 = 10;

// internal types

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)] // untagged as a meta-type for all incoming requests
pub enum Req {
    LocalRequest(LocalRequest),
    Progress(ProgressUpdate),
    Http(http::server::HttpServerRequest),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)] // untagged as a meta-type for all incoming responses
pub enum Resp {
    LocalResponse(LocalResponse),
    Download(DownloadResponse),
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
                // TODO handle these based on what they are triggered by
                println!("got network error: {send_error}");
            }
            Ok(message) => {
                if let Err(e) = handle_message(&our, &mut state, &mut http_server, &message) {
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
fn handle_message(
    our: &Address,
    state: &mut State,
    http_server: &mut http::server::HttpServer,
    message: &Message,
) -> anyhow::Result<()> {
    if message.is_request() {
        match serde_json::from_slice::<Req>(message.body())? {
            Req::LocalRequest(local_request) => {
                if !message.is_local(our) {
                    return Err(anyhow::anyhow!("request from non-local node"));
                }
                let (body, blob) = handle_local_request(our, state, local_request);
                let response = Response::new().body(serde_json::to_vec(&body)?);
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
            _ => {}
        }
    } else {
        match serde_json::from_slice::<Resp>(message.body())? {
            Resp::LocalResponse(_) => {
                // don't need to handle these at the moment?
                // play with context.
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
                match utils::new_package(&package_id.to_process_lib(), state, blob.bytes) {
                    Ok(()) => LocalResponse::NewPackageResponse(NewPackageResponse::Success),
                    Err(_) => LocalResponse::NewPackageResponse(NewPackageResponse::InstallFailed),
                },
                None,
            )
        }
        LocalRequest::Install(package_id) => (
            match utils::install(&package_id.to_process_lib(), &our.to_string()) {
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
