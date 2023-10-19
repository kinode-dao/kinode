cargo_component_bindings::generate!();
use bindings::component::uq_process::types::*;
use bindings::{get_payload, print_to_terminal, receive, send_request, send_response, Guest};
use serde::{Deserialize, Serialize};

struct Component;

mod ft_worker_lib;
#[allow(dead_code)]
mod process_lib;
use ft_worker_lib::*;

/// internal worker protocol
#[derive(Debug, Serialize, Deserialize)]
pub enum FTWorkerProtocol {
    Ready,
    Finished,
}

impl Guest for Component {
    fn init(our: Address) {
        print_to_terminal(1, &format!("{}: start", our.process));

        let Ok((parent_process, Message::Request(req))) = receive() else {
            panic!("ft_worker: got bad init message");
        };

        let command = serde_json::from_str::<FTWorkerCommand>(
            &req.ipc.expect("ft_worker: got empty init message"),
        )
        .expect("ft_worker: got unparseable init message");

        match command {
            FTWorkerCommand::Send {
                target,
                file_name,
                timeout,
            } => {
                let transfer_id: u64 = our.process.process().parse().unwrap();
                let Some(payload) = get_payload() else {
                    panic!("ft_worker: got empty payload");
                };
                let file_bytes = payload.bytes;
                let mut file_size = file_bytes.len() as u64;
                let mut offset: u64 = 0;
                let mut chunk_size: u64 = 1048576; // 1MB
                let total_chunks = (file_size as f64 / chunk_size as f64).ceil() as u64;
                // send a file to another worker
                // start by telling target to expect a file,
                // then upon reciving affirmative response,
                // send contents in chunks and wait for
                // acknowledgement.
                match bindings::send_and_await_response(
                    &Address::from_str(&target).unwrap(),
                    &Request {
                        inherit: false,
                        expects_response: Some(timeout),
                        ipc: Some(
                            serde_json::to_string(&FTWorkerCommand::Receive {
                                transfer_id,
                                file_name,
                                file_size,
                                total_chunks,
                                timeout,
                            })
                            .unwrap(),
                        ),
                        metadata: None,
                    },
                    None,
                ) {
                    Err(send_error) => {
                        respond_to_parent(FTWorkerResult::Err(match send_error.kind {
                            SendErrorKind::Offline => TransferError::TargetOffline,
                            SendErrorKind::Timeout => TransferError::TargetTimeout,
                        }))
                    }
                    Ok((opp_worker, Message::Response((response, _)))) => {
                        let Ok(FTWorkerProtocol::Ready) = serde_json::from_str(&response.ipc.expect("ft_worker: got empty response")) else {
                            respond_to_parent(FTWorkerResult::Err(TransferError::TargetRejected));
                            return;
                        };
                        // send file in chunks
                        loop {
                            if file_size < chunk_size {
                                // this is the last chunk, so we should expect a Finished response
                                chunk_size = file_size;
                                let payload = Payload {
                                    mime: None,
                                    bytes: file_bytes
                                        [offset as usize..offset as usize + chunk_size as usize]
                                        .to_vec(),
                                };
                                send_request(
                                    &opp_worker,
                                    &Request {
                                        inherit: false,
                                        expects_response: Some(timeout),
                                        ipc: None,
                                        metadata: None,
                                    },
                                    None,
                                    Some(&payload),
                                );
                                break;
                            }
                            let payload = Payload {
                                mime: None,
                                bytes: file_bytes
                                    [offset as usize..offset as usize + chunk_size as usize]
                                    .to_vec(),
                            };
                            send_request(
                                &opp_worker,
                                &Request {
                                    inherit: false,
                                    expects_response: None,
                                    ipc: None,
                                    metadata: None,
                                },
                                None,
                                Some(&payload),
                            );
                            file_size -= chunk_size;
                            offset += chunk_size;
                        }
                        // now wait for Finished response
                        let Ok((receiving_worker, Message::Response((resp, _)))) = receive() else {
                            respond_to_parent(FTWorkerResult::Err(TransferError::TargetRejected));
                            return;
                        };
                        let Ok(FTWorkerProtocol::Finished) = serde_json::from_str(
                            &resp.ipc.expect("ft_worker: got empty response"),
                        ) else {
                            respond_to_parent(FTWorkerResult::Err(TransferError::TargetRejected));
                            return;
                        };
                        // return success to parent
                        respond_to_parent(FTWorkerResult::SendSuccess);
                    }
                    _ => respond_to_parent(FTWorkerResult::Err(TransferError::TargetRejected)),
                }
            }
            FTWorkerCommand::Receive {
                transfer_id,
                file_name,
                file_size,
                total_chunks,
                timeout,
            } => {
                // send Ready response to counterparty
                send_response(
                    &Response {
                        inherit: false,
                        ipc: Some(serde_json::to_string(&FTWorkerProtocol::Ready).unwrap()),
                        metadata: None,
                    },
                    None,
                );
                // receive a file from a worker, then send it to parent
                // all messages will be chunks of file. when we receive the
                // last chunk, send a Finished message to sender and Success to parent.
                let mut file_bytes = Vec::new();
                let mut chunks_received = 0;
                let start_time = std::time::Instant::now();
                loop {
                    let Ok((source, Message::Request(req))) = receive() else {
                        respond_to_parent(FTWorkerResult::Err(TransferError::SourceFailed));
                        return;
                    };
                    if start_time.elapsed().as_secs() > timeout {
                        respond_to_parent(FTWorkerResult::Err(TransferError::SourceFailed));
                        return;
                    }
                    let Some(payload) = get_payload() else {
                        respond_to_parent(FTWorkerResult::Err(TransferError::SourceFailed));
                        return;
                    };
                    chunks_received += 1;
                    file_bytes.extend(payload.bytes);
                    if chunks_received == total_chunks {
                        break;
                    }
                }
                // send Finished message to sender
                send_response(
                    &Response {
                        inherit: false,
                        ipc: Some(serde_json::to_string(&FTWorkerProtocol::Finished).unwrap()),
                        metadata: None,
                    },
                    None,
                );
                // send Success message to parent
                send_request(
                    &parent_process,
                    &Request {
                        inherit: false,
                        expects_response: None,
                        ipc: Some(
                            serde_json::to_string(&FTWorkerResult::ReceiveSuccess(file_name))
                                .unwrap(),
                        ),
                        metadata: None,
                    },
                    None,
                    Some(&Payload {
                        mime: None,
                        bytes: file_bytes,
                    }),
                );
            }
        }
    }
}

fn respond_to_parent(result: FTWorkerResult) {
    send_response(
        &Response {
            inherit: false,
            ipc: Some(serde_json::to_string(&result).unwrap()),
            metadata: None,
        },
        None,
    );
}
