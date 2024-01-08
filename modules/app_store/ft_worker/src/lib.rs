use nectar_process_lib::println;
use nectar_process_lib::*;
use serde::{Deserialize, Serialize};

mod ft_worker_lib;
use ft_worker_lib::*;

wit_bindgen::generate!({
    path: "../../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

/// internal worker protocol
#[derive(Debug, Serialize, Deserialize)]
pub enum FTWorkerProtocol {
    Ready,
    Finished,
}

call_init!(init);

fn init(our: Address) {
    let Ok(Message::Request {
        source: parent_process,
        ipc,
        ..
    }) = await_message()
    else {
        panic!("ft_worker: got bad init message");
    };

    let command = serde_json::from_slice::<FTWorkerCommand>(&ipc)
        .expect("ft_worker: got unparseable init message");

    let Some(result) = (match command {
        FTWorkerCommand::Send {
            target,
            file_name,
            timeout,
        } => Some(handle_send(&our, &target, &file_name, timeout)),
        FTWorkerCommand::Receive {
            file_name,
            total_chunks,
            timeout,
            ..
        } => handle_receive(parent_process, &file_name, total_chunks, timeout),
    }) else {
        return;
    };

    Response::new()
        .ipc(serde_json::to_vec(&result).unwrap())
        .send()
        .unwrap();

    // job is done
}

fn handle_send(our: &Address, target: &Address, file_name: &str, timeout: u64) -> FTWorkerResult {
    let transfer_id: u64 = our.process().parse().unwrap();
    let Some(payload) = get_payload() else {
        println!("ft_worker: wasn't given payload!");
        return FTWorkerResult::Err(TransferError::SourceFailed);
    };
    let file_bytes = payload.bytes;
    let mut file_size = file_bytes.len() as u64;
    let mut offset: u64 = 0;
    let chunk_size: u64 = 1048576; // 1MB, can be changed
    let total_chunks = (file_size as f64 / chunk_size as f64).ceil() as u64;
    // send a file to another worker
    // start by telling target to expect a file,
    // then upon reciving affirmative response,
    // send contents in chunks and wait for
    // acknowledgement.
    let Ok(Ok(response)) = Request::to(target.clone())
        .ipc(
            serde_json::to_vec(&FTWorkerCommand::Receive {
                transfer_id,
                file_name: file_name.to_string(),
                file_size,
                total_chunks,
                timeout,
            })
            .unwrap(),
        )
        .send_and_await_response(timeout)
    else {
        return FTWorkerResult::Err(TransferError::TargetOffline);
    };
    let opp_worker = response.source();
    let Ok(FTWorkerProtocol::Ready) = serde_json::from_slice(&response.ipc()) else {
        return FTWorkerResult::Err(TransferError::TargetRejected);
    };
    // send file in chunks
    loop {
        if file_size < chunk_size {
            // this is the last chunk, so we should expect a Finished response
            let _ = Request::to(opp_worker.clone())
                .ipc(vec![])
                .payload(Payload {
                    mime: None,
                    bytes: file_bytes[offset as usize..offset as usize + file_size as usize]
                        .to_vec(),
                })
                .expects_response(timeout)
                .send();
            break;
        }
        let _ = Request::to(opp_worker.clone())
            .ipc(vec![])
            .payload(Payload {
                mime: None,
                bytes: file_bytes[offset as usize..offset as usize + chunk_size as usize].to_vec(),
            })
            .send();
        file_size -= chunk_size;
        offset += chunk_size;
    }
    // now wait for Finished response
    let Ok(Message::Response { ipc, .. }) = await_message() else {
        return FTWorkerResult::Err(TransferError::TargetRejected);
    };
    let Ok(FTWorkerProtocol::Finished) = serde_json::from_slice(&ipc) else {
        return FTWorkerResult::Err(TransferError::TargetRejected);
    };
    // return success to parent
    return FTWorkerResult::SendSuccess;
}

fn handle_receive(
    parent_process: Address,
    file_name: &str,
    total_chunks: u64,
    timeout: u64,
) -> Option<FTWorkerResult> {
    // send Ready response to counterparty
    Response::new()
        .ipc(serde_json::to_vec(&FTWorkerProtocol::Ready).unwrap())
        .send()
        .unwrap();
    // receive a file from a worker, then send it to parent
    // all messages will be chunks of file. when we receive the
    // last chunk, send a Finished message to sender and Success to parent.
    let mut file_bytes = Vec::new();
    let mut chunks_received = 0;
    let start_time = std::time::Instant::now();
    loop {
        let Ok(Message::Request { .. }) = await_message() else {
            return Some(FTWorkerResult::Err(TransferError::SourceFailed));
        };
        if start_time.elapsed().as_secs() > timeout {
            return Some(FTWorkerResult::Err(TransferError::SourceFailed));
        }
        let Some(payload) = get_payload() else {
            return Some(FTWorkerResult::Err(TransferError::SourceFailed));
        };
        chunks_received += 1;
        file_bytes.extend(payload.bytes);
        if chunks_received == total_chunks {
            break;
        }
    }
    // send Finished message to sender
    Response::new()
        .ipc(serde_json::to_vec(&FTWorkerProtocol::Finished).unwrap())
        .send()
        .unwrap();
    // send Success message to parent
    Request::to(parent_process)
        .ipc(serde_json::to_vec(&FTWorkerResult::ReceiveSuccess(file_name.to_string())).unwrap())
        .payload(Payload {
            mime: None,
            bytes: file_bytes,
        })
        .send()
        .unwrap();
    None
}
