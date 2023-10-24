use super::bindings::component::uq_process::types::*;
use super::bindings::{print_to_terminal, send_request, spawn, Address, Payload};
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
    Send {
        // make sure to attach file itself as payload
        target: String, // annoying, but this is Address
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
    ReceiveSuccess(String), // name of file, bytes in payload
    Err(TransferError),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum TransferError {
    TargetOffline,
    TargetTimeout,
    TargetRejected,
    SourceFailed,
}

pub fn spawn_transfer(
    our: &Address,
    file_name: &str,
    file_bytes: Option<Vec<u8>>, // if None, expects to inherit payload!
    to_addr: &Address,
) {
    let transfer_id: u64 = rand::random();
    // spawn a worker and tell it to send the file
    let Ok(worker_process_id) = spawn(
        Some(&transfer_id.to_string()),
        "/ft_worker.wasm".into(),
        &OnPanic::None, // can set message-on-panic here
        &Capabilities::All,
        false, // not public
    ) else {
        print_to_terminal(0, "file_transfer: failed to spawn worker!");
        return;
    };
    // tell the worker what to do
    let payload_or_inherit = match file_bytes {
        Some(bytes) => Some(Payload { mime: None, bytes }),
        None => None,
    };
    send_request(
        &Address {
            node: our.node.clone(),
            process: worker_process_id,
        },
        &Request {
            inherit: !payload_or_inherit.is_some(),
            expects_response: Some(61),
            ipc: Some(
                serde_json::to_string(&FTWorkerCommand::Send {
                    target: to_addr.to_string(),
                    file_name: file_name.into(),
                    timeout: 60,
                })
                .unwrap(),
            ),
            metadata: None,
        },
        Some(
            &serde_json::to_string(&FileTransferContext {
                file_name: file_name.into(),
                file_size: match &payload_or_inherit {
                    Some(p) => Some(p.bytes.len() as u64),
                    None => None, // TODO
                },
                start_time: std::time::SystemTime::now(),
            })
            .unwrap(),
        ),
        payload_or_inherit.as_ref(),
    );
}

pub fn spawn_receive_transfer(our: &Address, ipc: &str) {
    let Ok(FTWorkerCommand::Receive { transfer_id, .. }) = serde_json::from_str(ipc) else {
        print_to_terminal(0, "file_transfer: got weird request");
        return;
    };
    let Ok(worker_process_id) = spawn(
        Some(&transfer_id.to_string()),
        "/ft_worker.wasm".into(),
        &OnPanic::None, // can set message-on-panic here
        &Capabilities::All,
        false, // not public
    ) else {
        print_to_terminal(0, "file_transfer: failed to spawn worker!");
        return;
    };
    // forward receive command to worker
    send_request(
        &Address {
            node: our.node.clone(),
            process: worker_process_id,
        },
        &Request {
            inherit: true,
            expects_response: None,
            ipc: Some(ipc.to_string()),
            metadata: None,
        },
        None,
        None,
    );
}
