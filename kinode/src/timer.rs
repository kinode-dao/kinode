use lib::types::core::{
    Address, KernelMessage, Message, MessageReceiver, MessageSender, PrintSender, Printout,
    Response, TimerAction, TIMER_PROCESS_ID,
};
use serde::{Deserialize, Serialize};

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

/// A runtime module that allows processes to set timers. Interacting with the
/// timer is done with a simple Request/Response pattern, and the timer module
/// is public, so it can be used by any local process. It will not respond to
/// requests made by other nodes.
///
/// The interface of the timer module is as follows:
/// One kind of request is accepted: TimerAction::SetTimer(u64), where the u64 is the
/// time to wait in milliseconds. This request should always expect a Response.
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
) -> anyhow::Result<()> {
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
                // we only handle Requests
                let Message::Request(req) = km.message else { continue };
                let Ok(timer_action) = serde_json::from_slice::<TimerAction>(&req.body) else {
                    Printout::new(1, TIMER_PROCESS_ID.clone(), "timer service received a request with an invalid body").send(&print_tx).await;
                    continue
                };
                match timer_action {
                    TimerAction::Debug => {
                        Printout::new(0, TIMER_PROCESS_ID.clone(), format!("timer service active timers ({}):", timer_map.timers.len())).send(&print_tx).await;
                        for (k, v) in timer_map.timers.iter() {
                            Printout::new(0, TIMER_PROCESS_ID.clone(), format!("{k}: {v:?}")).send(&print_tx).await;
                        }
                        continue
                    }
                    TimerAction::SetTimer(timer_millis) => {
                        // if the timer is set to pop in 0 millis, we immediately respond
                        // otherwise, store in our persisted map, and spawn a task that
                        // sleeps for the given time, then sends the response
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_millis() as u64;
                        let pop_time = now + timer_millis;
                        if timer_millis == 0 {
                            KernelMessage::builder()
                                .id(km.id)
                                .source((our.as_str(), TIMER_PROCESS_ID.clone()))
                                .target(km.rsvp.unwrap_or(km.source))
                                .message(Message::Response((
                                    Response {
                                        inherit: false,
                                        body: vec![],
                                        metadata: None,
                                        capabilities: vec![],
                                    },
                                    None,
                                )))
                                .build()
                                .unwrap()
                                .send(&kernel_message_sender).await;
                            continue
                        }
                        Printout::new(3, TIMER_PROCESS_ID.clone(), format!("set timer to pop in {timer_millis}ms")).send(&print_tx).await;
                        if !timer_map.contains(pop_time) {
                            timer_tasks.spawn(async move {
                                tokio::time::sleep(std::time::Duration::from_millis(timer_millis - 1)).await;
                                pop_time
                            });
                        }
                        timer_map.insert(pop_time, km.id, km.rsvp.unwrap_or(km.source));
                    }
                }
            }
            Some(Ok(time)) = timer_tasks.join_next() => {
                // when a timer pops, we send the response to the process(es) that set
                // the timer(s), and then remove it from our persisted map
                let Some(timers) = timer_map.remove(time) else { continue };
                for (id, addr) in timers {
                    KernelMessage::builder()
                        .id(id)
                        .source((our.as_str(), TIMER_PROCESS_ID.clone()))
                        .target(addr)
                        .message(Message::Response((
                            Response {
                                inherit: false,
                                body: vec![],
                                metadata: None,
                                capabilities: vec![],
                            },
                            None,
                        )))
                        .build()
                        .unwrap()
                        .send(&kernel_message_sender).await;
                }
            }
        }
    }
}
