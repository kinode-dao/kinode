use crate::KERNEL_PROCESS_ID;
use anyhow::Result;
use lib::types::core as t;
pub use lib::v0::ProcessV0;
pub use lib::Process;
use ring::signature;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::fs;
use tokio::task::JoinHandle;
use wasi_common::sync::Dir;
use wasmtime::component::ResourceTable as Table;
use wasmtime::component::*;
use wasmtime::{Engine, Store};
use wasmtime_wasi::{
    pipe::MemoryOutputPipe, DirPerms, FilePerms, WasiCtx, WasiCtxBuilder, WasiView,
};

const STACK_TRACE_SIZE: usize = 5000;

pub struct ProcessContext {
    // store predecessor in order to set prompting message when popped
    pub prompting_message: Option<t::KernelMessage>,
    // can be empty if a request doesn't set context, but still needs to inherit
    pub context: Option<t::Context>,
}

pub struct ProcessState {
    /// our node's networking keypair
    pub keypair: Arc<signature::Ed25519KeyPair>,
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
    pub last_blob: Option<t::LazyLoadBlob>,
    /// store the contexts and timeout task of all outstanding requests
    pub contexts: HashMap<u64, (ProcessContext, JoinHandle<()>)>,
    /// store the messages that we've gotten from event loop but haven't processed yet
    /// TODO make this an ordered map for O(1) retrieval by ID
    pub message_queue: VecDeque<Result<t::KernelMessage, t::WrappedSendError>>,
    /// pipe for getting info about capabilities
    pub caps_oracle: t::CapMessageSender,
}

pub struct ProcessWasi {
    pub process: ProcessState,
    table: Table,
    wasi: WasiCtx,
}

impl WasiView for ProcessWasi {
    fn table(&mut self) -> &mut Table {
        &mut self.table
    }
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }
}

pub struct ProcessWasiV0 {
    pub process: ProcessState,
    table: Table,
    wasi: WasiCtx,
}

impl WasiView for ProcessWasiV0 {
    fn table(&mut self) -> &mut Table {
        &mut self.table
    }
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }
}

async fn make_component(
    engine: Engine,
    wasm_bytes: &[u8],
    home_directory_path: String,
    process_state: ProcessState,
) -> Result<(Process, Store<ProcessWasi>, MemoryOutputPipe)> {
    let component = Component::new(&engine, wasm_bytes.to_vec())
        .expect("make_process_loop: couldn't read file");

    let mut linker = Linker::new(&engine);
    Process::add_to_linker(&mut linker, |state: &mut ProcessWasi| state).unwrap();

    let table = Table::new();
    let wasi_stderr = MemoryOutputPipe::new(STACK_TRACE_SIZE);

    let our_process_id = process_state.metadata.our.process.clone();
    let send_to_terminal = process_state.send_to_terminal.clone();

    let tmp_path = format!(
        "{}/vfs/{}:{}/tmp",
        home_directory_path,
        our_process_id.package(),
        our_process_id.publisher()
    );

    let mut wasi = WasiCtxBuilder::new();

    // TODO make guarantees about this
    if let Ok(Ok(())) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        fs::create_dir_all(&tmp_path),
    )
    .await
    {
        if let Ok(wasi_tempdir) =
            Dir::open_ambient_dir(tmp_path.clone(), wasi_common::sync::ambient_authority())
        {
            wasi.preopened_dir(
                wasi_tempdir,
                DirPerms::all(),
                FilePerms::all(),
                tmp_path.clone(),
            )
            .env("TEMP_DIR", tmp_path);
        }
    }

    let wasi = wasi.stderr(wasi_stderr.clone()).build();

    wasmtime_wasi::command::add_to_linker(&mut linker).unwrap();

    let mut store = Store::new(
        &engine,
        ProcessWasi {
            process: process_state,
            table,
            wasi,
        },
    );

    let (bindings, _bindings) =
        match Process::instantiate_async(&mut store, &component, &linker).await {
            Ok(b) => b,
            Err(e) => {
                let _ = send_to_terminal
                    .send(t::Printout {
                        verbosity: 0,
                        content: format!(
                            "mk: process {:?} failed to instantiate: {:?}",
                            our_process_id, e,
                        ),
                    })
                    .await;
                return Err(e);
            }
        };

    Ok((bindings, store, wasi_stderr))
}

async fn make_component_v0(
    engine: Engine,
    wasm_bytes: &[u8],
    home_directory_path: String,
    process_state: ProcessState,
) -> Result<(ProcessV0, Store<ProcessWasiV0>, MemoryOutputPipe)> {
    let component = Component::new(&engine, wasm_bytes.to_vec())
        .expect("make_process_loop: couldn't read file");

    let mut linker = Linker::new(&engine);
    ProcessV0::add_to_linker(&mut linker, |state: &mut ProcessWasiV0| state).unwrap();

    let table = Table::new();
    let wasi_stderr = MemoryOutputPipe::new(STACK_TRACE_SIZE);

    let our_process_id = process_state.metadata.our.process.clone();
    let send_to_terminal = process_state.send_to_terminal.clone();

    let tmp_path = format!(
        "{}/vfs/{}:{}/tmp",
        home_directory_path,
        our_process_id.package(),
        our_process_id.publisher()
    );

    let mut wasi = WasiCtxBuilder::new();

    // TODO make guarantees about this
    if let Ok(Ok(())) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        fs::create_dir_all(&tmp_path),
    )
    .await
    {
        if let Ok(wasi_tempdir) =
            Dir::open_ambient_dir(tmp_path.clone(), wasi_common::sync::ambient_authority())
        {
            wasi.preopened_dir(
                wasi_tempdir,
                DirPerms::all(),
                FilePerms::all(),
                tmp_path.clone(),
            )
            .env("TEMP_DIR", tmp_path);
        }
    }

    let wasi = wasi.stderr(wasi_stderr.clone()).build();

    wasmtime_wasi::command::add_to_linker(&mut linker).unwrap();

    let mut store = Store::new(
        &engine,
        ProcessWasiV0 {
            process: process_state,
            table,
            wasi,
        },
    );

    let (bindings, _bindings) =
        match ProcessV0::instantiate_async(&mut store, &component, &linker).await {
            Ok(b) => b,
            Err(e) => {
                let _ = send_to_terminal
                    .send(t::Printout {
                        verbosity: 0,
                        content: format!(
                            "mk: process {:?} failed to instantiate: {:?}",
                            our_process_id, e,
                        ),
                    })
                    .await;
                return Err(e);
            }
        };

    Ok((bindings, store, wasi_stderr))
}

/// create a specific process, and generate a task that will run it.
pub async fn make_process_loop(
    keypair: Arc<signature::Ed25519KeyPair>,
    metadata: t::ProcessMetadata,
    send_to_loop: t::MessageSender,
    send_to_terminal: t::PrintSender,
    mut recv_in_process: t::ProcessMessageReceiver,
    send_to_process: t::ProcessMessageSender,
    wasm_bytes: Vec<u8>,
    caps_oracle: t::CapMessageSender,
    engine: Engine,
    home_directory_path: String,
) -> Result<()> {
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

    let process_state = ProcessState {
        keypair: keypair.clone(),
        metadata: metadata.clone(),
        recv_in_process,
        self_sender: send_to_process,
        send_to_loop: send_to_loop.clone(),
        send_to_terminal: send_to_terminal.clone(),
        prompting_message: None,
        last_blob: None,
        contexts: HashMap::new(),
        message_queue: VecDeque::new(),
        caps_oracle: caps_oracle.clone(),
    };

    let metadata = match metadata.wit_version {
        // assume missing version is oldest wit version
        None => {
            println!("WIT 0.7.0 OR NONE GIVEN\r");

            let (bindings, mut store, wasi_stderr) =
                make_component(engine, &wasm_bytes, home_directory_path, process_state).await?;

            // the process will run until it returns from init() or crashes
            match bindings
                .call_init(&mut store, &metadata.our.to_string())
                .await
            {
                Ok(()) => {
                    let _ = send_to_terminal
                        .send(t::Printout {
                            verbosity: 1,
                            content: format!(
                                "process {} returned without error",
                                metadata.our.process
                            ),
                        })
                        .await;
                }
                Err(_) => {
                    let stderr = wasi_stderr.contents().into();
                    let stderr = String::from_utf8(stderr)?;
                    let _ = send_to_terminal
                        .send(t::Printout {
                            verbosity: 0,
                            content: format!(
                                "\x1b[38;5;196mprocess {} ended with error:\x1b[0m\n{}",
                                metadata.our.process, stderr,
                            ),
                        })
                        .await;
                }
            };

            // update metadata to what was mutated by process in store
            store.data().process.metadata.to_owned()
        }
        // match version numbers
        // assume higher uncovered version number is latest version
        Some(0) | _ => {
            println!("WIT 0.8.0 OR HIGHER\r");

            let (bindings, mut store, wasi_stderr) =
                make_component_v0(engine, &wasm_bytes, home_directory_path, process_state).await?;

            // the process will run until it returns from init() or crashes
            match bindings
                .call_init(&mut store, &metadata.our.to_string())
                .await
            {
                Ok(()) => {
                    let _ = send_to_terminal
                        .send(t::Printout {
                            verbosity: 1,
                            content: format!(
                                "process {} returned without error",
                                metadata.our.process
                            ),
                        })
                        .await;
                }
                Err(_) => {
                    let stderr = wasi_stderr.contents().into();
                    let stderr = String::from_utf8(stderr)?;
                    let _ = send_to_terminal
                        .send(t::Printout {
                            verbosity: 0,
                            content: format!(
                                "\x1b[38;5;196mprocess {} ended with error:\x1b[0m\n{}",
                                metadata.our.process, stderr,
                            ),
                        })
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

    let our_kernel = t::Address {
        node: metadata.our.node.clone(),
        process: KERNEL_PROCESS_ID.clone(),
    };

    // get caps before killing
    let (tx, rx) = tokio::sync::oneshot::channel();
    let _ = caps_oracle
        .send(t::CapMessage::GetAll {
            on: metadata.our.process.clone(),
            responder: tx,
        })
        .await;
    let initial_capabilities = rx
        .await?
        .iter()
        .map(|c| t::Capability {
            issuer: c.0.issuer.clone(),
            params: c.0.params.clone(),
        })
        .collect();

    // fulfill the designated OnExit behavior
    match metadata.on_exit {
        t::OnExit::None => {
            send_to_loop
                .send(t::KernelMessage {
                    id: rand::random(),
                    source: our_kernel.clone(),
                    target: our_kernel.clone(),
                    rsvp: None,
                    message: t::Message::Request(t::Request {
                        inherit: false,
                        expects_response: None,
                        body: serde_json::to_vec(&t::KernelCommand::KillProcess(
                            metadata.our.process.clone(),
                        ))
                        .unwrap(),
                        metadata: None,
                        capabilities: vec![],
                    }),
                    lazy_load_blob: None,
                })
                .await?;
            let _ = send_to_terminal
                .send(t::Printout {
                    verbosity: 1,
                    content: format!("process {} had no OnExit behavior", metadata.our.process),
                })
                .await;
        }
        // if restart, tell ourselves to init the app again, with same capabilities
        t::OnExit::Restart => {
            send_to_loop
                .send(t::KernelMessage {
                    id: rand::random(),
                    source: our_kernel.clone(),
                    target: our_kernel.clone(),
                    rsvp: None,
                    message: t::Message::Request(t::Request {
                        inherit: false,
                        expects_response: None,
                        body: serde_json::to_vec(&t::KernelCommand::KillProcess(
                            metadata.our.process.clone(),
                        ))
                        .unwrap(),
                        metadata: None,
                        capabilities: vec![],
                    }),
                    lazy_load_blob: None,
                })
                .await?;
            let _ = send_to_terminal
                .send(t::Printout {
                    verbosity: 1,
                    content: format!(
                        "firing OnExit::Restart for process {}",
                        metadata.our.process
                    ),
                })
                .await;
            send_to_loop
                .send(t::KernelMessage {
                    id: rand::random(),
                    source: our_kernel.clone(),
                    target: our_kernel.clone(),
                    rsvp: None,
                    message: t::Message::Request(t::Request {
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
                    }),
                    lazy_load_blob: Some(t::LazyLoadBlob {
                        mime: None,
                        bytes: wasm_bytes,
                    }),
                })
                .await?;
            send_to_loop
                .send(t::KernelMessage {
                    id: rand::random(),
                    source: our_kernel.clone(),
                    target: our_kernel.clone(),
                    rsvp: None,
                    message: t::Message::Request(t::Request {
                        inherit: false,
                        expects_response: None,
                        body: serde_json::to_vec(&t::KernelCommand::RunProcess(
                            metadata.our.process.clone(),
                        ))
                        .unwrap(),
                        metadata: None,
                        capabilities: vec![],
                    }),
                    lazy_load_blob: None,
                })
                .await?;
        }
        // if requests, fire them
        // even in death, a process can only message processes it has capabilities for
        t::OnExit::Requests(requests) => {
            send_to_terminal
                .send(t::Printout {
                    verbosity: 1,
                    content: format!(
                        "firing OnExit::Requests for process {}",
                        metadata.our.process
                    ),
                })
                .await?;
            for (address, mut request, blob) in requests {
                request.expects_response = None;
                send_to_loop
                    .send(t::KernelMessage {
                        id: rand::random(),
                        source: metadata.our.clone(),
                        target: address,
                        rsvp: None,
                        message: t::Message::Request(request),
                        lazy_load_blob: blob,
                    })
                    .await?;
            }
            send_to_loop
                .send(t::KernelMessage {
                    id: rand::random(),
                    source: our_kernel.clone(),
                    target: our_kernel.clone(),
                    rsvp: None,
                    message: t::Message::Request(t::Request {
                        inherit: false,
                        expects_response: None,
                        body: serde_json::to_vec(&t::KernelCommand::KillProcess(
                            metadata.our.process.clone(),
                        ))
                        .unwrap(),
                        metadata: None,
                        capabilities: vec![],
                    }),
                    lazy_load_blob: None,
                })
                .await?;
        }
    }
    Ok(())
}

pub async fn print(sender: &t::PrintSender, verbosity: u8, content: String) {
    let _ = sender
        .send(t::Printout { verbosity, content })
        .await
        .expect("fatal: kernel terminal print pipe died!");
}
