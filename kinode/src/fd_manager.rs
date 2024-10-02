use lib::types::core::{
    Address, FdManagerError, FdManagerRequest, FdManagerResponse, KernelMessage, Message,
    MessageReceiver, MessageSender, PrintSender, Printout, ProcessId, Request,
    FD_MANAGER_PROCESS_ID,
};
use std::{collections::HashMap, sync::Arc};

const DEFAULT_MAX_OPEN_FDS: u64 = 180;
const DEFAULT_FDS_AS_FRACTION_OF_ULIMIT_PERCENTAGE: u64 = 60;
const DEFAULT_UPDATE_ULIMIT_SECS: u64 = 3600;
const DEFAULT_CULL_FRACTION_DENOMINATOR: u64 = 2;

struct State {
    fds: HashMap<ProcessId, u64>,
    mode: Mode,
    total_fds: u64,
    max_fds: u64,
    cull_fraction_denominator: u64,
}

enum Mode {
    /// don't update the max_fds except by user input
    StaticMax,
    /// check the system's ulimit periodically and update max_fds accordingly
    DynamicMax {
        max_fds_as_fraction_of_ulimit_percentage: u64,
        update_ulimit_secs: u64,
    },
}

impl State {
    fn new(static_max_fds: Option<u64>) -> Self {
        Self::default(static_max_fds)
    }

    fn default(static_max_fds: Option<u64>) -> Self {
        Self {
            fds: HashMap::new(),
            mode: Mode::default(static_max_fds),
            total_fds: 0,
            max_fds: match static_max_fds {
                Some(max) => max,
                None => DEFAULT_MAX_OPEN_FDS,
            },
            cull_fraction_denominator: DEFAULT_CULL_FRACTION_DENOMINATOR,
        }
    }

    fn update_max_fds_from_ulimit(&mut self, ulimit_max_fds: u64) {
        let Mode::DynamicMax {
            ref max_fds_as_fraction_of_ulimit_percentage,
            ..
        } = self.mode
        else {
            return;
        };
        self.max_fds = ulimit_max_fds * max_fds_as_fraction_of_ulimit_percentage / 100;
    }
}

impl Mode {
    fn default(static_max_fds: Option<u64>) -> Self {
        match static_max_fds {
            Some(_) => Self::StaticMax,
            None => Self::DynamicMax {
                max_fds_as_fraction_of_ulimit_percentage:
                    DEFAULT_FDS_AS_FRACTION_OF_ULIMIT_PERCENTAGE,
                update_ulimit_secs: DEFAULT_UPDATE_ULIMIT_SECS,
            },
        }
    }
}

/// The fd_manager entrypoint.
pub async fn fd_manager(
    our_node: Arc<String>,
    send_to_loop: MessageSender,
    send_to_terminal: PrintSender,
    mut recv_from_loop: MessageReceiver,
    static_max_fds: Option<u64>,
) -> anyhow::Result<()> {
    let mut state = State::new(static_max_fds);
    let mut interval = {
        // in code block to release the reference into state
        let Mode::DynamicMax {
            ref update_ulimit_secs,
            ..
        } = state.mode
        else {
            return Ok(());
        };
        tokio::time::interval(tokio::time::Duration::from_secs(*update_ulimit_secs))
    };
    let our_node = our_node.as_str();
    loop {
        tokio::select! {
            Some(message) = recv_from_loop.recv() => {
                match handle_message(
                    message,
                    &mut interval,
                    &mut state,
                    &send_to_loop,
                ).await {
                    Ok(Some(to_print)) => {
                        Printout::new(2, to_print).send(&send_to_terminal).await;
                    }
                    Err(e) => {
                        Printout::new(1, &format!("handle_message error: {e:?}"))
                            .send(&send_to_terminal)
                            .await;
                    }
                    _ => {}
                }
            }
            _ = interval.tick() => {
                if let Err(e) = update_max_fds(&mut state).await {
                    Printout::new(1, &format!("update_max_fds error: {e:?}"))
                        .send(&send_to_terminal)
                        .await;
                }
            }
        }

        if state.total_fds >= state.max_fds {
            Printout::new(
                2,
                format!(
                    "Have {} open >= {} max fds; sending Cull Request...",
                    state.total_fds, state.max_fds,
                ),
            )
            .send(&send_to_terminal)
            .await;
            send_cull(our_node, &send_to_loop, &state).await;
        }
    }
}

async fn handle_message(
    km: KernelMessage,
    _interval: &mut tokio::time::Interval,
    state: &mut State,
    send_to_loop: &MessageSender,
) -> anyhow::Result<Option<String>> {
    let Message::Request(Request {
        body,
        expects_response,
        ..
    }) = km.message
    else {
        return Err(FdManagerError::NotARequest.into());
    };
    let request: FdManagerRequest =
        serde_json::from_slice(&body).map_err(|_e| FdManagerError::BadRequest)?;
    let return_value = match request {
        FdManagerRequest::OpenFds { number_opened } => {
            state.total_fds += number_opened;
            state
                .fds
                .entry(km.source.process)
                .and_modify(|e| *e += number_opened)
                .or_insert(number_opened);
            None
        }
        FdManagerRequest::CloseFds { mut number_closed } => {
            assert!(state.total_fds >= number_closed);
            let return_value = Some(format!(
                "{} closed {} of {}",
                km.source.process, number_closed, state.total_fds,
            ));
            state.total_fds -= number_closed;
            state
                .fds
                .entry(km.source.process)
                .and_modify(|e| {
                    assert!(e >= &mut number_closed);
                    *e -= number_closed
                })
                .or_insert(number_closed);
            return_value
        }
        FdManagerRequest::Cull { .. } => {
            return Err(FdManagerError::FdManagerWasSentCull.into());
        }
        FdManagerRequest::UpdateMaxFdsAsFractionOfUlimitPercentage(new) => {
            match state.mode {
                Mode::DynamicMax {
                    ref mut max_fds_as_fraction_of_ulimit_percentage,
                    ..
                } => *max_fds_as_fraction_of_ulimit_percentage = new,
                _ => return Err(FdManagerError::BadRequest.into()),
            }
            None
        }
        FdManagerRequest::UpdateUpdateUlimitSecs(new) => {
            match state.mode {
                Mode::DynamicMax {
                    ref mut update_ulimit_secs,
                    ..
                } => *update_ulimit_secs = new,
                _ => return Err(FdManagerError::BadRequest.into()),
            }
            None
        }
        FdManagerRequest::UpdateCullFractionDenominator(new) => {
            state.cull_fraction_denominator = new;
            None
        }
        FdManagerRequest::GetState => {
            if expects_response.is_some() {
                KernelMessage::builder()
                    .id(km.id)
                    .source(km.target)
                    .target(km.rsvp.unwrap_or(km.source))
                    .message(Message::Response((
                        lib::core::Response {
                            body: serde_json::to_vec(&FdManagerResponse::GetState(
                                state.fds.clone(),
                            ))
                            .unwrap(),
                            inherit: false,
                            metadata: None,
                            capabilities: vec![],
                        },
                        None,
                    )))
                    .build()
                    .unwrap()
                    .send(send_to_loop)
                    .await;
            }
            None
        }
        FdManagerRequest::GetProcessFdCount(process) => {
            if expects_response.is_some() {
                KernelMessage::builder()
                    .id(km.id)
                    .source(km.target)
                    .target(km.rsvp.unwrap_or(km.source))
                    .message(Message::Response((
                        lib::core::Response {
                            body: serde_json::to_vec(&FdManagerResponse::GetProcessFdCount(
                                *state.fds.get(&process).unwrap_or(&0),
                            ))
                            .unwrap(),
                            inherit: false,
                            metadata: None,
                            capabilities: vec![],
                        },
                        None,
                    )))
                    .build()
                    .unwrap()
                    .send(send_to_loop)
                    .await;
            }
            None
        }
    };
    Ok(return_value)
}

async fn update_max_fds(state: &mut State) -> anyhow::Result<()> {
    let ulimit_max_fds = get_max_fd_limit()
        .map_err(|_| anyhow::anyhow!("Couldn't update max fd limit: ulimit failed"))?;
    state.update_max_fds_from_ulimit(ulimit_max_fds);
    Ok(())
}

async fn send_cull(
    our_node: &str,
    send_to_loop: &MessageSender,
    state: &State,
) {
    let message = Message::Request(Request {
        inherit: false,
        expects_response: None,
        body: serde_json::to_vec(&FdManagerRequest::Cull {
            cull_fraction_denominator: state.cull_fraction_denominator.clone(),
        })
        .unwrap(),
        metadata: None,
        capabilities: vec![],
    });
    for process_id in state.fds.keys() {
        KernelMessage::builder()
            .id(rand::random())
            .source((our_node, FD_MANAGER_PROCESS_ID.clone()))
            .target((our_node, process_id.clone()))
            .message(message.clone())
            .build()
            .unwrap()
            .send(send_to_loop)
            .await;
    }
}

fn get_max_fd_limit() -> anyhow::Result<u64> {
    let mut rlim = libc::rlimit {
        rlim_cur: 0, // Current limit
        rlim_max: 0, // Maximum limit value
    };

    // RLIMIT_NOFILE is the resource indicating the maximum file descriptor number.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut rlim) } == 0 {
        Ok(rlim.rlim_cur as u64)
    } else {
        Err(anyhow::anyhow!("Failed to get the resource limit."))
    }
}

pub async fn send_fd_manager_open(
    our: &Address,
    number_opened: u64,
    send_to_loop: &MessageSender,
) -> anyhow::Result<()> {
    let message = Message::Request(Request {
        inherit: false,
        expects_response: None,
        body: serde_json::to_vec(&FdManagerRequest::OpenFds { number_opened }).unwrap(),
        metadata: None,
        capabilities: vec![],
    });
    send_to_fd_manager(our, message, send_to_loop).await?;
    Ok(())
}

pub async fn send_fd_manager_close(
    our: &Address,
    number_closed: u64,
    send_to_loop: &MessageSender,
) -> anyhow::Result<()> {
    let message = Message::Request(Request {
        inherit: false,
        expects_response: None,
        body: serde_json::to_vec(&FdManagerRequest::CloseFds { number_closed }).unwrap(),
        metadata: None,
        capabilities: vec![],
    });
    send_to_fd_manager(our, message, send_to_loop).await?;
    Ok(())
}

async fn send_to_fd_manager(
    our: &Address,
    message: Message,
    send_to_loop: &MessageSender,
) -> anyhow::Result<()> {
    KernelMessage::builder()
        .id(rand::random())
        .source(our.clone())
        .target((our.node.clone(), FD_MANAGER_PROCESS_ID.clone()))
        .message(message)
        .build()
        .unwrap()
        .send(send_to_loop)
        .await;
    Ok(())
}
