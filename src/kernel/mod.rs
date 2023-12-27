use crate::types::STATE_PROCESS_ID;
use crate::types::{self as t, VFS_PROCESS_ID};
use crate::KERNEL_PROCESS_ID;
use anyhow::Result;
use ring::signature::{self, KeyPair};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use wasmtime::{Config, Engine, WasmBacktraceDetails};

/// Manipulate a single process.
pub mod process;
/// Implement the functions served to processes by `uqbar.wit`.
mod standard_host;

const PROCESS_CHANNEL_CAPACITY: usize = 100;

type ProcessMessageSender =
    tokio::sync::mpsc::Sender<Result<t::KernelMessage, t::WrappedSendError>>;
type ProcessMessageReceiver =
    tokio::sync::mpsc::Receiver<Result<t::KernelMessage, t::WrappedSendError>>;

#[derive(Serialize, Deserialize)]
struct StartProcessMetadata {
    source: t::Address,
    process_id: t::ProcessId,
    persisted: t::PersistedProcess,
    reboot: bool,
}

//  live in event loop
type Senders = HashMap<t::ProcessId, ProcessSender>;
//  handles are for managing liveness, map is for persistence and metadata.
type ProcessHandles = HashMap<t::ProcessId, JoinHandle<Result<()>>>;

enum ProcessSender {
    Runtime(t::MessageSender),
    Userspace(ProcessMessageSender),
}

/// persist kernel's process_map state for next bootup
/// and (TODO) wait for filesystem to respond in the affirmative
async fn persist_state(
    our_name: &str,
    send_to_loop: &t::MessageSender,
    process_map: &t::ProcessMap,
) -> Result<()> {
    let bytes = bincode::serialize(process_map)?;
    send_to_loop
        .send(t::KernelMessage {
            id: rand::random(),
            source: t::Address {
                node: our_name.to_string(),
                process: KERNEL_PROCESS_ID.clone(),
            },
            target: t::Address {
                node: our_name.to_string(),
                process: STATE_PROCESS_ID.clone(),
            },
            rsvp: None,
            message: t::Message::Request(t::Request {
                inherit: true,
                expects_response: None,
                ipc: serde_json::to_vec(&t::StateAction::SetState(KERNEL_PROCESS_ID.clone()))
                    .unwrap(),
                metadata: None,
            }),
            payload: Some(t::Payload { mime: None, bytes }),
            signed_capabilities: None,
        })
        .await?;
    Ok(())
}

/// handle messages sent directly to kernel. source is always our own node.
async fn handle_kernel_request(
    our_name: String,
    keypair: Arc<signature::Ed25519KeyPair>,
    km: t::KernelMessage,
    send_to_loop: t::MessageSender,
    send_to_terminal: t::PrintSender,
    senders: &mut Senders,
    process_handles: &mut ProcessHandles,
    process_map: &mut t::ProcessMap,
    caps_oracle: t::CapMessageSender,
    engine: &Engine,
) {
    let t::Message::Request(request) = km.message else {
        return;
    };
    let command: t::KernelCommand = match serde_json::from_slice(&request.ipc) {
        Err(e) => {
            let _ = send_to_terminal
                .send(t::Printout {
                    verbosity: 0,
                    content: format!("kernel: couldn't parse command: {:?}", e),
                })
                .await;
            return;
        }
        Ok(c) => c,
    };
    match command {
        t::KernelCommand::Booted => {
            for (process_id, process_sender) in senders {
                let ProcessSender::Userspace(sender) = process_sender else {
                    continue;
                };
                let _ = sender
                    .send(Ok(t::KernelMessage {
                        id: km.id,
                        source: t::Address {
                            node: our_name.clone(),
                            process: KERNEL_PROCESS_ID.clone(),
                        },
                        target: t::Address {
                            node: our_name.clone(),
                            process: process_id.clone(),
                        },
                        rsvp: None,
                        message: t::Message::Request(t::Request {
                            inherit: false,
                            expects_response: None,
                            ipc: b"run".to_vec(),
                            metadata: None,
                        }),
                        payload: None,
                        signed_capabilities: None,
                    }))
                    .await;
            }
        }
        t::KernelCommand::Shutdown => {
            for handle in process_handles.values() {
                handle.abort();
            }
        }
        //
        // initialize a new process. this is the only way to create a new process.
        //
        t::KernelCommand::InitializeProcess {
            id,
            wasm_bytes_handle,
            on_exit,
            initial_capabilities,
            public,
        } => {
            let Some(payload) = km.payload else {
                let _ = send_to_terminal
                    .send(t::Printout {
                        verbosity: 0,
                        content: "kernel: process startup requires bytes".into(),
                    })
                    .await;
                // fire an error back
                send_to_loop
                    .send(t::KernelMessage {
                        id: km.id,
                        source: t::Address {
                            node: our_name.clone(),
                            process: KERNEL_PROCESS_ID.clone(),
                        },
                        target: km.rsvp.unwrap_or(km.source),
                        rsvp: None,
                        message: t::Message::Response((
                            t::Response {
                                inherit: false,
                                ipc: serde_json::to_vec(&t::KernelResponse::InitializeProcessError)
                                    .unwrap(),
                                metadata: None,
                            },
                            None,
                        )),
                        payload: None,
                        signed_capabilities: None,
                    })
                    .await
                    .expect("event loop: fatal: sender died");
                return;
            };

            // check cap sigs & transform valid to unsigned to be plugged into procs
            let pk = signature::UnparsedPublicKey::new(&signature::ED25519, keypair.public_key());
            let mut valid_capabilities: HashSet<t::Capability> = HashSet::new();
            for signed_cap in initial_capabilities {
                let cap = t::Capability {
                    issuer: signed_cap.issuer,
                    params: signed_cap.params,
                };
                match pk.verify(
                    &rmp_serde::to_vec(&cap).unwrap_or_default(),
                    &signed_cap.signature,
                ) {
                    Ok(_) => {}
                    Err(e) => {
                        println!("kernel: StartProcess no cap: {}", e);
                        continue;
                    }
                }
                valid_capabilities.insert(cap);
            }

            // give the initializer and itself the messaging cap.
            // NOTE: we do this even if the process is public, because
            // a process might redundantly call grant_messaging.
            valid_capabilities.insert(t::Capability {
                issuer: t::Address {
                    node: our_name.clone(),
                    process: id.clone(),
                },
                params: "\"messaging\"".into(),
            });
            caps_oracle
                .send(t::CapMessage::Add {
                    on: km.source.process.clone(),
                    cap: t::Capability {
                        issuer: t::Address {
                            node: our_name.clone(),
                            process: id.clone(),
                        },
                        params: "\"messaging\"".into(),
                    },
                    responder: tokio::sync::oneshot::channel().0,
                })
                .await
                .expect("event loop: fatal: sender died");

            // fires "success" response back if successful
            match start_process(
                our_name.clone(),
                keypair.clone(),
                km.id,
                payload.bytes,
                send_to_loop.clone(),
                send_to_terminal,
                senders,
                process_handles,
                process_map,
                engine,
                caps_oracle,
                &StartProcessMetadata {
                    source: if let Some(ref rsvp) = km.rsvp {
                        rsvp.clone()
                    } else {
                        km.source.clone()
                    },
                    process_id: id,
                    persisted: t::PersistedProcess {
                        wasm_bytes_handle,
                        on_exit,
                        capabilities: valid_capabilities,
                        public,
                    },
                    reboot: false,
                },
            )
            .await
            {
                Ok(()) => (),
                Err(_e) => {
                    send_to_loop
                        .send(t::KernelMessage {
                            id: km.id,
                            source: t::Address {
                                node: our_name.clone(),
                                process: KERNEL_PROCESS_ID.clone(),
                            },
                            target: km.rsvp.unwrap_or(km.source),
                            rsvp: None,
                            message: t::Message::Response((
                                t::Response {
                                    inherit: false,
                                    ipc: serde_json::to_vec(
                                        &t::KernelResponse::InitializeProcessError,
                                    )
                                    .unwrap(),
                                    metadata: None,
                                },
                                None,
                            )),
                            payload: None,
                            signed_capabilities: None,
                        })
                        .await
                        .expect("event loop: fatal: sender died");
                }
            }
        }
        // send 'run' message to a process that's already been initialized
        t::KernelCommand::RunProcess(process_id) => {
            if let Some(ProcessSender::Userspace(process_sender)) = senders.get(&process_id) {
                if let Ok(()) = process_sender
                    .send(Ok(t::KernelMessage {
                        id: rand::random(),
                        source: t::Address {
                            node: our_name.clone(),
                            process: KERNEL_PROCESS_ID.clone(),
                        },
                        target: t::Address {
                            node: our_name.clone(),
                            process: process_id,
                        },
                        rsvp: None,
                        message: t::Message::Request(t::Request {
                            inherit: false,
                            expects_response: None,
                            ipc: b"run".to_vec(),
                            metadata: None,
                        }),
                        payload: None,
                        signed_capabilities: None,
                    }))
                    .await
                {
                    send_to_loop
                        .send(t::KernelMessage {
                            id: km.id,
                            source: t::Address {
                                node: our_name.clone(),
                                process: KERNEL_PROCESS_ID.clone(),
                            },
                            target: km.rsvp.unwrap_or(km.source),
                            rsvp: None,
                            message: t::Message::Response((
                                t::Response {
                                    inherit: false,
                                    ipc: serde_json::to_vec(&t::KernelResponse::StartedProcess)
                                        .unwrap(),
                                    metadata: None,
                                },
                                None,
                            )),
                            payload: None,
                            signed_capabilities: None,
                        })
                        .await
                        .expect("event loop: fatal: sender died");
                }
            } else {
                let _ = send_to_terminal
                    .send(t::Printout {
                        verbosity: 0,
                        content: format!("kernel: no such process {:?} to run", process_id),
                    })
                    .await;
                // fire an error back
                send_to_loop
                    .send(t::KernelMessage {
                        id: km.id,
                        source: t::Address {
                            node: our_name.clone(),
                            process: KERNEL_PROCESS_ID.clone(),
                        },
                        target: km.rsvp.unwrap_or(km.source),
                        rsvp: None,
                        message: t::Message::Response((
                            t::Response {
                                inherit: false,
                                ipc: serde_json::to_vec(&t::KernelResponse::RunProcessError)
                                    .unwrap(),
                                metadata: None,
                            },
                            None,
                        )),
                        payload: None,
                        signed_capabilities: None,
                    })
                    .await
                    .expect("event loop: fatal: sender died");
            }
        }
        t::KernelCommand::KillProcess(process_id) => {
            // brutal and savage killing: aborting the task.
            // do not do this to a process if you don't want to risk
            // dropped messages / un-replied-to-requests
            let _ = senders.remove(&process_id);
            let process_handle = match process_handles.remove(&process_id) {
                Some(ph) => ph,
                None => {
                    let _ = send_to_terminal
                        .send(t::Printout {
                            verbosity: 2,
                            content: format!("kernel: no such process {:?} to kill", process_id),
                        })
                        .await;
                    return;
                }
            };
            process_handle.abort();
            process_map.remove(&process_id);
            let _ = persist_state(&our_name, &send_to_loop, process_map).await;
            if request.expects_response.is_none() {
                return;
            }
            let _ = send_to_terminal
                .send(t::Printout {
                    verbosity: 0,
                    content: format!("kernel: killing process {}", process_id),
                })
                .await;
            send_to_loop
                .send(t::KernelMessage {
                    id: km.id,
                    source: t::Address {
                        node: our_name.clone(),
                        process: KERNEL_PROCESS_ID.clone(),
                    },
                    target: km.rsvp.unwrap_or(km.source),
                    rsvp: None,
                    message: t::Message::Response((
                        t::Response {
                            inherit: false,
                            ipc: serde_json::to_vec(&t::KernelResponse::KilledProcess(process_id))
                                .unwrap(),
                            metadata: None,
                        },
                        None,
                    )),
                    payload: None,
                    signed_capabilities: None,
                })
                .await
                .expect("event loop: fatal: sender died");
        }
    }
}

/// currently, the kernel only receives 2 classes of responses, file-read and set-state
/// responses from the filesystem module. it uses these to get wasm bytes of a process and
/// start that process.
///
/// TODO: boot relies on this -- if we can do that differently, can skip handling
/// responses entirely.
async fn handle_kernel_response(
    our_name: String,
    keypair: Arc<signature::Ed25519KeyPair>,
    km: t::KernelMessage,
    send_to_loop: t::MessageSender,
    send_to_terminal: t::PrintSender,
    senders: &mut Senders,
    process_handles: &mut ProcessHandles,
    process_map: &mut t::ProcessMap,
    caps_oracle: t::CapMessageSender,
    engine: &Engine,
) {
    let t::Message::Response((ref response, _)) = km.message else {
        let _ = send_to_terminal
            .send(t::Printout {
                verbosity: 0,
                content: "kernel: got weird Response".into(),
            })
            .await;
        return;
    };
    // ignore responses that aren't filesystem or state responses
    if km.source.process != *STATE_PROCESS_ID && km.source.process != *VFS_PROCESS_ID {
        return;
    }
    let Some(ref metadata) = response.metadata else {
        //  set-state response currently return here
        //  we might want to match on metadata type from both, and only update
        //  process map upon receiving confirmation that it's been persisted
        return;
    };
    let Ok(meta) = serde_json::from_str::<StartProcessMetadata>(metadata) else {
        let _ = send_to_terminal
            .send(t::Printout {
                verbosity: 0,
                content: "kernel: got weird metadata from filesystem".into(),
            })
            .await;
        return;
    };
    let Some(payload) = km.payload else {
        let _ = send_to_terminal
            .send(t::Printout {
                verbosity: 0,
                content: format!(
                    "kernel: process {} seemingly could not be read from filesystem. km: {}",
                    meta.process_id, km
                ),
            })
            .await;
        return;
    };

    if let Ok(()) = start_process(
        our_name.clone(),
        keypair,
        km.id,
        payload.bytes,
        send_to_loop,
        send_to_terminal.clone(),
        senders,
        process_handles,
        process_map,
        engine,
        caps_oracle,
        &meta,
    )
    .await
    {
        // immediately run a rebooted process
        if let Some(ProcessSender::Userspace(sender)) = senders.get(&meta.process_id) {
            let _ = sender
                .send(Ok(t::KernelMessage {
                    id: rand::random(),
                    source: t::Address {
                        node: our_name.clone(),
                        process: KERNEL_PROCESS_ID.clone(),
                    },
                    target: t::Address {
                        node: our_name.clone(),
                        process: meta.process_id.clone(),
                    },
                    rsvp: None,
                    message: t::Message::Request(t::Request {
                        inherit: false,
                        expects_response: None,
                        ipc: b"run".to_vec(),
                        metadata: None,
                    }),
                    payload: None,
                    signed_capabilities: None,
                }))
                .await;
            return;
        }
    };
    let _ = send_to_terminal
        .send(t::Printout {
            verbosity: 0,
            content: "kernel: process start fail".into(),
        })
        .await;
}

async fn start_process(
    our_name: String,
    keypair: Arc<signature::Ed25519KeyPair>,
    km_id: u64,
    km_payload_bytes: Vec<u8>,
    send_to_loop: t::MessageSender,
    send_to_terminal: t::PrintSender,
    senders: &mut Senders,
    process_handles: &mut ProcessHandles,
    process_map: &mut t::ProcessMap,
    engine: &Engine,
    caps_oracle: t::CapMessageSender,
    process_metadata: &StartProcessMetadata,
) -> Result<()> {
    let (send_to_process, recv_in_process) =
        mpsc::channel::<Result<t::KernelMessage, t::WrappedSendError>>(PROCESS_CHANNEL_CAPACITY);
    let id = &process_metadata.process_id;
    if senders.contains_key(&id) {
        let _ = send_to_terminal
            .send(t::Printout {
                verbosity: 0,
                content: format!("kernel: process with ID {} already exists", id),
            })
            .await;
        return Err(anyhow::anyhow!("process with ID {} already exists", id));
    }
    senders.insert(
        id.clone(),
        ProcessSender::Userspace(send_to_process.clone()),
    );
    let metadata = t::ProcessMetadata {
        our: t::Address {
            node: our_name.clone(),
            process: id.clone(),
        },
        wasm_bytes_handle: process_metadata.persisted.wasm_bytes_handle.clone(),
        on_exit: process_metadata.persisted.on_exit.clone(),
        public: process_metadata.persisted.public,
    };
    process_handles.insert(
        id.clone(),
        tokio::spawn(process::make_process_loop(
            keypair.clone(),
            metadata.clone(),
            send_to_loop.clone(),
            send_to_terminal.clone(),
            recv_in_process,
            send_to_process,
            km_payload_bytes,
            caps_oracle,
            engine.clone(),
        )),
    );

    process_map.insert(id.clone(), process_metadata.persisted.clone());
    if !process_metadata.reboot {
        // if new, persist
        persist_state(&our_name, &send_to_loop, process_map).await?;
    }
    send_to_loop
        .send(t::KernelMessage {
            id: km_id,
            source: t::Address {
                node: our_name.clone(),
                process: KERNEL_PROCESS_ID.clone(),
            },
            target: process_metadata.source.clone(),
            rsvp: None,
            message: t::Message::Response((
                t::Response {
                    inherit: false,
                    ipc: serde_json::to_vec(&t::KernelResponse::InitializedProcess)?,
                    metadata: None,
                },
                None,
            )),
            payload: None,
            signed_capabilities: None,
        })
        .await?;
    Ok(())
}

/// the uqbar kernel. contains event loop which handles all message-passing between
/// all processes (WASM apps) and also runtime tasks.
pub async fn kernel(
    our: t::Identity,
    keypair: Arc<signature::Ed25519KeyPair>,
    mut process_map: t::ProcessMap,
    caps_oracle_sender: t::CapMessageSender,
    mut caps_oracle_receiver: t::CapMessageReceiver,
    send_to_loop: t::MessageSender,
    send_to_terminal: t::PrintSender,
    mut recv_in_loop: t::MessageReceiver,
    mut network_error_recv: t::NetworkErrorReceiver,
    mut recv_debug_in_loop: t::DebugReceiver,
    send_to_net: t::MessageSender,
    runtime_extensions: Vec<(t::ProcessId, t::MessageSender, bool)>,
) -> Result<()> {
    let mut config = Config::new();
    config.cache_config_load_default().unwrap();
    config.wasm_backtrace_details(WasmBacktraceDetails::Enable);
    config.wasm_component_model(true);
    config.async_support(true);
    let engine = Engine::new(&config).unwrap();

    let mut senders: Senders = HashMap::new();
    senders.insert(
        t::ProcessId::new(Some("net"), "sys", "uqbar"),
        ProcessSender::Runtime(send_to_net.clone()),
    );
    for (process_id, sender, _) in runtime_extensions {
        senders.insert(process_id, ProcessSender::Runtime(sender));
    }

    // each running process is stored in this map
    let mut process_handles: ProcessHandles = HashMap::new();

    let mut is_debug: bool = false;

    for (process_id, persisted) in &process_map {
        // runtime extensions will have a bytes_handle of 0, because they have no
        // WASM code saved in filesystem.
        if persisted.on_exit.is_restart() && persisted.wasm_bytes_handle != "" {
            send_to_loop
                .send(t::KernelMessage {
                    id: rand::random(),
                    source: t::Address {
                        node: our.name.clone(),
                        process: KERNEL_PROCESS_ID.clone(),
                    },
                    target: t::Address {
                        node: our.name.clone(),
                        process: VFS_PROCESS_ID.clone(),
                    },
                    rsvp: None,
                    message: t::Message::Request(t::Request {
                        inherit: true,
                        expects_response: Some(5), // TODO evaluate
                        ipc: serde_json::to_vec(&t::VfsRequest {
                            path: persisted.wasm_bytes_handle.clone(),
                            action: t::VfsAction::Read,
                        })
                        .unwrap(),
                        metadata: Some(
                            serde_json::to_string(&StartProcessMetadata {
                                source: t::Address {
                                    node: our.name.clone(),
                                    process: KERNEL_PROCESS_ID.clone(),
                                },
                                process_id: process_id.clone(),
                                persisted: persisted.clone(),
                                reboot: true,
                            })
                            .unwrap(),
                        ),
                    }),
                    payload: None,
                    signed_capabilities: None,
                })
                .await
                .expect("event loop: fatal: sender died");
        }
        if let t::OnExit::Requests(requests) = &persisted.on_exit {
            // if a persisted process had on-death-requests, we should perform them now
            // even in death, a process can only message processes it has capabilities for
            for (address, request, payload) in requests {
                // the process that made the request is dead, so never expects response
                let mut request = request.to_owned();
                request.expects_response = None;
                if persisted.capabilities.contains(&t::Capability {
                    issuer: address.clone(),
                    params: "\"messaging\"".into(),
                }) {
                    send_to_loop
                        .send(t::KernelMessage {
                            id: rand::random(),
                            source: t::Address {
                                node: our.name.clone(),
                                process: process_id.clone(),
                            },
                            target: address.clone(),
                            rsvp: None,
                            message: t::Message::Request(request),
                            payload: payload.clone(),
                            signed_capabilities: None,
                        })
                        .await
                        .expect("fatal: kernel event loop died");
                }
            }
        }
    }

    // after all bootstrapping messages are handled, send a Booted kernelcommand
    // to turn it on
    send_to_loop
        .send(t::KernelMessage {
            id: rand::random(),
            source: t::Address {
                node: our.name.clone(),
                process: KERNEL_PROCESS_ID.clone(),
            },
            target: t::Address {
                node: our.name.clone(),
                process: KERNEL_PROCESS_ID.clone(),
            },
            rsvp: None,
            message: t::Message::Request(t::Request {
                inherit: true,
                expects_response: None,
                ipc: serde_json::to_vec(&t::KernelCommand::Booted).unwrap(),
                metadata: None,
            }),
            payload: None,
            signed_capabilities: None,
        })
        .await
        .expect("fatal: kernel event loop died");

    #[cfg(feature = "simulation-mode")]
    let tester_process_id = t::ProcessId::new(Some("tester"), "tester", "uqbar");

    // main event loop
    loop {
        tokio::select! {
            // debug mode toggle: when on, this loop becomes a manual step-through
            debug = recv_debug_in_loop.recv() => {
                if let Some(t::DebugCommand::Toggle) = debug {
                    is_debug = !is_debug;
                }
            },
            // network error message receiver: handle `timeout` and `offline` errors
            // directly from the networking task in runtime, and filter them to the
            // sender of the original attempted message.
            Some(wrapped_network_error) = network_error_recv.recv() => {
                let _ = send_to_terminal.send(
                    t::Printout {
                        verbosity: 2,
                        content: format!("event loop: got network error: {:?}", wrapped_network_error)
                    }
                ).await;
                // forward the error to the relevant process
                match senders.get(&wrapped_network_error.source.process) {
                    Some(ProcessSender::Userspace(sender)) => {
                        let _ = sender.send(Err(wrapped_network_error)).await;
                    }
                    Some(ProcessSender::Runtime(_sender)) => {
                        // TODO should runtime modules get these? no
                        // this will change if a runtime process ever makes
                        // a message directed to not-our-node
                    }
                    None => {
                        let _ = send_to_terminal
                            .send(t::Printout {
                                verbosity: 0,
                                content: format!(
                                    "event loop: don't have {} amongst registered processes (got net error for it)",
                                    wrapped_network_error.source.process,
                                )
                            })
                            .await;
                    }
                }
            },
            // main message receiver: kernel filters and dispatches messages
            kernel_message = recv_in_loop.recv() => {
                let mut kernel_message = kernel_message.expect("fatal: event loop died");
                // the kernel treats the node-string "our" as a special case,
                // and replaces it with the name of the node this kernel is running.
                if kernel_message.source.node == "our" {
                    kernel_message.source.node = our.name.clone();
                }
                if kernel_message.target.node == "our" {
                    kernel_message.target.node = our.name.clone();
                }
                //
                // here: are the special kernel-level capabilities checks!
                //
                // enforce capabilities by matching from our set based on fixed format
                // enforce that if message is directed over the network, process has capability to do so
                if kernel_message.source.node == our.name
                  && kernel_message.target.node != our.name {
                    let Some(proc) = process_map.get(&kernel_message.source.process) else {
                        continue
                    };
                    if !proc.capabilities.contains(
                        &t::Capability {
                            issuer: t::Address {
                                node: our.name.clone(),
                                process: KERNEL_PROCESS_ID.clone(),
                            },
                            params: "\"network\"".into(),
                        }
                    ) {
                        // capabilities are not correct! skip this message.
                        // TODO: some kind of error thrown back at process?
                        let _ = send_to_terminal.send(
                            t::Printout {
                                verbosity: 0,
                                content: format!(
                                    "event loop: process {} doesn't have capability to send networked messages",
                                    kernel_message.source.process
                                )
                            }
                        ).await;
                        continue;
                    }
                } else if kernel_message.source.node != our.name {
                    // note that messaging restrictions only apply to *local* processes:
                    // your process can be messaged by any process remotely if it has
                    // networking capabilities.
                    let Some(persisted) = process_map.get(&kernel_message.target.process) else {
                        let _ = send_to_terminal
                            .send(t::Printout {
                                verbosity: 0,
                                content: format!(
                                    "event loop: don't have {} amongst registered processes (got message for it from network)",
                                    kernel_message.target.process,
                                )
                            })
                            .await;
                        continue;
                    };
                    if !persisted.capabilities.contains(
                            &t::Capability {
                                issuer: t::Address {
                                node: our.name.clone(),
                                process: KERNEL_PROCESS_ID.clone(),
                            },
                            params: "\"network\"".into(),
                    }) {
                        // capabilities are not correct! skip this message.
                        let _ = send_to_terminal.send(
                            t::Printout {
                                verbosity: 0,
                                content: format!(
                                    "event loop: process {} got a message from over the network, but doesn't have capability to receive networked messages",
                                    kernel_message.target.process
                                )
                            }
                        ).await;
                        continue;
                    }
                } else {
                    // enforce that local process has capability to message a target process of this name
                    // kernel and filesystem can ALWAYS message any local process
                    if kernel_message.source.process != *KERNEL_PROCESS_ID
                        && kernel_message.source.process != *STATE_PROCESS_ID
                        && kernel_message.source.process != *VFS_PROCESS_ID
                    {
                        let Some(persisted_source) = process_map.get(&kernel_message.source.process) else {
                            continue
                        };
                        let Some(persisted_target) = process_map.get(&kernel_message.target.process) else {
                            continue
                        };
                        if !persisted_target.public && !persisted_source.capabilities.contains(&t::Capability {
                                issuer: t::Address {
                                    node: our.name.clone(),
                                    process: kernel_message.target.process.clone(),
                                },
                                params: "\"messaging\"".into(),
                            }) {
                            // capabilities are not correct! skip this message.
                            // TODO some kind of error thrown back at process?
                            let _ = send_to_terminal.send(
                                t::Printout {
                                    verbosity: 0,
                                    content: format!(
                                        "event loop: process {} doesn't have capability to message process {}",
                                        kernel_message.source.process, kernel_message.target.process
                                    )
                                }
                            ).await;
                            continue;
                        }
                    }
                }
                // end capabilities checks
                // if debug mode is on, wait for user to step through
                while is_debug {
                    let debug = recv_debug_in_loop.recv().await.expect("event loop: debug channel died");
                    match debug {
                        t::DebugCommand::Toggle => is_debug = !is_debug,
                        t::DebugCommand::Step => break,
                    }
                }
                // display every single event when verbose
                let _ = send_to_terminal.send(
                        t::Printout {
                            verbosity: 3,
                            content: format!("event loop: got message: {}", kernel_message)
                        }
                    ).await;

                if our.name != kernel_message.target.node {
                    send_to_net.send(kernel_message).await.expect("fatal: net module died");
                } else if kernel_message.target.process.process() == "kernel" {
                    // kernel only accepts messages from our own node
                    if our.name != kernel_message.source.node {
                        continue;
                    }
                    match kernel_message.message {
                        t::Message::Request(_) => {
                            handle_kernel_request(
                                our.name.clone(),
                                keypair.clone(),
                                kernel_message,
                                send_to_loop.clone(),
                                send_to_terminal.clone(),
                                &mut senders,
                                &mut process_handles,
                                &mut process_map,
                                caps_oracle_sender.clone(),
                                &engine,
                            ).await;
                        }
                        t::Message::Response(_) => {
                            handle_kernel_response(
                                our.name.clone(),
                                keypair.clone(),
                                kernel_message,
                                send_to_loop.clone(),
                                send_to_terminal.clone(),
                                &mut senders,
                                &mut process_handles,
                                &mut process_map,
                                caps_oracle_sender.clone(),
                                &engine,
                            ).await;
                        }
                    }
                } else {
                    // pass message to appropriate runtime module or process
                    match senders.get(&kernel_message.target.process) {
                        Some(ProcessSender::Userspace(sender)) => {
                            let target = kernel_message.target.process.clone();
                            match sender.send(Ok(kernel_message)).await {
                                Ok(()) => continue,
                                Err(_e) => {
                                    let _ = send_to_terminal
                                        .send(t::Printout {
                                            verbosity: 0,
                                            content: format!(
                                                "event loop: process {} appears to have died",
                                                target
                                            )
                                        })
                                        .await;
                                }
                            }
                        }
                        Some(ProcessSender::Runtime(sender)) => {
                            sender.send(kernel_message).await.expect("event loop: fatal: runtime module died");
                        }
                        None => {
                            send_to_terminal
                                .send(t::Printout {
                                    verbosity: 0,
                                    content: format!(
                                        "event loop: don't have {:?} amongst registered processes, got message for it: {}",
                                        kernel_message.target.process,
                                        kernel_message,
                                    )
                                })
                                .await
                                .expect("event loop: fatal: terminal sender died");
                        }
                    }
                }
            },
            // capabilities oracle: handles all requests to add, drop, and check capabilities
            Some(cap_message) = caps_oracle_receiver.recv() => {
                match cap_message {
                    t::CapMessage::Add { on, cap, responder } => {
                        // insert cap in process map
                        let Some(entry) = process_map.get_mut(&on) else {
                            let _ = responder.send(false);
                            continue;
                        };
                        entry.capabilities.insert(cap);
                        let _ = persist_state(&our.name, &send_to_loop, &process_map).await;
                        let _ = responder.send(true);
                    },
                    t::CapMessage::_Drop { on, cap, responder } => {
                        // remove cap from process map
                        let Some(entry) = process_map.get_mut(&on) else {
                            let _ = responder.send(false);
                            continue;
                        };
                        entry.capabilities.remove(&cap);
                        let _ = persist_state(&our.name, &send_to_loop, &process_map).await;
                        let _ = responder.send(true);
                    },
                    t::CapMessage::Has { on, cap, responder } => {
                        // return boolean on responder
                        let _ = responder.send(
                            match process_map.get(&on) {
                                None => false,
                                Some(p) => p.capabilities.contains(&cap),
                            }
                        );
                    },
                    t::CapMessage::GetAll { on, responder } => {
                        // return all caps, signed, on responder
                        let _ = responder.send(
                            match process_map.get(&on) {
                                None => HashSet::new(),
                                Some(p) => p.capabilities.clone().iter().map(|cap| t::SignedCapability {
                                    issuer: cap.issuer.clone(),
                                    params: cap.params.clone(),
                                    signature: keypair
                                        .sign(&rmp_serde::to_vec(&cap).unwrap())
                                        .as_ref()
                                        .to_vec(),
                                })
                                .collect(),
                            }
                        );
                    },
                    t::CapMessage::GetSome { on, caps, responder } => {
                        let _ = responder.send(
                            match process_map.get(&on) {
                                None => HashSet::new(),
                                Some(p) => {
                                    caps.iter().filter_map(|cap| {
                                        // if it is in our store, attach the signed versions
                                        if p.capabilities.contains(cap) ||
                                            // if this process is issuing the cap, sign it
                                            (cap.issuer.node == our.name && cap.issuer.process == on) {
                                            Some(t::SignedCapability {
                                                issuer: cap.issuer.clone(),
                                                params: cap.params.clone(),
                                                signature: keypair
                                                    .sign(&rmp_serde::to_vec(&cap).unwrap())
                                                    .as_ref()
                                                    .to_vec(),
                                            })
                                        // otherwise this is a bogus capability, discard it
                                        } else { None }
                                    }).collect()
                                },
                            }
                        );
                    }
                }
            }
        }
    }
}
