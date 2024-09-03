#![feature(let_chains)]
//! downloads:app_store:sys
//! manages downloading and sharing of versioned packages.
//!
use crate::kinode::process::downloads::{
    AutoUpdateRequest, DirEntry, DownloadCompleteRequest, DownloadError, DownloadRequests,
    DownloadResponses, Entry, FileEntry, HashMismatch, LocalDownloadRequest, RemoteDownloadRequest,
    RemoveFileRequest,
};
use std::{collections::HashSet, io::Read, str::FromStr};

use ft_worker_lib::{spawn_receive_transfer, spawn_send_transfer};
use kinode_process_lib::{
    await_message, call_init, get_blob, get_state,
    http::client,
    print_to_terminal, println, set_state,
    vfs::{self, Directory},
    Address, Message, PackageId, ProcessId, Request, Response,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

wit_bindgen::generate!({
    path: "target/wit",
    generate_unused_types: true,
    world: "app-store-sys-v1",
    additional_derives: [serde::Deserialize, serde::Serialize],
});

mod ft_worker_lib;

pub const VFS_TIMEOUT: u64 = 5; // 5s
pub const APP_SHARE_TIMEOUT: u64 = 120; // 120s

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)] // untagged as a meta-type for all incoming responses
pub enum Resp {
    Download(DownloadResponses),
    HttpClient(Result<client::HttpClientResponse, client::HttpClientError>),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct State {
    // persisted metadata about which packages we are mirroring
    mirroring: HashSet<PackageId>,
    // note, pending auto_updates are not persisted.
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
    println!("downloads: started");

    // mirroring metadata is separate from vfs downloads state.
    let mut state = State::load();

    // /app_store:sys/downloads/
    vfs::create_drive(our.package_id(), "downloads", None)
        .expect("could not create /downloads drive");

    let mut downloads =
        vfs::open_dir("/app_store:sys/downloads", true, None).expect("could not open downloads");
    let mut tmp =
        vfs::open_dir("/app_store:sys/downloads/tmp", true, None).expect("could not open tmp");

    let mut auto_updates: HashSet<(PackageId, String)> = HashSet::new();

    loop {
        match await_message() {
            Err(send_error) => {
                print_to_terminal(1, &format!("got network error: {send_error}"));
            }
            Ok(message) => {
                if let Err(e) = handle_message(
                    &our,
                    &mut state,
                    &message,
                    &mut downloads,
                    &mut tmp,
                    &mut auto_updates,
                ) {
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
    _tmp: &mut Directory,
    auto_updates: &mut HashSet<(PackageId, String)>,
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
                    download_from,
                    desired_version_hash,
                } = download_request.clone();

                if download_from.starts_with("http") {
                    // use http_client to GET it
                    Request::to(("our", "http_client", "distro", "sys"))
                        .body(
                            serde_json::to_vec(&client::HttpClientAction::Http(
                                client::OutgoingHttpRequest {
                                    method: "GET".to_string(),
                                    version: None,
                                    url: download_from.clone(),
                                    headers: std::collections::HashMap::new(),
                                },
                            ))
                            .unwrap(),
                        )
                        .context(serde_json::to_vec(&download_request)?)
                        .expects_response(60)
                        .send()?;
                    return Ok(());
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

                Request::to((&download_from, "downloads", "app_store", "sys"))
                    .body(serde_json::to_vec(&DownloadRequests::RemoteDownload(
                        RemoteDownloadRequest {
                            package_id,
                            desired_version_hash,
                            worker_address: our_worker.to_string(),
                        },
                    ))?)
                    .send()?;
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
            DownloadRequests::Progress(progress) => {
                // forward progress to main:app_store:sys,
                // pushed to UI via websockets
                let _ = Request::to(("our", "main", "app_store", "sys"))
                    .body(serde_json::to_vec(&progress)?)
                    .send();
            }
            DownloadRequests::DownloadComplete(req) => {
                if !message.is_local(our) {
                    return Err(anyhow::anyhow!("got non local download complete"));
                }
                // if we have a pending auto_install, forward that context to the main process.
                // it will check if the caps_hashes match (no change in capabilities), and auto_install if it does.

                let context = if auto_updates.remove(&(
                    req.package_id.clone().to_process_lib(),
                    req.version_hash.clone(),
                )) {
                    match get_manifest_hash(
                        req.package_id.clone().to_process_lib(),
                        req.version_hash.clone(),
                    ) {
                        Ok(manifest_hash) => Some(manifest_hash.as_bytes().to_vec()),
                        Err(e) => {
                            print_to_terminal(
                                1,
                                &format!("auto_update: error getting manifest hash: {:?}", e),
                            );
                            None
                        }
                    }
                } else {
                    None
                };

                // pushed to UI via websockets
                let mut request = Request::to(("our", "main", "app_store", "sys"))
                    .body(serde_json::to_vec(&req)?);

                if let Some(ctx) = context {
                    request = request.context(ctx);
                }
                request.send()?;
            }
            DownloadRequests::GetFiles(maybe_id) => {
                // if not local, throw to the boonies.
                // note, can also implement a discovery protocol here in the future
                if !message.is_local(our) {
                    return Err(anyhow::anyhow!("got non local get_files"));
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

                let resp = DownloadResponses::GetFiles(files);

                Response::new().body(serde_json::to_string(&resp)?).send()?;
            }
            DownloadRequests::RemoveFile(remove_req) => {
                if !message.is_local(our) {
                    return Err(anyhow::anyhow!("not local"));
                }
                let RemoveFileRequest {
                    package_id,
                    version_hash,
                } = remove_req;
                let package_dir = format!(
                    "{}/{}",
                    downloads.path,
                    package_id.to_process_lib().to_string()
                );
                let zip_path = format!("{}/{}.zip", package_dir, version_hash);
                let _ = vfs::remove_file(&zip_path, None);
                let manifest_path = format!("{}/{}.json", package_dir, version_hash);
                let _ = vfs::remove_file(&manifest_path, None);
                Response::new()
                    .body(serde_json::to_vec(&Resp::Download(
                        DownloadResponses::Success,
                    ))?)
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
                    add_req.package_id.clone().to_process_lib().to_string()
                );
                let _ = vfs::open_dir(&package_dir, true, None)?;

                // Write the zip file
                let zip_path = format!("{}/{}.zip", package_dir, add_req.version_hash);
                let file = vfs::create_file(&zip_path, None)?;
                file.write(bytes.as_slice())?;

                // Extract and write the manifest
                let manifest_path = format!("{}/{}.json", package_dir, add_req.version_hash);
                extract_and_write_manifest(&bytes, &manifest_path)?;

                // add mirrors if applicable and save:
                if add_req.mirror {
                    state.mirroring.insert(add_req.package_id.to_process_lib());
                    set_state(&serde_json::to_vec(&state)?);
                }

                Response::new()
                    .body(serde_json::to_vec(&Resp::Download(
                        DownloadResponses::Success,
                    ))?)
                    .send()?;
            }
            DownloadRequests::StartMirroring(package_id) => {
                let package_id = package_id.to_process_lib();
                state.mirroring.insert(package_id);
                set_state(&serde_json::to_vec(&state)?);
                Response::new()
                    .body(serde_json::to_vec(&Resp::Download(
                        DownloadResponses::Success,
                    ))?)
                    .send()?;
            }
            DownloadRequests::StopMirroring(package_id) => {
                let package_id = package_id.to_process_lib();
                state.mirroring.remove(&package_id);
                set_state(&serde_json::to_vec(&state)?);
                Response::new()
                    .body(serde_json::to_vec(&Resp::Download(
                        DownloadResponses::Success,
                    ))?)
                    .send()?;
            }
            DownloadRequests::AutoUpdate(auto_update_request) => {
                if !message.is_local(&our)
                    && message.source().process != ProcessId::new(Some("chain"), "app_store", "sys")
                {
                    return Err(anyhow::anyhow!(
                        "got auto-update from non local chain source"
                    ));
                }

                let AutoUpdateRequest {
                    package_id,
                    metadata,
                } = auto_update_request.clone();
                let process_lib_package_id = package_id.clone().to_process_lib();

                // default auto_update to publisher. TODO: more config here.
                let download_from = metadata.properties.publisher;
                let current_version = metadata.properties.current_version;
                let code_hashes = metadata.properties.code_hashes;

                let version_hash = code_hashes
                    .iter()
                    .find(|(version, _)| version == &current_version)
                    .map(|(_, hash)| hash.clone())
                    .ok_or_else(|| anyhow::anyhow!("auto_update: error for package_id: {}, current_version: {}, no matching hash found", process_lib_package_id.to_string(), current_version))?;

                let download_request = LocalDownloadRequest {
                    package_id,
                    download_from,
                    desired_version_hash: version_hash.clone(),
                };

                // kick off local download to ourselves.
                Request::to(("our", "downloads", "app_store", "sys"))
                    .body(serde_json::to_vec(&DownloadRequests::LocalDownload(
                        download_request,
                    ))?)
                    .send()?;

                auto_updates.insert((process_lib_package_id, version_hash));
            }
            _ => {}
        }
    } else {
        match serde_json::from_slice::<Resp>(message.body())? {
            Resp::Download(download_response) => {
                // these are handled in line.
                print_to_terminal(
                    1,
                    &format!("got a weird download response: {:?}", download_response),
                );
            }
            Resp::HttpClient(resp) => {
                let Some(context) = message.context() else {
                    return Err(anyhow::anyhow!("http_client response without context"));
                };
                let download_request = serde_json::from_slice::<LocalDownloadRequest>(context)?;
                if let Ok(client::HttpClientResponse::Http(client::HttpResponse {
                    status, ..
                })) = resp
                {
                    if status == 200 {
                        if let Err(e) = handle_receive_http_download(&download_request) {
                            print_to_terminal(
                                1,
                                &format!("error handling http_client response: {:?}", e),
                            );
                            Request::to(("our", "main", "app_store", "sys"))
                                .body(serde_json::to_vec(&DownloadRequests::DownloadComplete(
                                    DownloadCompleteRequest {
                                        package_id: download_request.package_id.clone(),
                                        version_hash: download_request.desired_version_hash.clone(),
                                        error: Some(e),
                                    },
                                ))?)
                                .send()?;
                        }
                    }
                } else {
                    println!("got http_client error: {resp:?}");
                }
            }
        }
    }
    Ok(())
}

fn handle_receive_http_download(
    download_request: &LocalDownloadRequest,
) -> anyhow::Result<(), DownloadError> {
    let package_id = download_request.package_id.clone().to_process_lib();
    let version_hash = download_request.desired_version_hash.clone();

    print_to_terminal(
        1,
        &format!(
            "Received HTTP download for: {}, with version hash: {}",
            package_id.to_string(),
            version_hash
        ),
    );

    let bytes = get_blob().ok_or(DownloadError::BlobNotFound)?.bytes;

    let package_dir = format!("{}/{}", "/app_store:sys/downloads", package_id.to_string());
    let _ = vfs::open_dir(&package_dir, true, None).map_err(|_| DownloadError::VfsError)?;

    let calculated_hash = format!("{:x}", Sha256::digest(&bytes));
    if calculated_hash != version_hash {
        return Err(DownloadError::HashMismatch(HashMismatch {
            desired: version_hash,
            actual: calculated_hash,
        }));
    }

    // Write the zip file
    let zip_path = format!("{}/{}.zip", package_dir, version_hash);
    let file = vfs::create_file(&zip_path, None).map_err(|_| DownloadError::VfsError)?;
    file.write(bytes.as_slice())
        .map_err(|_| DownloadError::VfsError)?;

    // Write the manifest file
    // Extract and write the manifest
    let manifest_path = format!("{}/{}.json", package_dir, version_hash);
    extract_and_write_manifest(&bytes, &manifest_path).map_err(|_| DownloadError::VfsError)?;

    Request::to(("our", "main", "app_store", "sys"))
        .body(
            serde_json::to_vec(&DownloadCompleteRequest {
                package_id: download_request.package_id.clone(),
                version_hash,
                error: None,
            })
            .unwrap(),
        )
        .send()
        .unwrap();

    Ok(())
}

fn format_entries(entries: Vec<vfs::DirEntry>, state: &State) -> Vec<Entry> {
    entries
        .into_iter()
        .filter_map(|entry| {
            let name = entry.path.split('/').last()?.to_string();
            let is_file = entry.file_type == vfs::FileType::File;

            if is_file && name.ends_with(".zip") {
                let size = vfs::metadata(&entry.path, None)
                    .map(|meta| meta.len)
                    .unwrap_or(0);
                let json_path = entry.path.replace(".zip", ".json");
                let manifest = vfs::open_file(&json_path, false, None)
                    .and_then(|file| file.read_to_string())
                    .unwrap_or_default();

                Some(Entry::File(FileEntry {
                    name,
                    size,
                    manifest,
                }))
            } else if !is_file {
                let mirroring = state.mirroring.iter().any(|pid| {
                    pid.package_name == name
                        || format!("{}:{}", pid.package_name, pid.publisher_node) == name
                });
                Some(Entry::Dir(DirEntry { name, mirroring }))
            } else {
                None // Skip non-zip files
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

            let manifest_file = vfs::open_file(&manifest_path, true, None)?;
            manifest_file.write(contents.as_bytes())?;

            print_to_terminal(1, &format!("Extracted and wrote manifest.json"));
            break;
        }
    }

    Ok(())
}

fn get_manifest_hash(package_id: PackageId, version_hash: String) -> anyhow::Result<String> {
    let package_dir = format!("{}/{}", "/app_store:sys/downloads", package_id.to_string());
    let manifest_path = format!("{}/{}.json", package_dir, version_hash);
    let manifest_file = vfs::open_file(&manifest_path, false, None)?;

    let manifest_bytes = manifest_file.read()?;
    let manifest_hash = keccak_256_hash(&manifest_bytes);
    Ok(manifest_hash)
}

/// generate a Keccak-256 hash string (with 0x prefix) of the metadata bytes
pub fn keccak_256_hash(bytes: &[u8]) -> String {
    use sha3::{Digest, Keccak256};
    let mut hasher = Keccak256::new();
    hasher.update(bytes);
    format!("0x{:x}", hasher.finalize())
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
