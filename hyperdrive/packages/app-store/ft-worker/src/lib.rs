//! {ft_worker_id}:app-store:sys
//! This process implements the file transfer functionality for the App Store system in the Hyperware ecosystem.
//! It handles the chunked transfer of app package files between nodes, including download initiation,
//! progress tracking, and integrity verification.
//!
//! ## Key Components:
//!
//! - `init`: The entry point for the worker process, handling both local and remote download requests.
//! - `handle_sender`: Manages the sending of file chunks to a target worker.
//! - `handle_receiver`: Manages the receiving of file chunks and assembles the complete file.
//! - `send_chunk`: Sends individual chunks of a file to the target.
//! - `handle_chunk`: Processes received chunks, updates progress, and verifies file integrity.
//!
//! ## Workflow:
//!
//! 1. The worker is initialized with either a local or remote download request.
//! 2. For sending:
//!    - The file is opened and its size is determined.
//!    - The file is split into chunks and sent sequentially.
//!    - Progress updates are sent after each chunk.
//! 3. For receiving:
//!    - A new file is created to store the incoming data.
//!    - Chunks are received and written to the file.
//!    - The file's integrity is verified using a SHA256 hash.
//!    - The manifest is extracted and saved separately.
//! 4. Upon completion or error, a status message is sent to the parent process.
//!
//! ## Error Handling:
//!
//! - Hash mismatches between the received file and the expected hash are detected and reported.
//! - Various I/O errors are caught and propagated.
//! - A 120 second killswitch is implemented to clean up dangling transfers.
//!
//! ## Integration with App Store:
//!
//! This worker process is spawned by the main downloads process of the App Store system.
//! It uses the `DownloadRequest` and related types from the app store's API to communicate
//! with other components of the system.
//!
//! Note: This implementation uses a fixed chunk size of 256KB for file transfers.
//!
use crate::hyperware::process::downloads::{
    ChunkRequest, DownloadCompleteRequest, DownloadError, DownloadRequest, HashMismatch,
    LocalDownloadRequest, ProgressUpdate, RemoteDownloadRequest, SizeUpdate,
};
use hyperware_process_lib::*;
use hyperware_process_lib::{
    print_to_terminal, println, timer,
    vfs::{File, SeekFrom},
};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::str::FromStr;

pub mod ft_worker_lib;

wit_bindgen::generate!({
    path: "target/wit",
    generate_unused_types: true,
    world: "app-store-sys-v1",
    additional_derives: [serde::Deserialize, serde::Serialize, process_macros::SerdeJsonInto],
});

const CHUNK_SIZE: u64 = 262144; // 256KB
const KILL_SWITCH_MS: u64 = 120000; // 2 minutes

call_init!(init);
fn init(our: Address) {
    let Ok(Message::Request {
        source: parent_process,
        body,
        ..
    }) = await_message()
    else {
        panic!("ft_worker: got bad init message");
    };

    if parent_process.node() != our.node() {
        panic!("ft_worker: got bad init message source");
    }

    // killswitch timer, 2 minutes. sender or receiver gets killed/cleaned up.
    timer::set_timer(KILL_SWITCH_MS, None);

    let start = std::time::Instant::now();

    match body
        .try_into()
        .expect("ft_worker: got unparseable init message")
    {
        DownloadRequest::LocalDownload(local_request) => {
            let LocalDownloadRequest {
                package_id,
                desired_version_hash,
                ..
            } = local_request;
            match handle_receiver(
                &parent_process,
                &package_id.to_process_lib(),
                &desired_version_hash,
            ) {
                Ok(_) => print_to_terminal(
                    1,
                    &format!(
                        "ft_worker: received downloaded package in {}ms",
                        start.elapsed().as_millis()
                    ),
                ),
                Err(e) => {
                    print_to_terminal(1, &format!("ft_worker: receive error: {}", e));
                    // fallback bubble up to parent.
                    Request::new()
                        .body(DownloadRequest::DownloadComplete(DownloadCompleteRequest {
                            package_id: package_id.clone().into(),
                            version_hash: desired_version_hash.to_string(),
                            err: Some(DownloadError::WorkerSpawnFailed),
                        }))
                        .target(parent_process)
                        .send()
                        .unwrap();
                }
            }
        }
        DownloadRequest::RemoteDownload(remote_request) => {
            let RemoteDownloadRequest {
                package_id,
                desired_version_hash,
                worker_address,
            } = remote_request;

            match handle_sender(
                &worker_address,
                &package_id.to_process_lib(),
                &desired_version_hash,
            ) {
                Ok(_) => print_to_terminal(
                    1,
                    &format!(
                        "ft_worker: sent package to {} in {}ms",
                        worker_address,
                        start.elapsed().as_millis()
                    ),
                ),
                Err(e) => print_to_terminal(1, &format!("ft_worker: send error: {}", e)),
            }
        }
        _ => println!("ft_worker: got unexpected message"),
    }
}

fn handle_sender(worker: &str, package_id: &PackageId, version_hash: &str) -> anyhow::Result<()> {
    let target_worker = Address::from_str(worker)?;

    let filename = format!(
        "/app-store:sys/downloads/{}:{}/{}.zip",
        package_id.package_name, package_id.publisher_node, version_hash
    );

    let mut file = vfs::open_file(&filename, false, None)?;
    let size = file.metadata()?.len;
    let num_chunks = (size as f64 / CHUNK_SIZE as f64).ceil() as u64;

    Request::new()
        .body(DownloadRequest::Size(SizeUpdate {
            package_id: package_id.clone().into(),
            size,
        }))
        .target(target_worker.clone())
        .send()?;
    file.seek(SeekFrom::Start(0))?;

    for i in 0..num_chunks {
        send_chunk(&mut file, i, size, &target_worker, package_id, version_hash)?;
    }

    Ok(())
}

fn handle_receiver(
    parent_process: &Address,
    package_id: &PackageId,
    version_hash: &str,
) -> anyhow::Result<()> {
    let timer_address = Address::from_str("our@timer:distro:sys")?;

    let mut file: Option<File> = None;
    let mut size: Option<u64> = None;
    let mut hasher = Sha256::new();

    let package_dir = vfs::open_dir(
        &format!(
            "/app-store:sys/downloads/{}:{}/",
            package_id.package_name,
            package_id.publisher(),
        ),
        true,
        None,
    )?;

    loop {
        let message = await_message()?;
        if *message.source() == timer_address {
            // send error message to downloads process
            Request::new()
                .body(DownloadRequest::DownloadComplete(DownloadCompleteRequest {
                    package_id: package_id.clone().into(),
                    version_hash: version_hash.to_string(),
                    err: Some(DownloadError::Timeout),
                }))
                .target(parent_process.clone())
                .send()?;
            return Ok(());
        }
        if !message.is_request() {
            return Err(anyhow::anyhow!("ft_worker: got bad message"));
        }

        match message.body().try_into()? {
            DownloadRequest::Chunk(chunk) => {
                let bytes = if let Some(blob) = get_blob() {
                    blob.bytes
                } else {
                    return Err(anyhow::anyhow!("ft_worker: got no blob in chunk request"));
                };

                if file.is_none() {
                    file = Some(vfs::open_file(
                        &format!("{}{}.zip", &package_dir.path, version_hash),
                        true,
                        None,
                    )?);
                }

                handle_chunk(
                    file.as_mut().unwrap(),
                    &chunk,
                    parent_process,
                    &mut size,
                    &mut hasher,
                    &bytes,
                )?;
                if let Some(s) = size {
                    if chunk.offset + chunk.length >= s {
                        let recieved_hash = format!("{:x}", hasher.finalize());

                        if recieved_hash != version_hash {
                            print_to_terminal(
                                1,
                                &format!(
                                    "ft_worker: {} hash mismatch: desired: {} != actual: {}",
                                    package_id.to_string(),
                                    version_hash,
                                    recieved_hash
                                ),
                            );
                            let req = DownloadCompleteRequest {
                                package_id: package_id.clone().into(),
                                version_hash: version_hash.to_string(),
                                err: Some(DownloadError::HashMismatch(HashMismatch {
                                    desired: version_hash.to_string(),
                                    actual: recieved_hash,
                                })),
                            };
                            Request::new()
                                .body(DownloadRequest::DownloadComplete(req))
                                .target(parent_process.clone())
                                .send()?;
                        }

                        let manifest_filename =
                            format!("{}{}.json", package_dir.path, version_hash);

                        let contents = file.as_mut().unwrap().read()?;
                        extract_and_write_manifest(&contents, &manifest_filename)?;

                        Request::new()
                            .body(DownloadRequest::DownloadComplete(DownloadCompleteRequest {
                                package_id: package_id.clone().into(),
                                version_hash: version_hash.to_string(),
                                err: None,
                            }))
                            .target(parent_process.clone())
                            .send()?;
                        return Ok(());
                    }
                }
            }
            DownloadRequest::Size(update) => {
                size = Some(update.size);
            }
            _ => println!("ft_worker: got unexpected message"),
        }
    }
}

fn send_chunk(
    file: &mut File,
    chunk_index: u64,
    total_size: u64,
    target: &Address,
    package_id: &PackageId,
    version_hash: &str,
) -> anyhow::Result<()> {
    let offset = chunk_index * CHUNK_SIZE;
    let length = CHUNK_SIZE.min(total_size - offset);

    let mut buffer = vec![0; length as usize];
    // this extra seek might be unnecessary. fix multireads per process in vfs
    file.seek(SeekFrom::Start(offset))?;
    file.read_at(&mut buffer)?;

    Request::new()
        .body(DownloadRequest::Chunk(ChunkRequest {
            package_id: package_id.clone().into(),
            version_hash: version_hash.to_string(),
            offset,
            length,
        }))
        .target(target.clone())
        .blob_bytes(buffer)
        .send()?;
    Ok(())
}

fn handle_chunk(
    file: &mut File,
    chunk: &ChunkRequest,
    parent: &Address,
    size: &mut Option<u64>,
    hasher: &mut Sha256,
    bytes: &[u8],
) -> anyhow::Result<()> {
    file.write_all(bytes)?;
    hasher.update(bytes);

    if let Some(total_size) = size {
        // let progress = ((chunk.offset + chunk.length) as f64 / *total_size as f64 * 100.0) as u64;

        Request::new()
            .body(DownloadRequest::Progress(ProgressUpdate {
                package_id: chunk.package_id.clone(),
                downloaded: chunk.offset + chunk.length,
                total: *total_size,
                version_hash: chunk.version_hash.clone(),
            }))
            .target(parent.clone())
            .send()?;
    }

    Ok(())
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

            print_to_terminal(1, "Extracted and wrote manifest.json");
            break;
        }
    }

    Ok(())
}

impl crate::hyperware::process::main::PackageId {
    pub fn to_process_lib(&self) -> hyperware_process_lib::PackageId {
        hyperware_process_lib::PackageId::new(&self.package_name, &self.publisher_node)
    }

    pub fn from_process_lib(package_id: &hyperware_process_lib::PackageId) -> Self {
        Self {
            package_name: package_id.package_name.clone(),
            publisher_node: package_id.publisher_node.clone(),
        }
    }
}

// Conversion from wit PackageId to process_lib's PackageId
impl From<crate::hyperware::process::downloads::PackageId> for hyperware_process_lib::PackageId {
    fn from(package_id: crate::hyperware::process::downloads::PackageId) -> Self {
        hyperware_process_lib::PackageId::new(&package_id.package_name, &package_id.publisher_node)
    }
}

// Conversion from process_lib's PackageId to wit PackageId
impl From<hyperware_process_lib::PackageId> for crate::hyperware::process::downloads::PackageId {
    fn from(package_id: hyperware_process_lib::PackageId) -> Self {
        Self {
            package_name: package_id.package_name,
            publisher_node: package_id.publisher_node,
        }
    }
}
