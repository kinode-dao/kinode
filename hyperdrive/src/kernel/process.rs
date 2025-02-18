use crate::KERNEL_PROCESS_ID;
use lib::{types::core as t, v1::ProcessV1};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::Arc,
};
use tokio::{fs, sync::Mutex, task::JoinHandle};
use wasmtime::{
    component::{Component, Linker, ResourceTable as Table},
    Engine, Store,
};
use wasmtime_wasi::{
    pipe::MemoryOutputPipe, DirPerms, FilePerms, WasiCtx, WasiCtxBuilder, WasiView,
};

use super::RestartBackoff;

const STACK_TRACE_SIZE: usize = 5000;

pub struct ProcessContext {
    // store predecessor in order to set prompting message when popped
    pub prompting_message: Option<t::KernelMessage>,
    // can be empty if a request doesn't set context, but still needs to inherit
    pub context: Option<t::Context>,
}

pub struct ProcessState {
    /// our node's networking keypair
    pub keypair: Arc<ring::signature::Ed25519KeyPair>,
    /// information about ourself
    pub metadata: t::ProcessMetadata,
    /// pipe from which we get messages from the main event loop
    pub recv_in_process: t::ProcessMessageReceiver,
    /// pipe to send messages to ourself (received in `recv_in_process`)
    pub self_sender: t::ProcessMessageSender,
    /// pipe for sending messages to the main event loop
    pub send_to_loop: t::MessageSender,
    /// pipe for sending [`t::Printout`]s to the terminal
    pub send_to_terminal: t::PrintSender,
    /// store the current incoming message that we've gotten from receive(), if it
    /// is a request. if it is a response, the context map will be used to set this
    /// as the message it was when the outgoing request for that response was made.
    /// however, the blob stored here will **always** be the blob of the last message
    /// received from the event loop.
    /// the prompting_message won't have a blob, rather it is stored in last_blob.
    pub prompting_message: Option<t::KernelMessage>,
    pub last_message_blobbed: bool,
    pub last_blob: Option<t::LazyLoadBlob>,
    /// store the contexts and timeout task of all outstanding requests
    pub contexts: HashMap<u64, (ProcessContext, JoinHandle<()>)>,
    /// store the messages that we've gotten from event loop but haven't processed yet
    /// TODO make this an ordered map for O(1) retrieval by ID
    pub message_queue: VecDeque<Result<t::KernelMessage, t::WrappedSendError>>,
    /// pipe for getting info about capabilities
    pub caps_oracle: t::CapMessageSender,
}

pub struct ProcessWasiV1 {
    pub process: ProcessState,
    table: Table,
    wasi: WasiCtx,
}

impl WasiView for ProcessWasiV1 {
    fn table(&mut self) -> &mut Table {
        &mut self.table
    }
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }
}

async fn make_table_and_wasi(
    home_directory_path: PathBuf,
    process_state: &ProcessState,
) -> (Table, WasiCtx, MemoryOutputPipe) {
    let table = Table::new();
    let wasi_stderr = MemoryOutputPipe::new(STACK_TRACE_SIZE);

    #[cfg(unix)]
    let tmp_path = home_directory_path
        .join("vfs")
        .join(format!(
            "{}:{}",
            process_state.metadata.our.process.package(),
            process_state.metadata.our.process.publisher()
        ))
        .join("tmp");
    #[cfg(target_os = "windows")]
    let tmp_path = home_directory_path
        .join("vfs")
        .join(format!(
            "{}_{}",
            process_state.metadata.our.process.package(),
            process_state.metadata.our.process.publisher()
        ))
        .join("tmp");

    let tmp_path = tmp_path.to_str().unwrap();

    let mut wasi = WasiCtxBuilder::new();

    // TODO make guarantees about this
    if let Ok(Ok(())) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        fs::create_dir_all(&tmp_path),
    )
    .await
    {
        if let Ok(wasi) = wasi.preopened_dir(tmp_path, tmp_path, DirPerms::all(), FilePerms::all())
        {
            wasi.env("TEMP_DIR", tmp_path);
        }
    }

    (table, wasi.stderr(wasi_stderr.clone()).build(), wasi_stderr)
}

async fn make_component_v1(
    engine: Engine,
    wasm_bytes: &[u8],
    home_directory_path: PathBuf,
    process_state: ProcessState,
) -> anyhow::Result<(ProcessV1, Store<ProcessWasiV1>, MemoryOutputPipe)> {
    let component =
        Component::new(&engine, wasm_bytes.to_vec()).expect("make_component: couldn't read file");

    let mut linker = Linker::new(&engine);
    ProcessV1::add_to_linker(&mut linker, |state: &mut ProcessWasiV1| state).unwrap();
    let (table, wasi, wasi_stderr) = make_table_and_wasi(home_directory_path, &process_state).await;
    wasmtime_wasi::add_to_linker_async(&mut linker).unwrap();

    let our_process_id = process_state.metadata.our.process.clone();
    let send_to_terminal = process_state.send_to_terminal.clone();

    let mut store = Store::new(
        &engine,
        ProcessWasiV1 {
            process: process_state,
            table,
            wasi,
        },
    );

    let bindings = match ProcessV1::instantiate_async(&mut store, &component, &linker).await {
        Ok(b) => b,
        Err(e) => {
            t::Printout::new(
                0,
                t::KERNEL_PROCESS_ID.clone(),
                format!("kernel: process {our_process_id} failed to instantiate: {e:?}"),
            )
            .send(&send_to_terminal)
            .await;
            return Err(e);
        }
    };

    Ok((bindings, store, wasi_stderr))
}

/// create a specific process, and generate a task that will run it.
pub async fn make_process_loop(
    keypair: Arc<ring::signature::Ed25519KeyPair>,
    metadata: t::ProcessMetadata,
    send_to_loop: t::MessageSender,
    send_to_terminal: t::PrintSender,
    mut recv_in_process: t::ProcessMessageReceiver,
    send_to_process: t::ProcessMessageSender,
    wasm_bytes: Vec<u8>,
    caps_oracle: t::CapMessageSender,
    engine: Engine,
    home_directory_path: PathBuf,
    maybe_restart_backoff: Option<Arc<Mutex<Option<RestartBackoff>>>>,
) -> anyhow::Result<()> {
    // before process can be instantiated, need to await 'run' message from kernel
    let mut pre_boot_queue = Vec::<Result<t::KernelMessage, t::WrappedSendError>>::new();
    while let Some(message) = recv_in_process.recv().await {
        match message {
            Err(_) => {
                pre_boot_queue.push(message);
                continue;
            }
            Ok(message) => {
                if (message.source
                    == t::Address {
                        node: metadata.our.node.clone(),
                        process: KERNEL_PROCESS_ID.clone(),
                    })
                    && (message.message
                        == t::Message::Request(t::Request {
                            inherit: false,
                            expects_response: None,
                            body: b"run".to_vec(),
                            metadata: None,
                            capabilities: vec![],
                        }))
                {
                    break;
                }
                pre_boot_queue.push(Ok(message));
            }
        }
    }
    // now that we've received the run message, we can send the pre-boot queue
    for message in pre_boot_queue {
        send_to_process.send(message).await?;
    }

    let our = metadata.our.clone();
    let wit_version = metadata.wit_version.clone();

    let process_state = ProcessState {
        keypair,
        metadata,
        recv_in_process,
        self_sender: send_to_process,
        send_to_loop: send_to_loop.clone(),
        send_to_terminal: send_to_terminal.clone(),
        prompting_message: None,
        last_message_blobbed: false,
        last_blob: None,
        contexts: HashMap::new(),
        message_queue: VecDeque::new(),
        caps_oracle: caps_oracle.clone(),
    };

    let metadata = match wit_version {
        // assume missing version is oldest wit version
        None | Some(1) | _ => {
            let (bindings, mut store, wasi_stderr) =
                make_component_v1(engine, &wasm_bytes, home_directory_path, process_state).await?;

            // the process will run until it returns from init() or crashes
            match bindings.call_init(&mut store, &our.to_string()).await {
                Ok(()) => {
                    t::Printout::new(
                        1,
                        t::KERNEL_PROCESS_ID.clone(),
                        format!("process {our} returned without error"),
                    )
                    .send(&send_to_terminal)
                    .await;
                }
                Err(e) => {
                    let stderr = wasi_stderr.contents().into();
                    let stderr = String::from_utf8(stderr)?;
                    let output = if stderr != String::new() {
                        stderr
                    } else {
                        format!("{}", e.root_cause())
                    };
                    t::Printout::new(
                        0,
                        t::KERNEL_PROCESS_ID.clone(),
                        format!("\x1b[38;5;196mprocess {our} ended with error:\x1b[0m\n{output}"),
                    )
                    .send(&send_to_terminal)
                    .await;
                }
            };

            // update metadata to what was mutated by process in store
            store.data().process.metadata.to_owned()
        }
    };

    //
    // the process has completed, time to perform cleanup
    //

    t::Printout::new(
        1,
        t::KERNEL_PROCESS_ID.clone(),
        format!(
            "process {} has OnExit behavior {}",
            metadata.our.process, metadata.on_exit
        ),
    )
    .send(&send_to_terminal)
    .await;

    // fulfill the designated OnExit behavior
    match metadata.on_exit {
        t::OnExit::None => {
            t::KernelMessage::builder()
                .id(rand::random())
                .source((&our.node, KERNEL_PROCESS_ID.clone()))
                .target((&our.node, KERNEL_PROCESS_ID.clone()))
                .message(t::Message::Request(t::Request {
                    inherit: false,
                    expects_response: None,
                    body: serde_json::to_vec(&t::KernelCommand::KillProcess(
                        metadata.our.process.clone(),
                    ))
                    .unwrap(),
                    metadata: None,
                    capabilities: vec![],
                }))
                .build()
                .unwrap()
                .send(&send_to_loop)
                .await;
        }
        // if restart, tell ourselves to init the app again, with same capabilities
        t::OnExit::Restart => {
            let restart_backoff = maybe_restart_backoff.unwrap();
            let mut restart_backoff_lock = restart_backoff.lock().await;
            let now = tokio::time::Instant::now();
            let (wait_till, next_soonest_restart_time, consecutive_attempts) =
                match *restart_backoff_lock {
                    None => (None, now + tokio::time::Duration::from_secs(1), 0),
                    Some(ref rb) => {
                        if rb.next_soonest_restart_time <= now {
                            // no need to wait
                            (None, now + tokio::time::Duration::from_secs(1), 0)
                        } else {
                            // must wait
                            let base: u64 = 2;
                            (
                                Some(rb.next_soonest_restart_time.clone()),
                                rb.next_soonest_restart_time.clone()
                                    + tokio::time::Duration::from_secs(
                                        base.pow(rb.consecutive_attempts),
                                    ),
                                rb.consecutive_attempts.clone() + 1,
                            )
                        }
                    }
                };

            // get caps before killing
            let (tx, rx) = tokio::sync::oneshot::channel();
            caps_oracle
                .send(t::CapMessage::GetAll {
                    on: metadata.our.process.clone(),
                    responder: tx,
                })
                .await?;
            let initial_capabilities = rx
                .await?
                .iter()
                .map(|c| t::Capability {
                    issuer: c.0.issuer.clone(),
                    params: c.0.params.clone(),
                })
                .collect();
            // kill, **without** revoking capabilities from others!
            t::KernelMessage::builder()
                .id(rand::random())
                .source((&our.node, KERNEL_PROCESS_ID.clone()))
                .target((&our.node, KERNEL_PROCESS_ID.clone()))
                .message(t::Message::Request(t::Request {
                    inherit: false,
                    expects_response: None,
                    body: serde_json::to_vec(&t::KernelCommand::KillProcess(
                        metadata.our.process.clone(),
                    ))
                    .unwrap(),
                    metadata: Some("no-revoke".to_string()),
                    capabilities: vec![],
                }))
                .build()
                .unwrap()
                .send(&send_to_loop)
                .await;

            let reinitialize = async move {
                // then re-initialize with same capabilities
                t::KernelMessage::builder()
                    .id(rand::random())
                    .source((&our.node, KERNEL_PROCESS_ID.clone()))
                    .target((&our.node, KERNEL_PROCESS_ID.clone()))
                    .message(t::Message::Request(t::Request {
                        inherit: false,
                        expects_response: None,
                        body: serde_json::to_vec(&t::KernelCommand::InitializeProcess {
                            id: metadata.our.process.clone(),
                            wasm_bytes_handle: metadata.wasm_bytes_handle,
                            wit_version: metadata.wit_version,
                            on_exit: metadata.on_exit,
                            initial_capabilities,
                            public: metadata.public,
                        })
                        .unwrap(),
                        metadata: None,
                        capabilities: vec![],
                    }))
                    .lazy_load_blob(Some(t::LazyLoadBlob {
                        mime: None,
                        bytes: wasm_bytes,
                    }))
                    .build()
                    .unwrap()
                    .send(&send_to_loop)
                    .await;
                // then run
                t::KernelMessage::builder()
                    .id(rand::random())
                    .source((&our.node, KERNEL_PROCESS_ID.clone()))
                    .target((&our.node, KERNEL_PROCESS_ID.clone()))
                    .message(t::Message::Request(t::Request {
                        inherit: false,
                        expects_response: None,
                        body: serde_json::to_vec(&t::KernelCommand::RunProcess(
                            metadata.our.process.clone(),
                        ))
                        .unwrap(),
                        metadata: None,
                        capabilities: vec![],
                    }))
                    .build()
                    .unwrap()
                    .send(&send_to_loop)
                    .await;
            };

            let restart_handle = match wait_till {
                None => {
                    reinitialize.await;
                    None
                }
                Some(wait_till) => Some(tokio::spawn(async move {
                    tokio::time::sleep_until(wait_till).await;
                    reinitialize.await;
                })),
            };
            *restart_backoff_lock = Some(RestartBackoff {
                next_soonest_restart_time,
                consecutive_attempts,
                _restart_handle: restart_handle,
            });
        }
        // if requests, fire them
        t::OnExit::Requests(requests) => {
            for (address, mut request, blob) in requests {
                request.expects_response = None;
                t::KernelMessage::builder()
                    .id(rand::random())
                    .source(metadata.our.clone())
                    .target(address)
                    .message(t::Message::Request(request))
                    .lazy_load_blob(blob)
                    .build()
                    .unwrap()
                    .send(&send_to_loop)
                    .await;
            }
            t::KernelMessage::builder()
                .id(rand::random())
                .source((&our.node, KERNEL_PROCESS_ID.clone()))
                .target((&our.node, KERNEL_PROCESS_ID.clone()))
                .message(t::Message::Request(t::Request {
                    inherit: false,
                    expects_response: None,
                    body: serde_json::to_vec(&t::KernelCommand::KillProcess(
                        metadata.our.process.clone(),
                    ))
                    .unwrap(),
                    metadata: None,
                    capabilities: vec![],
                }))
                .build()
                .unwrap()
                .send(&send_to_loop)
                .await;
        }
    }
    Ok(())
}
