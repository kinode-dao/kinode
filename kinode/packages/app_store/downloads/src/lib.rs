#![feature(let_chains)]
//! downloads:app_store:sys
//! manages downloading and sharing of versioned packages.
//!
use crate::kinode::process::downloads::{
    AvailableFiles, DownloadRequest, DownloadResponse, Downloads, Entry, ProgressUpdate,
};
use crate::kinode::process::main::Error;
use std::{collections::HashSet, str::FromStr};

use ft_worker_lib::{spawn_receive_transfer, spawn_send_transfer};
use kinode_process_lib::get_blob;
use kinode_process_lib::{
    await_message, call_init,
    http::{
        self,
        client::{HttpClientError, HttpClientResponse},
    },
    print_to_terminal, println,
    vfs::{self, DirEntry, Directory, File, SeekFrom},
    Address, Message, PackageId, ProcessId, Request, Response,
};
use serde::{Deserialize, Serialize};

wit_bindgen::generate!({
    path: "target/wit",
    generate_unused_types: true,
    world: "app-store-sys-v0",
    additional_derives: [serde::Deserialize, serde::Serialize],
});

mod ft_worker_lib;

pub const VFS_TIMEOUT: u64 = 5; // 5s
pub const APP_SHARE_TIMEOUT: u64 = 120; // 120s

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)] // untagged as a meta-type for all incoming responses
pub enum Resp {
    Download(DownloadResponse),
    HttpClient(Result<HttpClientResponse, HttpClientError>),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct State {
    mirroring: HashSet<(PackageId, String)>, // (package_id, version_hash)
                                             // Other implicit state needed?
                                             // we could do something like a logfile for errors, downloads succeeded etc!
}

call_init!(init);
fn init(our: Address) {
    println!("started");

    // load from state, okay to decouple from vfs. it's "app-state" for downloads.
    let mut state = State {
        mirroring: HashSet::new(),
    };

    // /log/.log file for debugs
    vfs::create_drive(our.package_id(), "log", None).expect("could not create /log drive");
    // /downloads/ for downloads
    vfs::create_drive(our.package_id(), "downloads", None)
        .expect("could not create /downloads drive");

    let mut logfile =
        vfs::open_file("/app_store:sys/log/.log", true, Some(5)).expect("could not open logfile");
    logfile
        .seek(SeekFrom::End(0))
        .expect("could not seek to end of logfile");
    // FIX this api... first creates (fails if already exists. second shouldn't fail like this... like files.)
    let _ = vfs::open_dir("/app_store:sys/downloads", true, None);
    let mut downloads =
        vfs::open_dir("/app_store:sys/downloads", false, None).expect("could not open downloads");
    let _ = vfs::open_dir("/app_store:sys/downloads/tmp", true, None);
    let mut tmp =
        vfs::open_dir("/app_store:sys/downloads/tmp", false, None).expect("could not open tmp");

    loop {
        match await_message() {
            Err(send_error) => {
                println!("got network error: {send_error}");
            }
            Ok(message) => {
                if let Err(e) = handle_message(
                    &our,
                    &mut state,
                    &message,
                    &mut logfile,
                    &mut downloads,
                    &mut tmp,
                ) {
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

/// message router: parse into our Req and Resp types, then pass to
/// function defined for each kind of message. check whether the source
/// of the message is allowed to send that kind of message to us.
/// finally, fire a response if expected from a request.
fn handle_message(
    our: &Address,
    state: &mut State,
    message: &Message,
    logfile: &mut File,
    downloads: &mut Directory,
    tmp: &mut Directory,
) -> anyhow::Result<()> {
    if message.is_request() {
        match serde_json::from_slice::<Downloads>(message.body())? {
            Downloads::Download(download_request) => {
                let is_local = message.is_local(our);
                match handle_download_request(our, state, download_request, is_local) {
                    Ok(()) => {
                        Response::new()
                            .body(serde_json::to_vec(&Resp::Download(DownloadResponse {
                                success: true,
                                error: None,
                            }))?)
                            .send()?;
                    }
                    Err(e) => {
                        // make function, print and log!
                        print_and_log(logfile, &format!("error handling download request: {e}"), 1);
                        Response::new()
                            .body(serde_json::to_vec(&Resp::Download(DownloadResponse {
                                success: false,
                                error: Some(e.to_string()),
                            }))?)
                            .send()?;
                    }
                }
            }
            Downloads::Progress(ProgressUpdate {
                package_id,
                version_hash,
                downloaded,
                total,
            }) => {
                // forward progress to main:app_store:sys,
                // which then pushes to the frontend.
                let target =
                    Address::new(&our.node, ProcessId::new(Some("main"), "app_store", "sys"));
                let _ = Request::new()
                    .target(target)
                    .body(
                        serde_json::to_vec(&ProgressUpdate {
                            package_id,
                            version_hash,
                            downloaded,
                            total,
                        })
                        .unwrap(),
                    )
                    .send();
            }
            Downloads::GetFiles(maybe_id) => {
                // if not local, throw to the boonies. (could also implement and discovery protocol here..)
                if !message.is_local(our) {
                    // todo figure out full error throwing for http pathways.
                    return Err(anyhow::anyhow!("not local"));
                }
                let files = match maybe_id {
                    Some(id) => {
                        let package_path =
                            format!("{}/{}", downloads.path, id.to_process_lib().to_string());
                        let dir = vfs::open_dir(&package_path, false, None)?;
                        let dir = dir.read()?;
                        format_entries(dir)
                    }
                    None => {
                        let dir = downloads.read()?;
                        format_entries(dir)
                    }
                };

                Response::new()
                    .body(serde_json::to_vec(&AvailableFiles { files })?)
                    .send()?;
            }
            Downloads::AddDownload(add_req) => {
                if !message.is_local(our) {
                    // todo figure out full error throwing for http pathways.
                    return Err(anyhow::anyhow!("not local"));
                }
                let Some(blob) = get_blob() else {
                    return Err(anyhow::anyhow!("could not get blob"));
                };
                let bytes = blob.bytes;

                let package_dir = format!(
                    "{}/{}",
                    downloads.path,
                    add_req.package_id.to_process_lib().to_string()
                );
                let _ = vfs::open_dir(&package_dir, true, None);
                let file = vfs::create_file(
                    &format!("{}/{}.zip", package_dir, add_req.version_hash),
                    None,
                )?;
                let _ = file.write(bytes.as_slice())?;

                Response::new()
                    .body(serde_json::to_vec(&Resp::Download(DownloadResponse {
                        success: true,
                        error: None,
                    }))?)
                    .send()?;
            }
            _ => {}
        }
    } else {
        match serde_json::from_slice::<Resp>(message.body())? {
            Resp::Download(download_response) => {
                // TODO handle download response
                // send_and_awaits? this might not be needed.
            }
            Resp::HttpClient(resp) => {
                let name = match message.context() {
                    Some(context) => std::str::from_utf8(context).unwrap_or_default(),
                    None => return Err(anyhow::anyhow!("http_client response without context")),
                };
                if let Ok(http::client::HttpClientResponse::Http(http::client::HttpResponse {
                    status,
                    ..
                })) = resp
                {
                    if status == 200 {
                        handle_receive_http_download(state, &name)?;
                    }
                } else {
                    println!("got http_client error: {resp:?}");
                }
            }
        }
    }
    Ok(())
}

fn handle_download_request(
    our: &Address,
    state: &mut State,
    download_request: DownloadRequest,
    is_local: bool,
) -> anyhow::Result<()> {
    let DownloadRequest {
        package_id,
        desired_version_hash,
        download_from,
    } = download_request;

    match is_local {
        true => {
            // we are requesting this: (handle http types in main?), forwarding here?
            if let Some(node_or_url) = download_from {
                if node_or_url.starts_with("http") {
                    // use http_client to GET it
                    let Ok(url) = url::Url::parse(&node_or_url) else {
                        return Err(anyhow::anyhow!("bad url: {node_or_url}"));
                    };
                    // TODO: need context in this to get it back.
                    http::client::send_request(http::Method::GET, url, None, Some(60), vec![]);
                }

                // go download from the node or url
                // spawn a worker, and send a downlaod to the node.
                let our_worker = spawn_receive_transfer(
                    our,
                    &package_id,
                    &desired_version_hash,
                    APP_SHARE_TIMEOUT,
                )?;

                let target_node = Address::new(
                    node_or_url,
                    ProcessId::new(Some("downloads"), "app_store", "sys"),
                );

                Request::new()
                    .target(target_node)
                    .body(
                        serde_json::to_vec(&DownloadRequest {
                            package_id,
                            desired_version_hash,
                            download_from: Some(our_worker.to_string()),
                        })
                        .unwrap(),
                    )
                    .send()?;
                // ok, now technically everything is ze ready. let's see what awaits and updates we send upstream/to the frontend.
            }
        }
        false => {
            // Someone wants to download something from us!
            // // check mirrors first! :]
            // handle the errors that come from spawning.

            if let Some(worker) = download_from {
                // handle this error too.
                let target_worker = Address::from_str(&worker)?;
                let _ = spawn_send_transfer(
                    our,
                    &package_id,
                    &desired_version_hash,
                    APP_SHARE_TIMEOUT,
                    &target_worker,
                )?;
            }

            // bam, transfer should happen. again, handle errors.
        }
    }

    // Update state to reflect that we're handling this download
    // fix wit things
    // state.mirroring.insert((package_id, desired_version_hash));

    Ok(())
}

fn handle_receive_http_download(state: &mut State, name: &str) -> anyhow::Result<()> {
    // use context here instead, verify bytes immediately.
    println!("Received HTTP download for: {}", name);

    // Parse the name to extract package_id and version_hash
    let parts: Vec<&str> = name.split('-').collect();
    if parts.len() != 2 {
        return Err(anyhow::anyhow!("Invalid download name format"));
    }

    let package_id = PackageId::from_str(parts[0])?;
    let version_hash = parts[1].to_string();

    // Move the downloaded file to the correct location
    let source_path = format!("/tmp/{}", name);
    let dest_path = format!(
        "/app_store:sys/downloads/{}:{}-{}.zip",
        package_id.package_name, package_id.publisher_node, version_hash
    );

    // vfs::rename(&source_path, &dest_path)?;

    // Update state to reflect that we're now mirroring this package
    state.mirroring.insert((package_id, version_hash.clone()));

    // TODO: Verify the integrity of the downloaded file (e.g., checksum)

    // TODO: Notify any waiting processes that the download is complete

    println!("Successfully processed HTTP download for: {}", name);

    Ok(())
}

fn format_entries(entries: Vec<DirEntry>) -> Vec<Entry> {
    entries
        .into_iter()
        .filter_map(|entry| {
            let name = entry.path.rsplit('/').next().unwrap_or_default();
            let is_file = entry.file_type == vfs::FileType::File;
            let size = vfs::metadata(&entry.path, None).ok().map(|meta| meta.len);
            Some(Entry {
                name: name.to_string(),
                is_file,
                size,
            })
        })
        .collect::<Vec<Entry>>()
}

// Some useful comments for the immediate future:

// NOTE: we should handle NewPackage, kit start-package should just work
//             (
//                 match utils::new_package(
//                     &package_id.to_process_lib(),
//                     state,
//                     metadata.to_erc721_metadata(),
//                     mirror,
//                     blob.bytes,
//                 ) {
//                     Ok(()) => LocalResponse::NewPackageResponse(NewPackageResponse::Success),
//                     Err(_) => LocalResponse::NewPackageResponse(NewPackageResponse::InstallFailed),
//                 },
//                 None,
//             )
// Need start/stop mirror commands here too.
// Auto updates... ?
// I'd imagine this would be triggered on the chain side almost right?
// that's where we hear about autoupdates first.

// then Apis.. we could do a get_apis, dependent on versions.
// but I'm going to punt for now, api_api can be moved to main_install section, 1 api per installed system: )
// that's actually a good system, because we don't really unzip unless we install (maybe in the future for inspecting files etc.)
// but that really should be done on remote

//         LocalRequest::StartMirroring(package_id) => (
//             match state.start_mirroring(&package_id.to_process_lib()) {
//                 true => LocalResponse::MirrorResponse(MirrorResponse::Success),
//                 false => LocalResponse::MirrorResponse(MirrorResponse::Failure),
//             },
//             None,
//         ),
//         LocalRequest::StopMirroring(package_id) => (
//             match state.stop_mirroring(&package_id.to_process_lib()) {
//                 true => LocalResponse::MirrorResponse(MirrorResponse::Success),
//                 false => LocalResponse::MirrorResponse(MirrorResponse::Failure),
//             },
//             None,
//

// note this, might be tricky:
// let wit_version = match metadata {
//     Some(metadata) => metadata.properties.wit_version,
//     None => Some(0),
// };

pub fn print_and_log(logfile: &mut File, message: &str, verbosity: u8) {
    print_to_terminal(verbosity, message);
    let _ = logfile.write_all(message.as_bytes());
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
