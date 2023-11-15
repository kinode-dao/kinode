use crate::types::{
    Address, FsAction, FsError, FsResponse, KernelMessage, Message, MessageReceiver, MessageSender,
    Payload, PrintSender, Printout, Request, Response, FILESYSTEM_PROCESS_ID, TIMER_PROCESS_ID,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A runtime module that allows processes to set timers. Interacting with the
/// timer is done with a simple Request/Response pattern, and the timer module
/// is public, so it can be used by any local process. It will not respond to
/// requests made by other nodes.
///
/// The interface of the timer module is as follows:
/// One kind of request is accepted: the IPC must be a little-endian byte-representation
/// of an unsigned 64-bit integer, in seconds. This request should always expect a Response.
/// If the request does not expect a Response, the timer will not be set.
///
/// A proper Request will trigger the timer module to send a Response. The Response will be
/// empty, so the user should either `send_and_await` the Request, or attach a `context` so
/// they can match the Response with their purpose.
///
pub async fn timer_service(
    our: String,
    kernel_message_sender: MessageSender,
    mut timer_message_receiver: MessageReceiver,
    print_tx: PrintSender,
) -> Result<()> {
    // if we have a persisted state file, load it
    let mut timer_map =
        match load_state_from_reboot(&our, &kernel_message_sender, &mut timer_message_receiver)
            .await
        {
            Ok(timer_map) => timer_map,
            Err(e) => {
                let _ = print_tx
                    .send(Printout {
                        verbosity: 1,
                        content: format!("Failed to load state from reboot: {:?}", e),
                    })
                    .await;
                TimerMap {
                    timers: BTreeMap::new(),
                }
            }
        };
    // for any persisted timers that have popped, send their responses
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    for (id, addr) in timer_map.drain_expired(now) {
        let _ = kernel_message_sender
            .send(KernelMessage {
                id,
                source: Address {
                    node: our.clone(),
                    process: TIMER_PROCESS_ID.clone(),
                },
                target: addr,
                rsvp: None,
                message: Message::Response((
                    Response {
                        inherit: false,
                        ipc: vec![],
                        metadata: None,
                    },
                    None,
                )),
                payload: None,
                signed_capabilities: None,
            })
            .await;
    }
    // and then re-persist the new state of the timer map
    persist_state(&our, &timer_map, &kernel_message_sender).await;
    // joinset holds active in-mem timers
    let mut timer_tasks = tokio::task::JoinSet::<u64>::new();
    loop {
        tokio::select! {
            Some(km) = timer_message_receiver.recv() => {
                // ignore Requests sent from other nodes
                if km.source.node != our { continue };
                // we only handle Requests which contain a little-endian u64 as IPC,
                // except for a special "debug" message, which prints the current state
                let Message::Request(req) = km.message else { continue };
                if req.ipc == "debug".as_bytes() {
                    let _ = print_tx.send(Printout {
                        verbosity: 0,
                        content: format!("timer service active timers ({}):", timer_map.timers.len()),
                    }).await;
                    for (k, v) in timer_map.timers.iter() {
                        let _ = print_tx.send(Printout {
                            verbosity: 0,
                            content: format!("{}: {:?}", k, v),
                        }).await;
                    }
                    continue
                }
                let Ok(bytes): Result<[u8; 8], _> = req.ipc.try_into() else { continue };
                let timer_secs = u64::from_le_bytes(bytes);
                // if the timer is set to pop in 0 seconds, we immediately respond
                // otherwise, store in our persisted map, and spawn a task that
                // sleeps for the given time, then sends the response
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let pop_time = now + timer_secs;
                if timer_secs == 0 {
                    send_response(&our, km.id, km.rsvp.unwrap_or(km.source), &kernel_message_sender).await;
                    continue
                }
                if !timer_map.contains(pop_time) {
                    timer_tasks.spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(timer_secs)).await;
                        return pop_time
                    });
                }
                timer_map.insert(pop_time, km.id, km.rsvp.unwrap_or(km.source));
                persist_state(&our, &timer_map, &kernel_message_sender).await;
            }
            Some(Ok(time)) = timer_tasks.join_next() => {
                // when a timer pops, we send the response to the process(es) that set
                // the timer(s), and then remove it from our persisted map
                let Some(timers) = timer_map.remove(time) else { continue };
                persist_state(&our, &timer_map, &kernel_message_sender).await;
                for (id, addr) in timers {
                    send_response(&our, id, addr, &kernel_message_sender).await;
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct TimerMap {
    // key: the unix timestamp at which the timer pops
    // value: a vector of KernelMessage ids and who to send Response to
    // this is because multiple processes can set timers for the same time
    timers: BTreeMap<u64, Vec<(u64, Address)>>,
}

impl TimerMap {
    fn insert(&mut self, pop_time: u64, id: u64, addr: Address) {
        self.timers
            .entry(pop_time)
            .or_insert(vec![])
            .push((id, addr));
    }

    fn contains(&mut self, pop_time: u64) -> bool {
        self.timers.contains_key(&pop_time)
    }

    fn remove(&mut self, pop_time: u64) -> Option<Vec<(u64, Address)>> {
        self.timers.remove(&pop_time)
    }

    fn drain_expired(&mut self, time: u64) -> Vec<(u64, Address)> {
        return self
            .timers
            .extract_if(|k, _| *k <= time)
            .map(|(_, v)| v)
            .flatten()
            .collect();
    }
}

async fn send_response(our_node: &str, id: u64, target: Address, send_to_loop: &MessageSender) {
    let _ = send_to_loop
        .send(KernelMessage {
            id,
            source: Address {
                node: our_node.to_string(),
                process: TIMER_PROCESS_ID.clone(),
            },
            target,
            rsvp: None,
            message: Message::Response((
                Response {
                    inherit: false,
                    ipc: vec![],
                    metadata: None,
                },
                None,
            )),
            payload: None,
            signed_capabilities: None,
        })
        .await;
}

async fn persist_state(our_node: &str, state: &TimerMap, send_to_loop: &MessageSender) {
    let _ = send_to_loop
        .send(KernelMessage {
            id: rand::random(),
            source: Address {
                node: our_node.to_string(),
                process: TIMER_PROCESS_ID.clone(),
            },
            target: Address {
                node: our_node.to_string(),
                process: FILESYSTEM_PROCESS_ID.clone(),
            },
            rsvp: None,
            message: Message::Request(Request {
                inherit: false,
                expects_response: None,
                ipc: serde_json::to_vec(&FsAction::SetState(TIMER_PROCESS_ID.clone())).unwrap(),
                metadata: None,
            }),
            payload: Some(Payload {
                mime: None,
                bytes: bincode::serialize(&state).unwrap(),
            }),
            signed_capabilities: None,
        })
        .await;
}

async fn load_state_from_reboot(
    our_node: &str,
    send_to_loop: &MessageSender,
    recv_from_loop: &mut MessageReceiver,
) -> Result<TimerMap> {
    let _ = send_to_loop
        .send(KernelMessage {
            id: rand::random(),
            source: Address {
                node: our_node.to_string(),
                process: TIMER_PROCESS_ID.clone(),
            },
            target: Address {
                node: our_node.to_string(),
                process: FILESYSTEM_PROCESS_ID.clone(),
            },
            rsvp: None,
            message: Message::Request(Request {
                inherit: true,
                expects_response: Some(5), // TODO evaluate
                ipc: serde_json::to_vec(&FsAction::GetState(TIMER_PROCESS_ID.clone())).unwrap(),
                metadata: None,
            }),
            payload: None,
            signed_capabilities: None,
        })
        .await;
    let km = recv_from_loop.recv().await;
    let Some(km) = km else {
        return Err(anyhow::anyhow!("Failed to load state from reboot!"));
    };

    let KernelMessage {
        message, payload, ..
    } = km;
    let Message::Response((Response { ipc, .. }, None)) = message else {
        return Err(anyhow::anyhow!("Failed to load state from reboot!"));
    };
    let Ok(Ok(FsResponse::GetState)) = serde_json::from_slice::<Result<FsResponse, FsError>>(&ipc)
    else {
        return Err(anyhow::anyhow!("Failed to load state from reboot!"));
    };
    let Some(payload) = payload else {
        return Err(anyhow::anyhow!("Failed to load state from reboot!"));
    };
    return Ok(bincode::deserialize::<TimerMap>(&payload.bytes)?);
}
