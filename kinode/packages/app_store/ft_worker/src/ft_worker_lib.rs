use crate::kinode::process::downloads::{
    DownloadRequests, LocalDownloadRequest, PackageId, RemoteDownloadRequest,
};

use kinode_process_lib::*;

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
        &format!("{}/pkg/ft_worker.wasm", our.package_id()),
        OnExit::None,
        our_capabilities(),
        vec![timer_id],
        false,
    ) else {
        return Err(anyhow::anyhow!("failed to spawn ft_worker!"));
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
        &format!("{}/pkg/ft_worker.wasm", our.package_id()),
        OnExit::None,
        our_capabilities(),
        vec![timer_id],
        false,
    ) else {
        return Err(anyhow::anyhow!("failed to spawn ft_worker!"));
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
