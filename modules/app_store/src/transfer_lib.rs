use super::bindings::component::uq_process::types::*;
use crate::bindings::{get_payload, receive, send_request, send_response};
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum TransferError {
    // in all errors, u64 is number of bytes successfully transferred
    TargetOffline(u64),
    TargetTimeout(u64),
    TargetRejected(u64),
    SourceFailed(u64),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum TransferMetadata {
    Begin {
        file_name: String,
        file_size: u64,
        total_chunks: u64,
    },
}

pub fn transfer(
    to_addr: Address,
    bytes: Vec<u8>,
    max_timeout: u64,
) -> (
    Result<(), TransferError>,
    Vec<Result<(Address, Message), (SendError, Option<Context>)>>,
) {
    let transfer_context_id: u64 = rand::random();
    let mut bytes_remaining: u64 = bytes.len() as u64;
    let mut offset: u64 = 0;
    let mut chunk_size: u64 = 1048576; // 1MB
    let mut chunks_sent = 0;
    let total_chunks = (bytes.len() as f64 / chunk_size as f64).ceil() as u64;
    loop {
        chunks_sent += 1;
        if bytes_remaining < chunk_size {
            chunk_size = bytes_remaining;
        }
        let payload = Payload {
            mime: None,
            bytes: bytes[offset as usize..offset as usize + chunk_size as usize].to_vec(),
        };
        send_request(
            &to_addr,
            &Request {
                inherit: false,
                expects_response: Some(max_timeout),
                ipc: None,
                metadata: Some(if chunks_sent == 1 {
                    serde_json::to_string(&TransferMetadata::Begin {
                        file_name: "test".to_string(),
                        file_size: bytes.len() as u64,
                        total_chunks,
                    })
                    .unwrap()
                } else {
                    chunks_sent.to_string()
                }),
            },
            Some(&&transfer_context_id.to_string()),
            Some(&payload),
        );
        bytes_remaining -= chunk_size;
        offset += chunk_size;
        if bytes_remaining == 0 {
            break;
        }
    }
    let mut chunks_confirmed = 0;
    let mut non_transfer_message_queue = Vec::new();
    loop {
        let next = receive();
        if let Err((send_error, context)) = &next {
            match context {
                Some(_) => match send_error.kind {
                    SendErrorKind::Offline => {
                        return (
                            Err(TransferError::TargetOffline(chunks_confirmed * chunk_size)),
                            non_transfer_message_queue,
                        )
                    }
                    SendErrorKind::Timeout => {
                        return (
                            Err(TransferError::TargetTimeout(chunks_confirmed * chunk_size)),
                            non_transfer_message_queue,
                        )
                    }
                },
                None => {
                    non_transfer_message_queue.push(next);
                    continue;
                }
            }
        }
        if let Ok((source, message)) = &next {
            if source.process == to_addr.process {
                match message {
                    Message::Request(_) => {
                        non_transfer_message_queue.push(next);
                        continue;
                    }
                    Message::Response((response, context)) => {
                        if transfer_context_id
                            == context
                                .as_ref()
                                .unwrap_or(&"".into())
                                .parse::<u64>()
                                .unwrap_or(0)
                        {
                            chunks_confirmed += 1;
                            if response
                                .metadata
                                .as_ref()
                                .unwrap_or(&"".into())
                                .parse::<u64>()
                                .unwrap_or(0)
                                != chunks_confirmed
                            {
                                return (
                                    Err(TransferError::TargetRejected(
                                        chunks_confirmed * chunk_size,
                                    )),
                                    non_transfer_message_queue,
                                );
                            }
                            if chunks_confirmed == chunks_sent {
                                return (Ok(()), non_transfer_message_queue);
                            }
                        } else {
                            non_transfer_message_queue.push(next);
                        }
                    }
                }
            } else {
                non_transfer_message_queue.push(next);
                continue;
            }
        }
    }
}

pub fn receive_transfer(
    transfer_source: Address,
    total_chunks: u64,
    max_timeout: u64,
) -> (
    Result<Vec<u8>, TransferError>,
    Vec<Result<(Address, Message), (SendError, Option<Context>)>>,
) {
    let start_time = std::time::SystemTime::now();
    // get first payload then loop and receive rest
    let mut file = match get_payload() {
        Some(payload) => payload.bytes,
        None => {
            return (Err(TransferError::SourceFailed(0)), vec![]);
        }
    };
    // respond to first request
    send_response(
        &Response {
            inherit: false,
            ipc: None,
            metadata: Some(1.to_string()),
        },
        None,
    );
    if total_chunks == 1 {
        return (Ok(file), vec![]);
    }
    let mut chunk_num = 1;
    let mut non_transfer_message_queue = Vec::new();
    loop {
        let next = receive();
        if start_time.elapsed().expect("time error").as_secs() > max_timeout {
            return (
                Err(TransferError::TargetTimeout(file.len() as u64)),
                non_transfer_message_queue,
            );
        }
        if let Err(_) = &next {
            non_transfer_message_queue.push(next);
        } else if let Ok((source, message)) = &next {
            // we know all messages from source process will be for this transfer,
            // since they are sent sequentially and it's a single-file queue.
            if source.process == transfer_source.process {
                match message {
                    Message::Request(_) => {
                        let payload = match get_payload() {
                            Some(payload) => payload,
                            None => {
                                return (
                                    Err(TransferError::SourceFailed(file.len() as u64)),
                                    non_transfer_message_queue,
                                );
                            }
                        };
                        chunk_num += 1;
                        file.extend(payload.bytes);
                        send_response(
                            &Response {
                                inherit: false,
                                ipc: None,
                                metadata: Some(chunk_num.to_string()),
                            },
                            None,
                        );
                        if chunk_num == total_chunks {
                            return (Ok(file), non_transfer_message_queue);
                        }
                    }
                    Message::Response(_) => {
                        return (
                            Err(TransferError::SourceFailed(file.len() as u64)),
                            non_transfer_message_queue,
                        );
                    }
                }
            } else {
                non_transfer_message_queue.push(next);
                continue;
            }
        }
    }
}
