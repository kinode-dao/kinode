//! Helper functions for spawning file transfer workers.
//! These functions are used to initiate send and receive operations
//! for file transfers in the App Store system
//!
use crate::kinode::process::downloads::{
    DownloadRequests, LocalDownloadRequest, PackageId, RemoteDownloadRequest,
};

use kinode_process_lib::*;

/// Spawns a worker process to send a file transfer.
///
/// This function creates a new worker process, configures it for sending a file,
/// and initiates the transfer to the specified address.
#[allow(dead_code)]
pub fn spawn_send_transfer(
    our: &Address,
    package_id: &PackageId,
    version_hash: &str,
    timeout: u64,
    to_addr: &Address,
) -> anyhow::Result<()> {
    let transfer_id: u64 = rand::random();
    let timer_id = ProcessId::new(Some("timer"), "distro", "sys");
    let Ok(worker_process_id) = spawn(
        Some(&transfer_id.to_string()),
        &format!("{}/pkg/ft-worker.wasm", our.package_id()),
        OnExit::None,
        our_capabilities(),
        vec![timer_id],
        false,
    ) else {
        return Err(anyhow::anyhow!("failed to spawn ft-worker!"));
    };

    let req = Request::new()
        .target((&our.node, worker_process_id))
        .expects_response(timeout + 1)
        .body(
            serde_json::to_vec(&DownloadRequests::RemoteDownload(RemoteDownloadRequest {
                package_id: package_id.clone(),
                desired_version_hash: version_hash.to_string(),
                worker_address: to_addr.to_string(),
            }))
            .unwrap(),
        );
    req.send()?;
    Ok(())
}

/// Spawns a worker process to receive a file transfer.
///
/// This function creates a new worker process, configures it to receive a file
/// from the specified node, and prepares it to handle the incoming transfer.
#[allow(dead_code)]
pub fn spawn_receive_transfer(
    our: &Address,
    package_id: &PackageId,
    version_hash: &str,
    from_node: &str,
    timeout: u64,
) -> anyhow::Result<Address> {
    let transfer_id: u64 = rand::random();
    let timer_id = ProcessId::new(Some("timer"), "distro", "sys");
    let Ok(worker_process_id) = spawn(
        Some(&transfer_id.to_string()),
        &format!("{}/pkg/ft-worker.wasm", our.package_id()),
        OnExit::None,
        our_capabilities(),
        vec![timer_id],
        false,
    ) else {
        return Err(anyhow::anyhow!("failed to spawn ft-worker!"));
    };

    let req = Request::new()
        .target((&our.node, worker_process_id.clone()))
        .expects_response(timeout + 1)
        .body(
            serde_json::to_vec(&DownloadRequests::LocalDownload(LocalDownloadRequest {
                package_id: package_id.clone(),
                desired_version_hash: version_hash.to_string(),
                download_from: from_node.to_string(),
            }))
            .unwrap(),
        );

    req.send()?;
    Ok(Address::new(&our.node, worker_process_id))
}
