use kinode_process_lib::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct FileTransferContext {
    pub file_name: String,
    pub file_size: Option<u64>,
    pub start_time: std::time::SystemTime,
}

/// sent as first Request to a newly spawned worker
/// the Receive command will be sent out to target
/// in order to prompt them to spawn a worker
#[derive(Debug, Serialize, Deserialize)]
pub enum FTWorkerCommand {
    /// make sure to attach file itself as blob
    Send {
        target: Address,
        file_name: String,
        timeout: u64,
    },
    Receive {
        transfer_id: u64,
        file_name: String,
        file_size: u64,
        total_chunks: u64,
        timeout: u64,
    },
}

/// sent as Response by worker to its parent
#[derive(Debug, Serialize, Deserialize)]
pub enum FTWorkerResult {
    SendSuccess,
    /// string is name of file. bytes in blob
    ReceiveSuccess(String),
    Err(TransferError),
}

/// the possible errors that can be returned to the parent inside `FTWorkerResult`
#[derive(Debug, Serialize, Deserialize)]
pub enum TransferError {
    TargetOffline,
    TargetTimeout,
    TargetRejected,
    SourceFailed,
}

/// A helper function to spawn a worker and initialize a file transfer.
/// The outcome will be sent as an [`FTWorkerResult`] to the caller process.
///
/// if `file_bytes` is None, expects to inherit blob!
#[allow(dead_code)]
pub fn spawn_transfer(
    our: &Address,
    file_name: &str,
    file_bytes: Option<Vec<u8>>,
    timeout: u64,
    to_addr: &Address,
) -> anyhow::Result<()> {
    let transfer_id: u64 = rand::random();
    // spawn a worker and tell it to send the file
    let Ok(worker_process_id) = spawn(
        Some(&transfer_id.to_string()),
        &format!("{}/pkg/ft_worker.wasm", our.package_id()),
        OnExit::None, // can set message-on-panic here
        our_capabilities(),
        vec![],
        false, // not public
    ) else {
        return Err(anyhow::anyhow!("failed to spawn ft_worker!"));
    };
    // tell the worker what to do
    let blob_or_inherit = match file_bytes {
        Some(bytes) => Some(LazyLoadBlob { mime: None, bytes }),
        None => None,
    };
    let mut req = Request::new()
        .target((our.node.as_ref(), worker_process_id))
        .inherit(!blob_or_inherit.is_some())
        .expects_response(timeout + 1) // don't call with 2^64 lol
        .body(
            serde_json::to_vec(&FTWorkerCommand::Send {
                target: to_addr.clone(),
                file_name: file_name.into(),
                timeout,
            })
            .unwrap(),
        )
        .context(
            serde_json::to_vec(&FileTransferContext {
                file_name: file_name.into(),
                file_size: match &blob_or_inherit {
                    Some(p) => Some(p.bytes.len() as u64),
                    None => None, // TODO
                },
                start_time: std::time::SystemTime::now(),
            })
            .unwrap(),
        );

    if let Some(blob) = blob_or_inherit {
        req = req.blob(blob);
    }
    req.send()
}

/// A helper function to allow a process to easily handle an incoming transfer
/// from an ft_worker. Call this when you get the initial [`FTWorkerCommand::Receive`]
/// and let it do the rest. The outcome will be sent as an [`FTWorkerResult`] inside
/// a Response to the caller.
#[allow(dead_code)]
pub fn spawn_receive_transfer(our: &Address, body: &[u8]) -> anyhow::Result<()> {
    let Ok(FTWorkerCommand::Receive { transfer_id, .. }) = serde_json::from_slice(body) else {
        return Err(anyhow::anyhow!(
            "spawn_receive_transfer: got malformed request"
        ));
    };
    let Ok(worker_process_id) = spawn(
        Some(&transfer_id.to_string()),
        &format!("{}/pkg/ft_worker.wasm", our.package_id()),
        OnExit::None, // can set message-on-panic here
        our_capabilities(),
        vec![],
        false, // not public
    ) else {
        return Err(anyhow::anyhow!("failed to spawn ft_worker!"));
    };
    // forward receive command to worker
    Request::new()
        .target((our.node.as_ref(), worker_process_id))
        .inherit(true)
        .body(body)
        .send()
}
