#![feature(let_chains)]
//! downloads:app_store:sys
//! manages downloading and sharing of versioned packages.
//!
use crate::kinode::process::downloads::{
    DirEntry, DownloadError, DownloadRequests, DownloadResponses, Entry, FileEntry,
    LocalDownloadRequest, ProgressUpdate, RemoteDownloadRequest,
};
use std::{collections::HashSet, io::Read, str::FromStr};

use ft_worker_lib::{spawn_receive_transfer, spawn_send_transfer};
use kinode_process_lib::{
    await_message, call_init, get_blob, get_state,
    http::{
        self,
        client::{HttpClientError, HttpClientResponse},
    },
    print_to_terminal, println,
    vfs::{self, Directory, File},
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
    Download(DownloadResponses),
    HttpClient(Result<HttpClientResponse, HttpClientError>),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct State {
    mirroring: HashSet<PackageId>,
}

impl State {
    fn load() -> Self {
        match get_state() {
            Some(blob) => match serde_json::from_slice::<State>(&blob) {
                Ok(state) => state,
                Err(_) => State {
                    mirroring: HashSet::new(),
                },
            },
            None => State {
                mirroring: HashSet::new(),
            },
        }
    }
}

call_init!(init);
fn init(our: Address) {
    println!("started");

    // mirroring metadata is separate from vfs downloads state.
    let mut state = State::load();

    // /app_store:sys/downloads/
    vfs::create_drive(our.package_id(), "downloads", None)
        .expect("could not create /downloads drive");

    let mut downloads =
        open_or_create_dir("/app_store:sys/downloads").expect("could not open downloads");
    let mut tmp = open_or_create_dir("/app_store:sys/downloads/tmp").expect("could not open tmp");

    loop {
        match await_message() {
            Err(send_error) => {
                print_to_terminal(1, &format!("got network error: {send_error}"));
            }
            Ok(message) => {
                if let Err(e) = handle_message(&our, &mut state, &message, &mut downloads, &mut tmp)
                {
                    print_to_terminal(1, &format!("error handling message: {:?}", e));
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
    downloads: &mut Directory,
    tmp: &mut Directory,
) -> anyhow::Result<()> {
    if message.is_request() {
        match serde_json::from_slice::<DownloadRequests>(message.body())? {
            DownloadRequests::LocalDownload(download_request) => {
                // we want to download a package.
                if !message.is_local(our) {
                    return Err(anyhow::anyhow!("not local"));
                }
                let LocalDownloadRequest {
                    package_id,
                    desired_version_hash,
                    download_from,
                } = download_request;

                if download_from.starts_with("http") {
                    // use http_client to GET it
                    let Ok(url) = url::Url::parse(&download_from) else {
                        return Err(anyhow::anyhow!("bad url: {download_from}"));
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
                    &download_from,
                    APP_SHARE_TIMEOUT,
                )?;

                let target_node = Address::new(
                    download_from,
                    ProcessId::new(Some("downloads"), "app_store", "sys"),
                );

                Request::new()
                    .target(target_node)
                    .body(serde_json::to_vec(&DownloadRequests::RemoteDownload(
                        RemoteDownloadRequest {
                            package_id,
                            desired_version_hash,
                            worker_address: our_worker.to_string(),
                        },
                    ))?)
                    .send()?;
                // ok, now technically everything is ze ready. let's see what awaits and updates we send upstream/to the frontend.
            }
            DownloadRequests::RemoteDownload(download_request) => {
                // this is a node requesting a download from us.
                // check if we are mirroring. we should maybe implement some back and forth here.
                // small handshake for started? but we do not really want to wait for that in this loop..
                // might be okay. implement.
                let RemoteDownloadRequest {
                    package_id,
                    desired_version_hash,
                    worker_address,
                } = download_request;

                let target_worker = Address::from_str(&worker_address)?;
                let _ = spawn_send_transfer(
                    our,
                    &package_id,
                    &desired_version_hash,
                    APP_SHARE_TIMEOUT,
                    &target_worker,
                )?;
            }
            DownloadRequests::Progress(ProgressUpdate {
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
                        serde_json::to_vec(&DownloadRequests::Progress(ProgressUpdate {
                            package_id,
                            version_hash,
                            downloaded,
                            total,
                        }))
                        .unwrap(),
                    )
                    .send();
            }
            DownloadRequests::GetFiles(maybe_id) => {
                // if not local, throw to the boonies.
                // note, can also implement a discovery protocol here in the future
                if !message.is_local(our) {
                    return Err(anyhow::anyhow!("not local"));
                }
                let files = match maybe_id {
                    Some(id) => {
                        let package_path =
                            format!("{}/{}", downloads.path, id.to_process_lib().to_string());
                        let dir = vfs::open_dir(&package_path, false, None)?;
                        let dir = dir.read()?;
                        format_entries(dir, state)
                    }
                    None => {
                        let dir = downloads.read()?;
                        format_entries(dir, state)
                    }
                };

                Response::new()
                    .body(serde_json::to_string(&files)?)
                    .send()?;
            }
            DownloadRequests::AddDownload(add_req) => {
                if !message.is_local(our) {
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

                // Write the zip file
                let zip_path = format!("{}/{}.zip", package_dir, add_req.version_hash);
                let file = vfs::create_file(&zip_path, None)?;
                file.write(bytes.as_slice())?;

                // Write the manifest file
                // Extract and write the manifest
                let manifest_path = format!("{}/{}.json", package_dir, add_req.version_hash);
                extract_and_write_manifest(&bytes, &manifest_path)?;

                Response::new()
                    .body(serde_json::to_vec(&Resp::Download(
                        DownloadResponses::Success,
                    ))?)
                    .send()?;
            }
            DownloadRequests::StartMirroring(package_id) => {
                let package_id = package_id.to_process_lib();
                state.mirroring.insert(package_id);
                Response::new()
                    .body(serde_json::to_vec(&Resp::Download(
                        DownloadResponses::Success,
                    ))?)
                    .send()?;
            }
            DownloadRequests::StopMirroring(package_id) => {
                let package_id = package_id.to_process_lib();
                state.mirroring.remove(&package_id);
                Response::new()
                    .body(serde_json::to_vec(&Resp::Download(
                        DownloadResponses::Success,
                    ))?)
                    .send()?;
            }
            _ => {}
        }
    } else {
        match serde_json::from_slice::<Resp>(message.body())? {
            Resp::Download(download_response) => {
                // TODO handle download response
                // maybe push to http? need await for that...
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
    // state.mirroring.insert(package_id);

    // TODO: Verify the integrity of the downloaded file (e.g., checksum)

    // TODO: Notify any waiting processes that the download is complete

    println!("Successfully processed HTTP download for: {}", name);

    Ok(())
}

fn format_entries(entries: Vec<vfs::DirEntry>, state: &State) -> Vec<Entry> {
    entries
        .into_iter()
        .filter_map(|entry| {
            let name = entry
                .path
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_string();
            let is_file = entry.file_type == vfs::FileType::File;

            if is_file {
                if name.ends_with(".zip") {
                    let size = vfs::metadata(&entry.path, None)
                        .ok()
                        .map(|meta| meta.len)
                        .unwrap_or(0);
                    let json_path = entry.path.replace(".zip", ".json");
                    let manifest = if let Ok(file) = vfs::open_file(&json_path, false, None) {
                        file.read_to_string().ok()
                    } else {
                        None
                    };

                    Some(Entry::File(FileEntry {
                        name,
                        size,
                        manifest: manifest.unwrap_or_default(),
                    }))
                } else {
                    None // ignore non-zip files
                }
            } else {
                let mirroring = PackageId::from_str(&name)
                    .map(|package_id| state.mirroring.contains(&package_id))
                    .unwrap_or(false);

                Some(Entry::Dir(DirEntry { name, mirroring }))
            }
        })
        .collect()
}

fn extract_and_write_manifest(file_contents: &[u8], manifest_path: &str) -> anyhow::Result<()> {
    let reader = std::io::Cursor::new(file_contents);
    let mut archive = zip::ZipArchive::new(reader)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if file.name() == "manifest.json" {
            let mut contents = String::new();
            file.read_to_string(&mut contents)?;

            let manifest_file = open_or_create_file(&manifest_path)?;
            manifest_file.write(contents.as_bytes())?;

            println!("Extracted and wrote manifest.json");
            break;
        }
    }

    Ok(())
}

// note this, might be tricky:
// when ready, extract + write the damn manifest to a file location baby!

// let wit_version = match metadata {
//     Some(metadata) => metadata.properties.wit_version,
//     None => Some(0),
// };

/// helper function for vfs files, open if exists, if not create
fn open_or_create_file(path: &str) -> anyhow::Result<File> {
    match vfs::open_file(path, false, None) {
        Ok(file) => Ok(file),
        Err(_) => match vfs::open_file(path, true, None) {
            Ok(file) => Ok(file),
            Err(_) => Err(anyhow::anyhow!("could not create file")),
        },
    }
}

/// helper function for vfs directories, open if exists, if not create
fn open_or_create_dir(path: &str) -> anyhow::Result<Directory> {
    match vfs::open_dir(path, false, None) {
        Ok(dir) => Ok(dir),
        Err(_) => match vfs::open_dir(path, true, None) {
            Ok(dir) => Ok(dir),
            Err(_) => Err(anyhow::anyhow!("could not create file")),
        },
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
