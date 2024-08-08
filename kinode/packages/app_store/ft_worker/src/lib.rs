use crate::kinode::process::downloads::{
    ChunkRequest, DownloadRequest, Downloads, ProgressUpdate, SizeUpdate,
};
use kinode_process_lib::*;
use kinode_process_lib::{println, vfs::open_file, vfs::File, vfs::SeekFrom};
use std::str::FromStr;

pub mod ft_worker_lib;

wit_bindgen::generate!({
    path: "target/wit",
    generate_unused_types: true,
    world: "app-store-sys-v0",
    additional_derives: [serde::Deserialize, serde::Serialize],
});

const CHUNK_SIZE: u64 = 262144; // 256KB

// TODO: Add a timer request that returns to us whenever the timeout time is back.
// If we're still alive at that point for any reason, we are getting purged.

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

    let req: DownloadRequest =
        serde_json::from_slice(&body).expect("ft_worker: got unparseable init message");

    if let Some(node) = req.download_from {
        match handle_sender(&node, &req.package_id.into(), &req.desired_version_hash) {
            Ok(_) => {}
            Err(e) => println!("send error: {}", e),
        }
    } else {
        match handle_receiver(
            &parent_process,
            &req.package_id.into(),
            &req.desired_version_hash,
        ) {
            Ok(_) => {}
            Err(e) => println!("receive error: {}", e),
        }
    }
}

fn handle_sender(node: &str, package_id: &PackageId, version_hash: &str) -> anyhow::Result<()> {
    let target_worker = Address::from_str(node)?;

    let filename = format!(
        "/app_store:sys/downloads/{}:{}-{}.zip",
        package_id.package_name, package_id.publisher_node, version_hash
    );

    let mut file = open_file(&filename, false, None)?;
    let size = file.metadata()?.len;
    let num_chunks = (size as f64 / CHUNK_SIZE as f64).ceil() as u64;

    Request::new()
        .body(serde_json::to_vec(&SizeUpdate {
            package_id: package_id.clone().into(),
            size,
        })?)
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
    // TODO: write to a temporary location first, then check hash as we go, then rename to final location.
    let full_filename = format!(
        "/app_store:sys/downloads/{}:{}-{}.zip",
        package_id.package_name, package_id.publisher_node, version_hash
    );

    let mut file = open_file(&full_filename, true, None)?;

    let mut size: Option<u64> = None;

    loop {
        let Ok(Message::Request { body, .. }) = await_message() else {
            return Err(anyhow::anyhow!("ft_worker: got bad message"));
        };

        let req: Downloads = serde_json::from_slice(&body)?;

        match req {
            Downloads::Chunk(chunk) => {
                handle_chunk(&mut file, &chunk, parent_process, &mut size)?;
                if let Some(s) = size {
                    if chunk.offset + chunk.length >= s {
                        return Ok(());
                    }
                }
            }
            Downloads::Size(update) => {
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
        .body(serde_json::to_vec(&ChunkRequest {
            package_id: package_id.clone().into(),
            version_hash: version_hash.to_string(),
            offset,
            length,
        })?)
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
) -> anyhow::Result<()> {
    let bytes = if let Some(blob) = get_blob() {
        blob.bytes
    } else {
        return Err(anyhow::anyhow!("ft_worker: got no blob"));
    };
    file.write_all(&bytes)?;

    if let Some(total_size) = size {
        // let progress = ((chunk.offset + chunk.length) as f64 / *total_size as f64 * 100.0) as u64;

        Request::new()
            .body(serde_json::to_vec(&ProgressUpdate {
                package_id: chunk.package_id.clone(),
                downloaded: chunk.offset + chunk.length,
                total: *total_size,
                version_hash: chunk.version_hash.clone(),
            })?)
            .target(parent.clone())
            .send()?;
    }

    Ok(())
}

impl crate::kinode::process::main::PackageId {
    pub fn to_process_lib(&self) -> kinode_process_lib::PackageId {
        kinode_process_lib::PackageId::new(&self.package_name, &self.publisher_node)
    }

    pub fn from_process_lib(package_id: &kinode_process_lib::PackageId) -> Self {
        Self {
            package_name: package_id.package_name.clone(),
            publisher_node: package_id.publisher_node.clone(),
        }
    }
}
// Conversion from wit PackageId to process_lib's PackageId
impl From<crate::kinode::process::downloads::PackageId> for kinode_process_lib::PackageId {
    fn from(package_id: crate::kinode::process::downloads::PackageId) -> Self {
        kinode_process_lib::PackageId::new(&package_id.package_name, &package_id.publisher_node)
    }
}

// Conversion from process_lib's PackageId to wit PackageId
impl From<kinode_process_lib::PackageId> for crate::kinode::process::downloads::PackageId {
    fn from(package_id: kinode_process_lib::PackageId) -> Self {
        Self {
            package_name: package_id.package_name,
            publisher_node: package_id.publisher_node,
        }
    }
}
