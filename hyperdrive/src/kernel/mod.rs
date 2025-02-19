use lib::types::core::{self as t, KERNEL_PROCESS_ID, STATE_PROCESS_ID, VFS_PROCESS_ID};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};
use tokio::{
    sync::{mpsc, Mutex},
    task::JoinHandle,
};
use wasmtime::{Config, Engine, WasmBacktraceDetails};

/// Manipulate a single process.
pub mod process;
/// Implement the functions served to processes by `wit-v1.0.0/hyperware.wit`.
mod standard_host_v1;

pub const LATEST_WIT_VERSION: u32 = 1;
const PROCESS_CHANNEL_CAPACITY: usize = 100;

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
type ProcessHandles = HashMap<t::ProcessId, JoinHandle<anyhow::Result<()>>>;

enum ProcessSender {
    Runtime {
        sender: t::MessageSender,
        net_errors: Option<t::NetworkErrorSender>,
    },
    Userspace(t::ProcessMessageSender),
}

pub type ProcessRestartBackoffs = HashMap<t::ProcessId, Arc<Mutex<Option<RestartBackoff>>>>;

pub struct RestartBackoff {
    /// if try to restart before this:
    ///  * wait till `next_soonest_restart_time`
    ///  * increment `consecutive_attempts`
    /// else if try to restart after this:
    ///  * set `consecutive_attempts = 0`,
    /// and in either case:
    ///  set `next_soonest_restart_time += 2 ** consecutive_attempts` seconds
    next_soonest_restart_time: tokio::time::Instant,
    /// how many times has process tried to restart in a row
    consecutive_attempts: u32,
    /// task that will do the restart after wait time has elapsed
    _restart_handle: Option<JoinHandle<()>>,
}

/// persist kernel's process_map state for next bootup
/// TODO refactor this to hit the DB directly for performance's sake
async fn persist_state(send_to_loop: &t::MessageSender, process_map: &t::ProcessMap) {
    t::KernelMessage::builder()
        .id(rand::random())
        .source(("our", KERNEL_PROCESS_ID.clone()))
        .target(("our", STATE_PROCESS_ID.clone()))
        .message(t::Message::Request(t::Request {
            inherit: false,
            expects_response: None,
            body: serde_json::to_vec(&t::StateAction::SetState(KERNEL_PROCESS_ID.clone())).unwrap(),
            metadata: None,
            capabilities: vec![],
        }))
        .lazy_load_blob(Some(t::LazyLoadBlob {
            mime: None,
            bytes: bincode::serialize(process_map)
                .expect("fatal: kernel couldn't serialize process map"),
        }))
        .build()
        .unwrap()
        .send(send_to_loop)
        .await;
}

/// handle commands inside messages sent directly to kernel. source must be our own node.
/// returns Some(()) if the kernel should shut down.
async fn handle_kernel_request(
    our_name: &str,
    keypair: &Arc<ring::signature::Ed25519KeyPair>,
    km: t::KernelMessage,
    send_to_loop: &t::MessageSender,
    send_to_terminal: &t::PrintSender,
    senders: &mut Senders,
    process_handles: &mut ProcessHandles,
    process_map: &mut t::ProcessMap,
    caps_oracle: &t::CapMessageSender,
    engine: &Engine,
    home_directory_path: &PathBuf,
    process_restart_backoffs: &mut ProcessRestartBackoffs,
) -> Option<()> {
    let t::Message::Request(request) = km.message else {
        return None;
    };
    let command: t::KernelCommand = match serde_json::from_slice(&request.body) {
        Err(e) => {
            t::Printout::new(
                0,
                KERNEL_PROCESS_ID.clone(),
                format!("kernel: couldn't parse command: {e:?}"),
            )
            .send(send_to_terminal)
            .await;
            return None;
        }
        Ok(c) => c,
    };
    match command {
        t::KernelCommand::Shutdown => {
            for handle in process_handles.values() {
                handle.abort();
            }
            Some(())
        }
        //
        // sent from kernel to kernel: we've completed boot sequence, and can
        // now go ahead and actually start executing persisted userspace processes
        //
        t::KernelCommand::Booted => {
            for (process_id, process_sender) in senders {
                let ProcessSender::Userspace(sender) = process_sender else {
                    continue;
                };
                sender
                    .send(Ok(t::KernelMessage::builder()
                        .id(km.id)
                        .source((our_name, KERNEL_PROCESS_ID.clone()))
                        .target((our_name, process_id))
                        .message(t::Message::Request(t::Request {
                            inherit: false,
                            expects_response: None,
                            body: b"run".to_vec(),
                            metadata: None,
                            capabilities: vec![],
                        }))
                        .build()
                        .unwrap()))
                    .await
                    .expect("fatal: kernel couldn't send run message to process");
            }
            None
        }
        //
        // initialize a new process. this is the only way to create a new process.
        //
        t::KernelCommand::InitializeProcess {
            id,
            wasm_bytes_handle,
            wit_version,
            on_exit,
            initial_capabilities,
            public,
        } => {
            let Some(blob) = km.lazy_load_blob else {
                t::Printout::new(
                    0,
                    KERNEL_PROCESS_ID.clone(),
                    "kernel: process startup requires bytes",
                )
                .send(send_to_terminal)
                .await;
                // fire an error back
                t::KernelMessage::builder()
                    .id(km.id)
                    .source((our_name, KERNEL_PROCESS_ID.clone()))
                    .target(km.rsvp.unwrap_or(km.source))
                    .message(t::Message::Response((
                        t::Response {
                            inherit: false,
                            body: serde_json::to_vec(&t::KernelResponse::InitializeProcessError)
                                .unwrap(),
                            metadata: None,
                            capabilities: vec![],
                        },
                        None,
                    )))
                    .build()
                    .unwrap()
                    .send(send_to_loop)
                    .await;
                return None;
            };
            if let Err(e) = t::check_process_id_hypermap_safe(&id) {
                t::Printout::new(0, KERNEL_PROCESS_ID.clone(), &format!("kernel: {e}"))
                    .send(send_to_terminal)
                    .await;
                // fire an error back
                t::KernelMessage::builder()
                    .id(km.id)
                    .source((our_name, KERNEL_PROCESS_ID.clone()))
                    .target(km.rsvp.unwrap_or(km.source))
                    .message(t::Message::Response((
                        t::Response {
                            inherit: false,
                            body: serde_json::to_vec(&t::KernelResponse::InitializeProcessError)
                                .unwrap(),
                            metadata: None,
                            capabilities: vec![],
                        },
                        None,
                    )))
                    .build()
                    .unwrap()
                    .send(send_to_loop)
                    .await;
                return None;
            }

            // check cap sigs & transform valid to unsigned to be plugged into procs
            let parent_caps: &HashMap<t::Capability, Vec<u8>> =
                &process_map.get(&km.source.process).unwrap().capabilities;
            let mut valid_capabilities: HashMap<t::Capability, Vec<u8>> = HashMap::new();
            if km.source.process == "kernel:distro:sys" {
                for cap in initial_capabilities {
                    let sig = keypair.sign(&rmp_serde::to_vec(&cap).unwrap());
                    valid_capabilities.insert(cap, sig.as_ref().to_vec());
                }
            } else {
                for cap in initial_capabilities {
                    match parent_caps.get(&cap) {
                        // NOTE: verifying sigs here would be unnecessary
                        Some(sig) => {
                            valid_capabilities.insert(cap, sig.to_vec());
                        }
                        None => {
                            t::Printout::new(
                                    0,
                                    KERNEL_PROCESS_ID.clone(),
                                    format!(
                                        "kernel: InitializeProcess caller {} doesn't have capability {}",
                                        km.source.process,
                                        cap
                                    )
                                )
                                .send(send_to_terminal)
                                .await;
                        }
                    }
                }
            }
            // give the initializer and itself the messaging cap.
            // NOTE: we do this even if the process is public, because
            // a process might redundantly call grant_capabilities.
            let msg_cap = t::Capability::messaging((our_name, &id));
            let cap_sig = keypair.sign(&rmp_serde::to_vec(&msg_cap).unwrap());
            valid_capabilities.insert(msg_cap.clone(), cap_sig.as_ref().to_vec());

            caps_oracle
                .send(t::CapMessage::Add {
                    on: km.source.process.clone(),
                    caps: vec![msg_cap],
                    responder: None,
                })
                .await
                .expect("event loop: fatal: sender died");

            let start_process_metadata = StartProcessMetadata {
                source: if let Some(ref rsvp) = km.rsvp {
                    rsvp.clone()
                } else {
                    km.source.clone()
                },
                process_id: id,
                persisted: t::PersistedProcess {
                    wasm_bytes_handle,
                    wit_version,
                    on_exit,
                    capabilities: valid_capabilities,
                    public,
                },
                reboot: false,
            };
            let response = match start_process(
                our_name,
                keypair.clone(),
                blob.bytes,
                send_to_loop,
                send_to_terminal,
                senders,
                process_handles,
                engine,
                caps_oracle,
                &start_process_metadata,
                &home_directory_path,
                process_restart_backoffs,
            )
            .await
            {
                Ok(()) => {
                    let on_exit_none = start_process_metadata.persisted.on_exit.is_none();
                    process_map.insert(
                        start_process_metadata.process_id,
                        start_process_metadata.persisted,
                    );
                    if !start_process_metadata.reboot && !on_exit_none {
                        // if new, and not totally transient, persist
                        persist_state(&send_to_loop, process_map).await;
                    }
                    t::KernelResponse::InitializedProcess
                }
                Err(e) => {
                    t::Printout::new(
                        0,
                        KERNEL_PROCESS_ID.clone(),
                        format!("kernel: error initializing process: {e:?}"),
                    )
                    .send(send_to_terminal)
                    .await;
                    t::KernelResponse::InitializeProcessError
                }
            };
            t::KernelMessage::builder()
                .id(km.id)
                .source(("our", KERNEL_PROCESS_ID.clone()))
                .target(km.rsvp.unwrap_or(km.source))
                .message(t::Message::Response((
                    t::Response {
                        inherit: false,
                        body: serde_json::to_vec(&response).unwrap(),
                        metadata: None,
                        capabilities: vec![],
                    },
                    None,
                )))
                .build()
                .unwrap()
                .send(send_to_loop)
                .await;
            None
        }
        t::KernelCommand::GrantCapabilities {
            target,
            capabilities,
        } => {
            caps_oracle
                .send(t::CapMessage::Add {
                    on: target,
                    caps: capabilities,
                    responder: None,
                })
                .await
                .expect("event loop: fatal: sender died");
            None
        }
        t::KernelCommand::DropCapabilities {
            target,
            capabilities,
        } => {
            caps_oracle
                .send(t::CapMessage::Drop {
                    on: target,
                    caps: capabilities,
                    responder: None,
                })
                .await
                .expect("event loop: fatal: sender died");
            None
        }
        t::KernelCommand::SetOnExit { target, on_exit } => {
            if let Some(process) = process_map.get_mut(&target) {
                process.on_exit = on_exit;
            }
            // persist state because it changed
            persist_state(&send_to_loop, process_map).await;
            None
        }
        //
        // send 'run' message to a process that's already been initialized
        //
        t::KernelCommand::RunProcess(process_id) => {
            let response =
                if let Some(ProcessSender::Userspace(process_sender)) = senders.get(&process_id) {
                    if let Ok(()) = process_sender
                        .send(Ok(t::KernelMessage::builder()
                            .id(rand::random())
                            .source((our_name, KERNEL_PROCESS_ID.clone()))
                            .target((our_name, &process_id))
                            .message(t::Message::Request(t::Request {
                                inherit: false,
                                expects_response: None,
                                body: b"run".to_vec(),
                                metadata: None,
                                capabilities: vec![],
                            }))
                            .build()
                            .unwrap()))
                        .await
                    {
                        t::KernelResponse::StartedProcess
                    } else {
                        t::KernelResponse::RunProcessError
                    }
                } else {
                    t::Printout::new(
                        0,
                        KERNEL_PROCESS_ID.clone(),
                        format!("kernel: no such process {process_id} to run"),
                    )
                    .send(send_to_terminal)
                    .await;
                    t::KernelResponse::RunProcessError
                };
            t::KernelMessage::builder()
                .id(km.id)
                .source(("our", KERNEL_PROCESS_ID.clone()))
                .target(km.rsvp.unwrap_or(km.source))
                .message(t::Message::Response((
                    t::Response {
                        inherit: false,
                        body: serde_json::to_vec(&response).unwrap(),
                        metadata: None,
                        capabilities: vec![],
                    },
                    None,
                )))
                .build()
                .unwrap()
                .send(send_to_loop)
                .await;
            None
        }
        //
        // brutal and savage killing: aborting the task.
        // do not do this to a process if you don't want to risk
        // dropped messages / un-replied-to-requests
        // if you want to immediately restart a process or otherwise
        // skip the capabilities-cleanup RevokeAll, pass "no-revoke" in the metadata
        //
        t::KernelCommand::KillProcess(process_id) => {
            let process_handle = match process_handles.remove(&process_id) {
                Some(ph) => ph,
                None => {
                    t::Printout::new(
                        2,
                        KERNEL_PROCESS_ID.clone(),
                        format!("kernel: no such process {process_id} to kill"),
                    )
                    .send(send_to_terminal)
                    .await;
                    return None;
                }
            };
            senders.remove(&process_id);
            process_handle.abort();
            process_map.remove(&process_id);
            if request.metadata != Some("no-revoke".to_string()) {
                caps_oracle
                    .send(t::CapMessage::RevokeAll {
                        on: process_id.clone(),
                        responder: None,
                    })
                    .await
                    .expect("event loop: fatal: sender died");
            }
            if request.expects_response.is_none() {
                t::Printout::new(
                    2,
                    KERNEL_PROCESS_ID.clone(),
                    format!("kernel: killing process {process_id}"),
                )
                .send(send_to_terminal)
                .await;
                return None;
            }
            t::Printout::new(
                0,
                KERNEL_PROCESS_ID.clone(),
                format!("kernel: killing process {process_id}"),
            )
            .send(send_to_terminal)
            .await;
            t::KernelMessage::builder()
                .id(km.id)
                .source(("our", KERNEL_PROCESS_ID.clone()))
                .target(km.rsvp.unwrap_or(km.source))
                .message(t::Message::Response((
                    t::Response {
                        inherit: false,
                        body: serde_json::to_vec(&t::KernelResponse::KilledProcess(process_id))
                            .unwrap(),
                        metadata: None,
                        capabilities: vec![],
                    },
                    None,
                )))
                .build()
                .unwrap()
                .send(send_to_loop)
                .await;
            None
        }
        t::KernelCommand::Debug(kind) => {
            let response = match kind {
                t::KernelPrint::ProcessMap => t::KernelPrintResponse::ProcessMap(
                    process_map
                        .clone()
                        .into_iter()
                        .map(|(k, v)| (k, v.into()))
                        .collect(),
                ),
                t::KernelPrint::Process(process_id) => t::KernelPrintResponse::Process(
                    process_map.get(&process_id).cloned().map(|p| p.into()),
                ),
                t::KernelPrint::HasCap { on, cap } => t::KernelPrintResponse::HasCap(
                    process_map
                        .get(&on)
                        .map(|p| p.capabilities.contains_key(&cap)),
                ),
            };
            t::KernelMessage::builder()
                .id(km.id)
                .source(("our", KERNEL_PROCESS_ID.clone()))
                .target(km.rsvp.unwrap_or(km.source))
                .message(t::Message::Response((
                    t::Response {
                        inherit: false,
                        body: serde_json::to_vec(&t::KernelResponse::Debug(response)).unwrap(),
                        metadata: None,
                        capabilities: vec![],
                    },
                    None,
                )))
                .build()
                .unwrap()
                .send(send_to_loop)
                .await;
            None
        }
    }
}

/// spawn a process loop and insert the process in the relevant kernel state maps
async fn start_process(
    our_name: &str,
    keypair: Arc<ring::signature::Ed25519KeyPair>,
    km_blob_bytes: Vec<u8>,
    send_to_loop: &t::MessageSender,
    send_to_terminal: &t::PrintSender,
    senders: &mut Senders,
    process_handles: &mut ProcessHandles,
    engine: &Engine,
    caps_oracle: &t::CapMessageSender,
    process_metadata: &StartProcessMetadata,
    home_directory_path: &PathBuf,
    process_restart_backoffs: &mut ProcessRestartBackoffs,
) -> anyhow::Result<()> {
    let (send_to_process, recv_in_process) =
        mpsc::channel::<Result<t::KernelMessage, t::WrappedSendError>>(PROCESS_CHANNEL_CAPACITY);
    let id = &process_metadata.process_id;
    if senders.contains_key(id) {
        return Err(anyhow::anyhow!("process with ID {id} already exists"));
    }
    senders.insert(
        id.clone(),
        ProcessSender::Userspace(send_to_process.clone()),
    );
    let metadata = t::ProcessMetadata {
        our: t::Address {
            node: our_name.to_string(),
            process: id.clone(),
        },
        wasm_bytes_handle: process_metadata.persisted.wasm_bytes_handle.clone(),
        wit_version: process_metadata.persisted.wit_version,
        on_exit: process_metadata.persisted.on_exit.clone(),
        public: process_metadata.persisted.public,
    };
    let maybe_restart_backoff = if let t::OnExit::Restart = process_metadata.persisted.on_exit {
        let restart_backoff = process_restart_backoffs
            .remove(id)
            .unwrap_or_else(|| Arc::new(Mutex::new(None)));
        process_restart_backoffs.insert(id.clone(), Arc::clone(&restart_backoff));
        Some(restart_backoff)
    } else {
        None
    };
    process_handles.insert(
        id.clone(),
        tokio::spawn(process::make_process_loop(
            keypair.clone(),
            metadata,
            send_to_loop.clone(),
            send_to_terminal.clone(),
            recv_in_process,
            send_to_process,
            km_blob_bytes,
            caps_oracle.clone(),
            engine.clone(),
            home_directory_path.clone(),
            maybe_restart_backoff,
        )),
    );
    Ok(())
}

/// the OS kernel. contains event loop which handles all message-passing between
/// all processes (Wasm apps) and also runtime tasks.
pub async fn kernel(
    our: t::Identity,
    keypair: Arc<ring::signature::Ed25519KeyPair>,
    mut process_map: t::ProcessMap,
    mut reverse_cap_index: t::ReverseCapIndex,
    caps_oracle_sender: t::CapMessageSender,
    mut caps_oracle_receiver: t::CapMessageReceiver,
    send_to_loop: t::MessageSender,
    send_to_terminal: t::PrintSender,
    mut recv_in_loop: t::MessageReceiver,
    mut network_error_recv: t::NetworkErrorReceiver,
    mut recv_debug_in_loop: t::DebugReceiver,
    send_to_net: t::MessageSender,
    home_directory_path: PathBuf,
    runtime_extensions: Vec<(
        t::ProcessId,
        t::MessageSender,
        Option<t::NetworkErrorSender>,
        bool,
    )>,
    default_pki_entries: Vec<t::HnsUpdate>,
) -> anyhow::Result<()> {
    let mut config = Config::new();
    config.cache_config_load_default().unwrap();
    config.wasm_backtrace_details(WasmBacktraceDetails::Enable);
    config.wasm_component_model(true);
    config.async_support(true);
    let engine = Engine::new(&config).unwrap();

    let vfs_path = home_directory_path.join("vfs");
    tokio::fs::create_dir_all(&vfs_path)
        .await
        .expect("kernel startup fatal: couldn't create vfs dir");

    let mut senders: Senders = HashMap::with_capacity(process_map.len() + runtime_extensions.len());
    senders.insert(
        t::ProcessId::new(Some("net"), "distro", "sys"),
        ProcessSender::Runtime {
            sender: send_to_net.clone(),
            net_errors: None, // networking module does not accept net errors sent to it
        },
    );
    for (process_id, sender, net_error_sender, _) in runtime_extensions {
        senders.insert(
            process_id,
            ProcessSender::Runtime {
                sender,
                net_errors: net_error_sender,
            },
        );
    }

    // each running process is stored in this map
    let mut process_handles: ProcessHandles = HashMap::with_capacity(process_map.len());

    let mut in_stepthrough_mode: bool = false;
    // this flag starts as true, and terminal will alert us if we can
    // skip sending prints for every event.
    let mut print_full_event_loop: bool = true;

    let mut print_full_event_loop_for_process: HashSet<t::ProcessId> = HashSet::new();

    // create a list of processes which are successfully rebooted,
    // keeping only them in the updated post-boot process map
    let mut non_rebooted_processes: HashSet<t::ProcessId> = HashSet::new();

    let mut process_restart_backoffs: ProcessRestartBackoffs = HashMap::new();

    for (process_id, persisted) in &process_map {
        // runtime extensions will have a bytes_handle of "", because they have no
        // Wasm code saved in filesystem.
        if persisted.wasm_bytes_handle.is_empty() {
            continue;
        }
        let wasm_bytes_handle = persisted
            .wasm_bytes_handle
            .strip_prefix("/")
            .unwrap_or_else(|| &persisted.wasm_bytes_handle);
        #[cfg(unix)]
        let path = vfs_path.join(wasm_bytes_handle);
        #[cfg(target_os = "windows")]
        let path = vfs_path.join(wasm_bytes_handle.replace(":", "_"));

        // read wasm bytes directly from vfs
        let wasm_bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(e) => {
                t::Printout::new(
                    0,
                    KERNEL_PROCESS_ID.clone(),
                    format!("kernel: couldn't read wasm bytes for process: {process_id} at {path:?}: {e}"),
                )
                .send(&send_to_terminal)
                .await;
                non_rebooted_processes.insert(process_id.clone());
                continue;
            }
        };
        if let t::OnExit::Requests(requests) = &persisted.on_exit {
            // if a persisted process had on-death-requests, we should perform them now
            // even in death, a process can only message processes it has capabilities for
            for (address, request, blob) in requests {
                // the process that made the request is dead, so never expects response
                let mut request = request.to_owned();
                request.expects_response = None;
                // TODO not sure if we need to verify the signature
                if persisted
                    .capabilities
                    .contains_key(&t::Capability::messaging(address.clone()))
                {
                    t::KernelMessage::builder()
                        .id(rand::random())
                        .source((&our.name, process_id))
                        .target(address.clone())
                        .message(t::Message::Request(request))
                        .lazy_load_blob(blob.clone())
                        .build()
                        .unwrap()
                        .send(&send_to_loop)
                        .await;
                }
            }
        }

        let start_process_metadata = StartProcessMetadata {
            source: t::Address {
                node: our.name.clone(),
                process: KERNEL_PROCESS_ID.clone(),
            },
            process_id: process_id.clone(),
            persisted: persisted.clone(),
            reboot: true,
        };

        match start_process(
            &our.name,
            keypair.clone(),
            wasm_bytes,
            &send_to_loop,
            &send_to_terminal,
            &mut senders,
            &mut process_handles,
            &engine,
            &caps_oracle_sender,
            &start_process_metadata,
            &home_directory_path,
            &mut process_restart_backoffs,
        )
        .await
        {
            Ok(()) => {}
            Err(e) => {
                t::Printout::new(
                    0,
                    KERNEL_PROCESS_ID.clone(),
                    format!("kernel: couldn't reboot process: {e}"),
                )
                .send(&send_to_terminal)
                .await;
                non_rebooted_processes.insert(process_id.clone());
            }
        }
    }

    process_map.retain(|process_id, _| !non_rebooted_processes.contains(process_id));

    // persist new state
    persist_state(&send_to_loop, &process_map).await;

    // after all bootstrapping messages are handled, send a Booted kernelcommand
    // to turn it on
    t::KernelMessage::builder()
        .id(rand::random())
        .source((&our.name, KERNEL_PROCESS_ID.clone()))
        .target((&our.name, KERNEL_PROCESS_ID.clone()))
        .message(t::Message::Request(t::Request {
            inherit: true,
            expects_response: None,
            body: serde_json::to_vec(&t::KernelCommand::Booted).unwrap(),
            metadata: None,
            capabilities: vec![],
        }))
        .build()
        .unwrap()
        .send(&send_to_loop)
        .await;

    // sending hard coded pki entries into networking for bootstrapped rpc
    t::KernelMessage::builder()
        .id(rand::random())
        .source((&our.name, KERNEL_PROCESS_ID.clone()))
        .target((our.name.as_str(), "net", "distro", "sys"))
        .message(t::Message::Request(t::Request {
            inherit: false,
            expects_response: None,
            body: rmp_serde::to_vec(&t::NetAction::HnsBatchUpdate(default_pki_entries)).unwrap(),
            metadata: None,
            capabilities: vec![],
        }))
        .build()
        .unwrap()
        .send(&send_to_loop)
        .await;

    // main event loop
    loop {
        tokio::select! {
            // debug mode toggle: when on, this loop becomes a manual step-through
            Some(debug_command) = recv_debug_in_loop.recv() => {
                match debug_command {
                    t::DebugCommand::ToggleStepthrough => {
                        in_stepthrough_mode = !in_stepthrough_mode;
                    },
                    t::DebugCommand::Step => {
                        // can't step here, must be in stepthrough-mode
                    },
                    t::DebugCommand::ToggleEventLoop => {
                        print_full_event_loop = !print_full_event_loop;
                    }
                    t::DebugCommand::ToggleEventLoopForProcess(ref process) => {
                        if print_full_event_loop_for_process.contains(process) {
                            print_full_event_loop_for_process.remove(process);
                        } else {
                            print_full_event_loop_for_process.insert(process.clone());
                        }
                    }
                }
            },
            // network error message receiver: handle `timeout` and `offline` errors
            // directly from the networking task in runtime, and filter them to the
            // sender of the original attempted message.
            Some(wrapped_network_error) = network_error_recv.recv() => {
                // display every single event when verbose
                if print_full_event_loop {
                    t::Printout::new(3, KERNEL_PROCESS_ID.clone(), format!("{wrapped_network_error:?}")).send(&send_to_terminal).await;
                } else if print_full_event_loop_for_process.contains(&wrapped_network_error.source.process) && wrapped_network_error.source.node == our.name {
                    t::Printout::new(3, wrapped_network_error.source.process.clone(), format!("{wrapped_network_error:?}")).send(&send_to_terminal).await;
                } else if print_full_event_loop_for_process.contains(&wrapped_network_error.error.target.process) && wrapped_network_error.error.target.node == our.name {
                    t::Printout::new(3, wrapped_network_error.error.target.process.clone(), format!("{wrapped_network_error:?}")).send(&send_to_terminal).await;
                }
                // forward the error to the relevant process
                match senders.get(&wrapped_network_error.source.process) {
                    Some(ProcessSender::Userspace(sender)) => {
                        sender.send(Err(wrapped_network_error)).await.ok();
                    }
                    Some(ProcessSender::Runtime { net_errors, .. }) => {
                        if let Some(net_errors) = net_errors {
                            net_errors.send(wrapped_network_error).await.ok();
                        }
                    }
                    None => {
                        t::Printout::new(
                            0,
                            KERNEL_PROCESS_ID.clone(),
                            format!(
                                "event loop: {} failed to deliver a message {}; but process has already terminated",
                                wrapped_network_error.source.process,
                                match wrapped_network_error.error.kind {
                                    t::SendErrorKind::Timeout => "due to timeout",
                                    t::SendErrorKind::Offline => "because the receiver is offline",
                                },
                            )
                        ).send(&send_to_terminal).await;
                    }
                }
            },
            // main message receiver: kernel filters and dispatches messages
            Some(mut kernel_message) = recv_in_loop.recv() => {
                // the kernel treats the node-string "our" as a special case,
                // and replaces it with the name of the node this kernel is running.
                if kernel_message.source.node == "our" {
                    kernel_message.source.node = our.name.clone();
                }
                if kernel_message.target.node == "our" {
                    kernel_message.target.node = our.name.clone();
                }
                //
                // here are the special kernel-level capabilities checks!
                //
                // enforce capabilities by matching from our set based on fixed format
                // enforce that if message is directed over the network, process has capability to do so
                if kernel_message.source.node == our.name
                  && kernel_message.target.node != our.name {
                    let Some(proc) = process_map.get(&kernel_message.source.process) else {
                        continue;
                    };
                    if !proc.capabilities.contains_key(
                        &t::Capability::new((&our.name, KERNEL_PROCESS_ID.clone()), "\"network\"")
                    ) {
                        // capabilities are not correct! skip this message.
                        t::Printout::new(
                            0,
                            KERNEL_PROCESS_ID.clone(),
                            format!(
                                "event loop: process {} doesn't have capability to send networked messages",
                                kernel_message.source.process
                            )
                        ).send(&send_to_terminal).await;
                        t::Printout::new(
                            0,
                            KERNEL_PROCESS_ID.clone(),
                            format!("their capabilities: {:?}", proc.capabilities)
                        ).send(&send_to_terminal).await;
                        throw_timeout(&our.name, &senders, kernel_message).await;
                        continue;
                    }
                } else if kernel_message.source.node != our.name {
                    // note that messaging restrictions only apply to *local* processes:
                    // your process can be messaged by any process remotely if it has
                    // networking capabilities.
                    let Some(persisted) = process_map.get(&kernel_message.target.process) else {
                        t::Printout::new(
                            2,
                            KERNEL_PROCESS_ID.clone(),
                            format!(
                                "event loop: got {} from network for {}, but process does not exist{}",
                                match kernel_message.message {
                                    t::Message::Request(_) => "Request",
                                    t::Message::Response(_) => "Response",
                                },
                                kernel_message.target.process,
                                match kernel_message.message {
                                    t::Message::Request(_) => "",
                                    t::Message::Response(_) =>
                                        "\nhint: if you are using `m`, try awaiting the Response: `m --await 5 ...`",
                                }
                            )
                        ).send(&send_to_terminal).await;
                        continue;
                    };
                    if !persisted.capabilities.contains_key(
                        &t::Capability::new((&our.name, KERNEL_PROCESS_ID.clone()), "\"network\"")
                    ) {
                        // capabilities are not correct! skip this message.
                        t::Printout::new(
                            0,
                            KERNEL_PROCESS_ID.clone(),
                            format!(
                                "event loop: process {} got a message from over the network, but doesn't have capability to receive networked messages",
                                kernel_message.target.process
                            )
                        ).send(&send_to_terminal).await;
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
                            throw_timeout(&our.name, &senders, kernel_message).await;
                            continue;
                        };
                        let Some(persisted_target) = process_map.get(&kernel_message.target.process) else {
                            t::Printout::new(
                                2,
                                KERNEL_PROCESS_ID.clone(),
                                format!(
                                    "event loop: process {} sent message to non-existing {}; dropping message",
                                    kernel_message.source.process, kernel_message.target.process
                                )
                            ).send(&send_to_terminal).await;
                            throw_timeout(&our.name, &senders, kernel_message).await;
                            continue;
                        };
                        if !persisted_target.public
                        && !persisted_source.capabilities.contains_key(
                            &t::Capability::messaging((&our.name, &kernel_message.target.process))
                        ) {
                            // capabilities are not correct! skip this message.
                            t::Printout::new(
                                0,
                                KERNEL_PROCESS_ID.clone(),
                                format!(
                                    "event loop: process {} doesn't have capability to message process {}",
                                    kernel_message.source.process, kernel_message.target.process
                                )
                            ).send(&send_to_terminal).await;
                            throw_timeout(&our.name, &senders, kernel_message).await;
                            continue;
                        }
                    }
                }
                // end capabilities checks

                // if debug mode is on, wait for user to step through
                while in_stepthrough_mode {
                    let debug = recv_debug_in_loop.recv().await.expect("event loop: debug channel died");
                    match debug {
                        t::DebugCommand::ToggleStepthrough => in_stepthrough_mode = !in_stepthrough_mode,
                        t::DebugCommand::Step => break,
                        t::DebugCommand::ToggleEventLoop => print_full_event_loop = !print_full_event_loop,
                        t::DebugCommand::ToggleEventLoopForProcess(ref process) => {
                            if print_full_event_loop_for_process.contains(process) {
                                print_full_event_loop_for_process.remove(process);
                            } else {
                                print_full_event_loop_for_process.insert(process.clone());
                            }
                        }
                    }
                }
                // display every single event when verbose
                if print_full_event_loop {
                    t::Printout::new(3, KERNEL_PROCESS_ID.clone(), format!("{kernel_message}")).send(&send_to_terminal).await;
                } else if print_full_event_loop_for_process.contains(&kernel_message.source.process) && kernel_message.source.node == our.name {
                    t::Printout::new(3, kernel_message.source.process.clone(), format!("{kernel_message}")).send(&send_to_terminal).await;
                } else if print_full_event_loop_for_process.contains(&kernel_message.target.process) && kernel_message.target.node == our.name {
                    t::Printout::new(3, kernel_message.target.process.clone(), format!("{kernel_message}")).send(&send_to_terminal).await;
                }

                if our.name != kernel_message.target.node {
                    // handle messages sent over network
                    send_to_net.send(kernel_message).await.expect("fatal: net module died");
                } else if kernel_message.target.process.process() == "kernel" && kernel_message.source.node == our.name {
                    // handle messages sent to local kernel
                    if let Some(()) = handle_kernel_request(
                        &our.name,
                        &keypair,
                        kernel_message,
                        &send_to_loop,
                        &send_to_terminal,
                        &mut senders,
                        &mut process_handles,
                        &mut process_map,
                        &caps_oracle_sender,
                        &engine,
                        &home_directory_path,
                        &mut process_restart_backoffs,
                    ).await {
                        // drain process map of processes with OnExit::None
                        process_map.retain(|_, persisted| !persisted.on_exit.is_none());
                        // persist state
                        persist_state(&send_to_loop, &process_map).await;
                        // shut down the node
                        return Ok(());
                    }
                } else {
                    // pass message to appropriate runtime module or process
                    match senders.get(&kernel_message.target.process) {
                        Some(ProcessSender::Userspace(sender)) => {
                            sender.send(Ok(kernel_message)).await.ok();
                        }
                        Some(ProcessSender::Runtime { sender, .. }) => {
                            sender.send(kernel_message).await.expect("event loop: fatal: runtime module died");
                        }
                        None => {
                            t::Printout::new(
                                0,
                                KERNEL_PROCESS_ID.clone(),
                                format!(
                                    "event loop: got {} from {:?} for {:?}, but target doesn't exist (perhaps it terminated): {}",
                                    match kernel_message.message {
                                        t::Message::Request(_) => "Request",
                                        t::Message::Response(_) => "Response",
                                    },
                                    kernel_message.source.process,
                                    kernel_message.target.process,
                                    kernel_message,
                                )
                            ).send(&send_to_terminal).await;
                            throw_timeout(&our.name, &senders, kernel_message).await;
                        }
                    }
                }
            },
            // capabilities oracle: handles all requests to add, drop, and check capabilities
            Some(cap_message) = caps_oracle_receiver.recv() => {
                if print_full_event_loop {
                    t::Printout::new(3, KERNEL_PROCESS_ID.clone(), format!("{cap_message}")).send(&send_to_terminal).await;
                } else {
                    let on = match cap_message {
                        t::CapMessage::Add { ref on, .. } => on,
                        t::CapMessage::Drop { ref on, .. } => on,
                        t::CapMessage::Has { ref on, .. } => on,
                        t::CapMessage::GetAll { ref on, .. } => on,
                        t::CapMessage::RevokeAll { ref on, .. } => on,
                        t::CapMessage::FilterCaps { ref on, .. } => on,
                    };
                    if print_full_event_loop_for_process.contains(on) {
                        t::Printout::new(3, on.clone(), format!("{cap_message}")).send(&send_to_terminal).await;
                    }
                }
                match cap_message {
                    t::CapMessage::Add { on, caps, responder } => {
                        // insert cap in process map
                        let Some(entry) = process_map.get_mut(&on) else {
                            if let Some(responder) = responder {
                                responder.send(false).ok();
                            }
                            continue;
                        };
                        let signed_caps: Vec<(t::Capability, Vec<u8>)> =
                            caps.into_iter().map(|cap| {
                                let sig = keypair.sign(&rmp_serde::to_vec(&cap).unwrap());
                                (cap, sig.as_ref().to_vec())
                            }).collect();
                        entry.capabilities.extend(signed_caps.clone());
                        // now we have to insert all caps into the reverse cap index
                        for (cap, _) in &signed_caps {
                            reverse_cap_index
                                .entry(cap.clone().issuer.process)
                                .or_insert_with(HashMap::new)
                                .entry(on.clone())
                                .or_insert_with(Vec::new)
                                .push(cap.clone());
                        }
                        if !entry.on_exit.is_none() {
                            persist_state(&send_to_loop, &process_map).await;
                        }
                        if let Some(responder) = responder {
                            responder.send(true).ok();
                        }
                    },
                    t::CapMessage::Drop { on, caps, responder } => {
                        // remove cap from process map
                        let Some(entry) = process_map.get_mut(&on) else {
                            if let Some(responder) = responder {
                                responder.send(false).ok();
                            }
                            continue;
                        };
                        for cap in &caps {
                            entry.capabilities.remove(&cap);
                        }
                        if !entry.on_exit.is_none() {
                            persist_state(&send_to_loop, &process_map).await;
                        }
                        if let Some(responder) = responder {
                            responder.send(true).ok();
                        }
                    },
                    t::CapMessage::Has { on, cap, responder } => {
                        // return boolean on responder
                        responder.send(
                            match process_map.get(&on) {
                                None => false,
                                Some(p) => p.capabilities.contains_key(&cap),
                            }
                        ).ok();
                    },
                    t::CapMessage::GetAll { on, responder } => {
                        // return all caps, signed, on responder
                        responder.send(
                            match process_map.get(&on) {
                                None => vec![],
                                Some(p) => p.capabilities.clone().into_iter().collect(),
                            }
                        ).ok();
                    },
                    t::CapMessage::RevokeAll { on, responder } => {
                        let Some(granter) = reverse_cap_index.get(&on) else {
                            if let Some(responder) = responder {
                                responder.send(true).ok();
                            }
                            continue;
                        };
                        for (grantee, caps) in granter {
                            if let Some(entry) = process_map.get_mut(&grantee) {
                                for cap in caps {
                                    entry.capabilities.remove(&cap);
                                }
                            };
                        }
                        persist_state(&send_to_loop, &process_map).await;
                        if let Some(responder) = responder {
                            responder.send(true).ok();
                        }
                    }
                    t::CapMessage::FilterCaps { on, caps, responder } => {
                        responder.send(
                            match process_map.get(&on) {
                                None => vec![],
                                Some(p) => {
                                    caps.into_iter().filter_map(|cap| {
                                        // if issuer is message source, then sign the cap
                                        if cap.issuer.process == on {
                                            let sig = keypair.sign(&rmp_serde::to_vec(&cap).unwrap());
                                            Some((cap, sig.as_ref().to_vec()))
                                        // otherwise, only attach previously saved caps
                                        // NOTE we don't need to verify the sigs!
                                        } else {
                                            p.capabilities.get(&cap).map(|sig| (cap, sig.clone()))
                                        }
                                    }).collect()
                                },
                            }
                        ).ok();
                    },
                }
            }
        }
    }
}

async fn throw_timeout(
    our_name: &str,
    senders: &HashMap<t::ProcessId, ProcessSender>,
    km: t::KernelMessage,
) {
    if let t::Message::Request(req) = &km.message {
        if req.expects_response.is_some() {
            if let Some(ProcessSender::Userspace(sender)) = senders.get(&km.source.process) {
                sender
                    .send(Err(t::WrappedSendError {
                        id: km.id,
                        source: t::Address {
                            node: our_name.to_string(),
                            process: KERNEL_PROCESS_ID.clone(),
                        },
                        error: t::SendError {
                            kind: t::SendErrorKind::Timeout,
                            target: km.target,
                            lazy_load_blob: km.lazy_load_blob,
                            message: km.message,
                        },
                    }))
                    .await
                    .ok();
            }
        }
    }
}
