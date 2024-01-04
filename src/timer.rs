use crate::types::{
    Address, KernelMessage, Message, MessageReceiver, MessageSender, PrintSender, Printout,
    Response, TIMER_PROCESS_ID,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// A runtime module that allows processes to set timers. Interacting with the
/// timer is done with a simple Request/Response pattern, and the timer module
/// is public, so it can be used by any local process. It will not respond to
/// requests made by other nodes.
///
/// The interface of the timer module is as follows:
/// One kind of request is accepted: the IPC must be a little-endian byte-representation
/// of an unsigned 64-bit integer, in milliseconds. This request should always expect a Response.
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
    let mut timer_map = TimerMap {
        timers: nohash_hasher::IntMap::default(),
    };
    // joinset holds 1 active timer per expiration-time
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
                let timer_millis = u64::from_le_bytes(bytes);
                // if the timer is set to pop in 0 millis, we immediately respond
                // otherwise, store in our persisted map, and spawn a task that
                // sleeps for the given time, then sends the response
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;
                let pop_time = now + timer_millis;
                if timer_millis == 0 {
                    send_response(&our, km.id, km.rsvp.unwrap_or(km.source), &kernel_message_sender).await;
                    continue
                }
                let _ = print_tx.send(Printout {
                    verbosity: 1,
                    content: format!("set timer to pop in {}ms", timer_millis),
                }).await;
                if !timer_map.contains(pop_time) {
                    timer_tasks.spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(timer_millis - 1)).await;
                        pop_time
                    });
                }
                timer_map.insert(pop_time, km.id, km.rsvp.unwrap_or(km.source));
            }
            Some(Ok(time)) = timer_tasks.join_next() => {
                // when a timer pops, we send the response to the process(es) that set
                // the timer(s), and then remove it from our persisted map
                let Some(timers) = timer_map.remove(time) else { continue };
                for (id, addr) in timers {
                    send_response(&our, id, addr, &kernel_message_sender).await;
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct TimerMap {
    // key: the unix timestamp in milliseconds at which the timer pops
    // value: a vector of KernelMessage ids and who to send Response to
    // this is because multiple processes can set timers for the same time
    timers: nohash_hasher::IntMap<u64, Vec<(u64, Address)>>,
}

impl TimerMap {
    fn insert(&mut self, pop_time: u64, id: u64, addr: Address) {
        self.timers.entry(pop_time).or_default().push((id, addr));
    }

    fn contains(&mut self, pop_time: u64) -> bool {
        self.timers.contains_key(&pop_time)
    }

    fn remove(&mut self, pop_time: u64) -> Option<Vec<(u64, Address)>> {
        self.timers.remove(&pop_time)
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
                    capabilities: vec![],
                },
                None,
            )),
            payload: None,
            signed_capabilities: vec![],
        })
        .await;
}
