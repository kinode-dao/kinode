use lib::types::core::{
    Address, FdManagerError, FdManagerRequest, FdManagerResponse, FdsLimit, KernelMessage, Message,
    MessageReceiver, MessageSender, PrintSender, Printout, ProcessId, Request,
    FD_MANAGER_PROCESS_ID,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};

#[cfg(unix)]
const DEFAULT_MAX_OPEN_FDS: u64 = 180;
#[cfg(target_os = "windows")]
const DEFAULT_MAX_OPEN_FDS: u64 = 7_000;

#[cfg(unix)]
const SYS_RESERVED_FDS: u64 = 30;

const DEFAULT_FDS_AS_FRACTION_OF_ULIMIT_PERCENTAGE: u64 = 90;
const DEFAULT_UPDATE_ULIMIT_SECS: u64 = 3600;
const _DEFAULT_CULL_FRACTION_DENOMINATOR: u64 = 2;

#[derive(Debug, Serialize, Deserialize)]
struct State {
    fds_limits: HashMap<ProcessId, FdsLimit>,
    mode: Mode,
    max_fds: u64,
}

#[derive(Debug, Serialize, Deserialize)]
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
            fds_limits: HashMap::new(),
            mode: Mode::default(static_max_fds),
            max_fds: match static_max_fds {
                Some(max) => max,
                None => DEFAULT_MAX_OPEN_FDS,
            },
        }
    }

    #[cfg(unix)]
    fn update_max_fds_from_ulimit(&mut self, ulimit_max_fds: u64) {
        let Mode::DynamicMax {
            ref max_fds_as_fraction_of_ulimit_percentage,
            ..
        } = self.mode
        else {
            return;
        };
        let min_ulimit = SYS_RESERVED_FDS + 10;
        if ulimit_max_fds <= min_ulimit {
            panic!(
                "fatal: ulimit from system ({ulimit_max_fds}) is too small to operate Hyperdrive. Please run Hyperdrive with a larger ulimit (at least {min_ulimit}).",
            );
        }

        self.max_fds =
            ulimit_max_fds * max_fds_as_fraction_of_ulimit_percentage / 100 - SYS_RESERVED_FDS;
    }

    async fn update_all_fds_limits(&mut self, our_node: &str, send_to_loop: &MessageSender) {
        let weights = self
            .fds_limits
            .values()
            .map(|limit| limit.hit_count)
            .sum::<u64>();
        let statically_allocated = self.max_fds as f64 / 2.0;
        let per_process_unweighted =
            statically_allocated / std::cmp::max(self.fds_limits.len() as u64, 1) as f64;
        let per_process_weighted = statically_allocated / std::cmp::max(weights, 1) as f64;
        for limit in self.fds_limits.values_mut() {
            limit.limit = (per_process_unweighted + per_process_weighted * limit.hit_count as f64)
                .floor() as u64;
        }
        send_all_fds_limits(our_node, send_to_loop, self).await;
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
    // Windows does not allow querying of max fds allowed.
    //  However, it allows some 16m, will expectation of actual
    //  max number open nearer to 10k; set to 7k which should be plenty.
    //  https://techcommunity.microsoft.com/t5/windows-blog-archive/pushing-the-limits-of-windows-handles/ba-p/723848
    #[cfg(target_os = "windows")]
    let static_max_fds = match static_max_fds {
        Some(smf) => Some(smf),
        None => Some(DEFAULT_MAX_OPEN_FDS),
    };

    let mut state = State::new(static_max_fds);
    #[cfg(unix)]
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
    loop {
        #[cfg(unix)]
        tokio::select! {
            Some(message) = recv_from_loop.recv() => {
                match handle_message(
                    &our_node,
                    message,
                    &mut state,
                    &send_to_loop,
                ).await {
                    Ok(Some(to_print)) => {
                        Printout::new(2, FD_MANAGER_PROCESS_ID.clone(), to_print).send(&send_to_terminal).await;
                    }
                    Err(e) => {
                        Printout::new(1, FD_MANAGER_PROCESS_ID.clone(), &format!("handle_message error: {e:?}"))
                            .send(&send_to_terminal)
                            .await;
                    }
                    _ => {}
                }
            }
            _ = interval.tick() => {
                let old_max_fds = state.max_fds;
                match update_max_fds(&mut state).await {
                    Ok(new) => {
                        if new != old_max_fds {
                            state.update_all_fds_limits(our_node.as_str(), &send_to_loop).await;
                        }
                    }
                    Err(e) => Printout::new(1, FD_MANAGER_PROCESS_ID.clone(), &format!("update_max_fds error: {e:?}"))
                        .send(&send_to_terminal)
                        .await,
                }
            }
        }
        #[cfg(target_os = "windows")]
        if let Some(message) = recv_from_loop.recv().await {
            match handle_message(&our_node, message, &mut state, &send_to_loop).await {
                Ok(Some(to_print)) => {
                    Printout::new(2, FD_MANAGER_PROCESS_ID.clone(), to_print)
                        .send(&send_to_terminal)
                        .await;
                }
                Err(e) => {
                    Printout::new(
                        1,
                        FD_MANAGER_PROCESS_ID.clone(),
                        &format!("handle_message error: {e:?}"),
                    )
                    .send(&send_to_terminal)
                    .await;
                }
                _ => {}
            }
        }
    }
}

async fn handle_message(
    our_node: &str,
    km: KernelMessage,
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
        FdManagerRequest::RequestFdsLimit => {
            // divide max_fds by number of processes requesting fds limits,
            // then send each process its new limit
            // TODO can weight different processes differently
            state.fds_limits.insert(
                km.source.process,
                FdsLimit {
                    limit: 0,
                    hit_count: 1, // starts with 1 to give initial weight
                },
            );
            state.update_all_fds_limits(our_node, &send_to_loop).await;
            None
        }
        FdManagerRequest::FdsLimitHit => {
            // sender process hit its fd limit
            // react to this by incrementing hit count and
            // re-weighting all processes' limits
            state.fds_limits.get_mut(&km.source.process).map(|limit| {
                limit.hit_count += 1;
            });
            state.update_all_fds_limits(our_node, &send_to_loop).await;
            Some(format!("{} hit its fd limit", km.source.process))
        }
        FdManagerRequest::FdsLimit(_) => {
            // should only send this, never receive it
            return Err(FdManagerError::FdManagerWasSentLimit.into());
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
        FdManagerRequest::UpdateCullFractionDenominator(_new) => {
            // state.cull_fraction_denominator = new;
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
                                state.fds_limits.clone(),
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
            Some(format!("fd-manager: {:?}", state))
        }
        FdManagerRequest::GetProcessFdLimit(process) => {
            if expects_response.is_some() {
                KernelMessage::builder()
                    .id(km.id)
                    .source(km.target)
                    .target(km.rsvp.unwrap_or(km.source))
                    .message(Message::Response((
                        lib::core::Response {
                            body: serde_json::to_vec(&FdManagerResponse::GetProcessFdLimit(
                                state
                                    .fds_limits
                                    .get(&process)
                                    .map(|limit| limit.limit)
                                    .unwrap_or(0),
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

#[cfg(unix)]
async fn update_max_fds(state: &mut State) -> anyhow::Result<u64> {
    let ulimit_max_fds = get_max_fd_limit()
        .map_err(|_| anyhow::anyhow!("Couldn't update max fd limit: ulimit failed"))?;
    state.update_max_fds_from_ulimit(ulimit_max_fds);
    Ok(ulimit_max_fds)
}

#[cfg(unix)]
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

async fn send_all_fds_limits(our_node: &str, send_to_loop: &MessageSender, state: &State) {
    for (process_id, limit) in &state.fds_limits {
        KernelMessage::builder()
            .id(rand::random())
            .source((our_node, FD_MANAGER_PROCESS_ID.clone()))
            .target((our_node, process_id))
            .message(Message::Request(Request {
                inherit: false,
                expects_response: None,
                body: serde_json::to_vec(&FdManagerRequest::FdsLimit(limit.limit)).unwrap(),
                metadata: None,
                capabilities: vec![],
            }))
            .build()
            .unwrap()
            .send(send_to_loop)
            .await;
    }
}

pub async fn send_fd_manager_request_fds_limit(our: &Address, send_to_loop: &MessageSender) {
    let message = Message::Request(Request {
        inherit: false,
        expects_response: None,
        body: serde_json::to_vec(&FdManagerRequest::RequestFdsLimit).unwrap(),
        metadata: None,
        capabilities: vec![],
    });
    send_to_fd_manager(our, message, send_to_loop).await
}

pub async fn send_fd_manager_hit_fds_limit(our: &Address, send_to_loop: &MessageSender) {
    let message = Message::Request(Request {
        inherit: false,
        expects_response: None,
        body: serde_json::to_vec(&FdManagerRequest::FdsLimitHit).unwrap(),
        metadata: None,
        capabilities: vec![],
    });
    send_to_fd_manager(our, message, send_to_loop).await
}

async fn send_to_fd_manager(our: &Address, message: Message, send_to_loop: &MessageSender) {
    KernelMessage::builder()
        .id(rand::random())
        .source(our.clone())
        .target((our.node.clone(), FD_MANAGER_PROCESS_ID.clone()))
        .message(message)
        .build()
        .unwrap()
        .send(send_to_loop)
        .await
}
