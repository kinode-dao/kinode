use crate::kinode::process::downloads::{DownloadRequest, PackageId};

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
    let Ok(worker_process_id) = spawn(
        Some(&transfer_id.to_string()),
        &format!("{}/pkg/ft_worker.wasm", our.package_id()),
        OnExit::None,
        our_capabilities(),
        vec![],
        false,
    ) else {
        return Err(anyhow::anyhow!("failed to spawn ft_worker!"));
    };

    let req = Request::new()
        .target((&our.node, worker_process_id))
        .expects_response(timeout + 1)
        .body(
            serde_json::to_vec(&DownloadRequest {
                package_id: package_id.clone(),
                desired_version_hash: version_hash.to_string(),
                download_from: Some(to_addr.to_string()),
            })
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
    timeout: u64,
) -> anyhow::Result<Address> {
    let transfer_id: u64 = rand::random();
    let Ok(worker_process_id) = spawn(
        Some(&transfer_id.to_string()),
        &format!("{}/pkg/ft_worker.wasm", our.package_id()),
        OnExit::None,
        our_capabilities(),
        vec![],
        false,
    ) else {
        return Err(anyhow::anyhow!("failed to spawn ft_worker!"));
    };

    let req = Request::new()
        .target((&our.node, worker_process_id.clone()))
        .expects_response(timeout + 1)
        .body(
            serde_json::to_vec(&DownloadRequest {
                package_id: package_id.clone(),
                desired_version_hash: version_hash.to_string(),
                download_from: None,
            })
            .unwrap(),
        );

    req.send()?;
    Ok(Address::new(&our.node, worker_process_id))
}
