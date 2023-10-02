use anyhow::Result;
use ring::signature::{self, KeyPair};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use wasmtime::component::*;
use wasmtime::{Config, Engine, Store, WasmBacktraceDetails};

use wasmtime_wasi::preview2::{DirPerms, FilePerms, Table, WasiCtx, WasiCtxBuilder, WasiView};

use crate::types as t;
//  WIT errors when `use`ing interface unless we import this and implement Host for Process below
use crate::kernel::component::uq_process::types as wit;
use crate::kernel::component::uq_process::types::Host;

mod utils;
use crate::kernel::utils::*;

bindgen!({
    path: "wit",
    world: "uq-process",
    async: true,
});
const PROCESS_CHANNEL_CAPACITY: usize = 100;

type ProcessMessageSender =
    tokio::sync::mpsc::Sender<Result<t::KernelMessage, t::WrappedSendError>>;
type ProcessMessageReceiver =
    tokio::sync::mpsc::Receiver<Result<t::KernelMessage, t::WrappedSendError>>;

struct Process {
    keypair: Arc<signature::Ed25519KeyPair>,
    metadata: t::ProcessMetadata,
    recv_in_process: ProcessMessageReceiver,
    send_to_loop: t::MessageSender,
    send_to_terminal: t::PrintSender,
    prompting_message: Option<t::KernelMessage>,
    last_payload: Option<t::Payload>,
    contexts: HashMap<u64, t::ProcessContext>,
    message_queue: VecDeque<Result<t::KernelMessage, t::WrappedSendError>>,
    caps_oracle: t::CapMessageSender,
    next_message_caps: Option<Vec<t::SignedCapability>>,
}

struct ProcessWasi {
    process: Process,
    table: Table,
    wasi: WasiCtx,
}

#[derive(Serialize, Deserialize)]
struct StartProcessMetadata {
    source: t::Address,
    process_id: Option<t::ProcessId>,
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

impl Host for ProcessWasi {}

impl WasiView for ProcessWasi {
    fn table(&self) -> &Table {
        &self.table
    }
    fn table_mut(&mut self) -> &mut Table {
        &mut self.table
    }
    fn ctx(&self) -> &WasiCtx {
        &self.wasi
    }
    fn ctx_mut(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }
}

///
/// intercept wasi random
///

#[async_trait::async_trait]
impl wasi::random::insecure::Host for ProcessWasi {
    async fn get_insecure_random_bytes(&mut self, len: u64) -> Result<Vec<u8>> {
        let mut bytes = Vec::with_capacity(len as usize);
        for _ in 0..len {
            bytes.push(rand::random());
        }
        Ok(bytes)
    }

    async fn get_insecure_random_u64(&mut self) -> Result<u64> {
        Ok(rand::random())
    }
}

#[async_trait::async_trait]
impl wasi::random::insecure_seed::Host for ProcessWasi {
    async fn insecure_seed(&mut self) -> Result<(u64, u64)> {
        Ok((rand::random(), rand::random()))
    }
}

#[async_trait::async_trait]
impl wasi::random::random::Host for ProcessWasi {
    async fn get_random_bytes(&mut self, len: u64) -> Result<Vec<u8>> {
        let mut bytes = Vec::with_capacity(len as usize);
        getrandom::getrandom(&mut bytes[..])?;
        Ok(bytes)
    }

    async fn get_random_u64(&mut self) -> Result<u64> {
        let mut bytes = Vec::with_capacity(8);
        getrandom::getrandom(&mut bytes[..])?;

        let mut number = 0u64;
        for (i, &byte) in bytes.iter().enumerate() {
            number |= (byte as u64) << (i * 8);
        }
        Ok(number)
    }
}

///
/// create the process API. this is where the functions that a process can use live.
///
#[async_trait::async_trait]
impl UqProcessImports for ProcessWasi {
    //
    // system utils:
    //f
    async fn print_to_terminal(&mut self, verbosity: u8, content: String) -> Result<()> {
        match self
            .process
            .send_to_terminal
            .send(t::Printout { verbosity, content })
            .await
        {
            Ok(()) => Ok(()),
            Err(e) => Err(anyhow::anyhow!("fatal: couldn't send to terminal: {:?}", e)),
        }
    }

    async fn get_unix_time(&mut self) -> Result<u64> {
        match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            Ok(t) => Ok(t.as_secs()),
            Err(e) => Err(e.into()),
        }
    }

    async fn get_eth_block(&mut self) -> Result<u64> {
        // TODO connect to eth RPC
        unimplemented!()
    }

    //
    // process management:
    //

    ///  todo -> move to kernel logic to enable persistence etc.
    async fn set_on_panic(&mut self, _on_panic: wit::OnPanic) -> Result<()> {
        unimplemented!();
        //     let on_panic = match on_panic {
        //         wit::OnPanic::None => t::OnPanic::None,
        //         wit::OnPanic::Restart => t::OnPanic::Restart,
        //         wit::OnPanic::Requests(reqs) => t::OnPanic::Requests(
        //             reqs.into_iter()
        //                 .map(|(addr, req, payload)| {
        //                     (
        //                         de_wit_address(addr),
        //                         de_wit_request(req),
        //                         de_wit_payload(payload),
        //                     )
        //                 })
        //                 .collect(),
        //         ),
        //     };

        //     self.process.metadata.on_panic = on_panic;
        //     Ok(())
    }

    async fn spawn(
        &mut self,
        id: wit::ProcessId,
        _bytes_uri: String,
        on_panic: wit::OnPanic,
        capabilities: wit::Capabilities,
    ) -> Result<Option<wit::ProcessId>> {
        self.process
            .send_to_loop
            .send(t::KernelMessage {
                id: 0,
                source: self.process.metadata.our.clone(),
                target: t::Address {
                    node: self.process.metadata.our.node.clone(),
                    process: t::ProcessId::Name("kernel".into()),
                },
                rsvp: Some(self.process.metadata.our.clone()),
                message: t::Message::Request(t::Request {
                    inherit: false,
                    expects_response: Some(5), // TODO evaluate
                    ipc: Some(
                        serde_json::to_string(&t::KernelCommand::StartProcess {
                            name: match id {
                                wit::ProcessId::Name(ref name) => Some(name.into()),
                                wit::ProcessId::Id(_id) => None,
                            },
                            wasm_bytes_handle: 0, // ???????
                            on_panic: de_wit_on_panic(on_panic),
                            // TODO
                            initial_capabilities: match capabilities {
                                wit::Capabilities::None => HashSet::new(),
                                wit::Capabilities::All => {
                                    let (tx, rx) = tokio::sync::oneshot::channel();
                                    let _ = self.process.caps_oracle.send(t::CapMessage::GetAll {
                                        on: self.process.metadata.our.process.clone(),
                                        responder: tx,
                                    });
                                    rx.await.unwrap()
                                }
                                wit::Capabilities::Some(caps) => caps
                                    .into_iter()
                                    .map(|cap| t::Capability {
                                        issuer: de_wit_address(cap.issuer),
                                        params: cap.params,
                                    })
                                    .collect(),
                            },
                        })
                        .unwrap(),
                    ),
                    metadata: None,
                }),
                payload: None,
                signed_capabilities: None,
            })
            .await?;
        unimplemented!()
        //Ok(Some(id))
    }

    //
    // capabilities management
    //
    async fn get_capabilities(&mut self) -> Result<Vec<wit::SignedCapability>> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = self.process.caps_oracle.send(t::CapMessage::GetAll {
            on: self.process.metadata.our.process.clone(),
            responder: tx,
        });
        Ok(rx
            .await
            .unwrap()
            .into_iter()
            .map(|cap| wit::SignedCapability {
                issuer: en_wit_address(cap.issuer.clone()),
                params: cap.params.clone(),
                signature: self
                    .process
                    .keypair
                    .sign(&bincode::serialize(&cap).unwrap())
                    .as_ref()
                    .to_vec(),
            })
            .collect())
    }

    async fn get_capability(
        &mut self,
        issuer: wit::Address,
        params: String,
    ) -> Result<Option<wit::SignedCapability>> {
        let cap = t::Capability {
            issuer: de_wit_address(issuer),
            params,
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = self.process.caps_oracle.send(t::CapMessage::Has {
            on: self.process.metadata.our.process.clone(),
            cap: cap.clone(),
            responder: tx,
        });
        if rx.await.unwrap() {
            let sig = self
                .process
                .keypair
                .sign(&bincode::serialize(&cap).unwrap());
            return Ok(Some(wit::SignedCapability {
                issuer: en_wit_address(cap.issuer.clone()),
                params: cap.params.clone(),
                signature: sig.as_ref().to_vec(),
            }));
        } else {
            return Ok(None);
        }
    }

    async fn attach_capability(&mut self, capability: wit::SignedCapability) -> Result<()> {
        match self.process.next_message_caps {
            None => {
                self.process.next_message_caps = Some(vec![de_wit_signed_capability(capability)]);
                Ok(())
            }
            Some(ref mut v) => {
                v.push(de_wit_signed_capability(capability));
                Ok(())
            }
        }
    }

    async fn save_capabilities(&mut self, capabilities: Vec<wit::SignedCapability>) -> Result<()> {
        let pk = signature::UnparsedPublicKey::new(
            &signature::ED25519,
            self.process.keypair.public_key(),
        );
        for signed_cap in capabilities {
            // validate our signature!
            let cap = t::Capability {
                issuer: de_wit_address(signed_cap.issuer),
                params: signed_cap.params,
            };
            pk.verify(&bincode::serialize(&cap).unwrap(), &signed_cap.signature)?;

            let _ = self.process.caps_oracle.send(t::CapMessage::Add {
                on: self.process.metadata.our.process.clone(),
                cap: cap.clone(),
            });
        }
        Ok(())
    }

    async fn has_capability(&mut self, params: String) -> Result<bool> {
        if self.process.prompting_message.is_none() {
            return Err(anyhow::anyhow!(
                "kernel: has_capability() called with no prompting_message"
            ));
        }
        let prompt = self.process.prompting_message.as_ref().unwrap();
        if prompt.source.node == self.process.metadata.our.node {
            // if local, need to ask them
            let cap = t::Capability {
                issuer: prompt.source.clone(),
                params,
            };
            let (tx, rx) = tokio::sync::oneshot::channel();
            let _ = self.process.caps_oracle.send(t::CapMessage::Has {
                on: self.process.metadata.our.process.clone(),
                cap: cap.clone(),
                responder: tx,
            });
            Ok(rx.await.unwrap_or(false))
        } else {
            // if remote, just check prompting_message
            if prompt.signed_capabilities.is_none() {
                return Ok(false);
            }
            for cap in prompt.signed_capabilities.as_ref().unwrap() {
                if cap.issuer == self.process.metadata.our && cap.params == params {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
    }

    //
    // message I/O:
    //

    /// from a process: receive the next incoming message. will wait async until a message is received.
    /// the incoming message can be a Request or a Response, or an Error of the Network variety.
    async fn receive(
        &mut self,
    ) -> Result<Result<(wit::Address, wit::Message), (wit::SendError, Option<wit::Context>)>> {
        Ok(self.process.get_next_message_for_process().await)
    }

    /// from a process: grab the payload part of the current prompting message.
    /// if the prompting message did not have a payload, will return None.
    /// will also return None if there is no prompting message.
    async fn get_payload(&mut self) -> Result<Option<wit::Payload>> {
        Ok(en_wit_payload(self.process.last_payload.clone()))
    }

    async fn send_request(
        &mut self,
        target: wit::Address,
        request: wit::Request,
        context: Option<wit::Context>,
        payload: Option<wit::Payload>,
    ) -> Result<()> {
        let id = self
            .process
            .handle_request(target, request, context, payload)
            .await;
        match id {
            Ok(_id) => Ok(()),
            Err(e) => Err(e),
        }
    }

    async fn send_requests(
        &mut self,
        requests: Vec<(
            wit::Address,
            wit::Request,
            Option<wit::Context>,
            Option<wit::Payload>,
        )>,
    ) -> Result<()> {
        for request in requests {
            let id = self
                .process
                .handle_request(request.0, request.1, request.2, request.3)
                .await;
            match id {
                Ok(_id) => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    async fn send_response(
        &mut self,
        response: wit::Response,
        payload: Option<wit::Payload>,
    ) -> Result<()> {
        self.process.send_response(response, payload).await;
        Ok(())
    }

    async fn send_and_await_response(
        &mut self,
        target: wit::Address,
        request: wit::Request,
        payload: Option<wit::Payload>,
    ) -> Result<Result<(wit::Address, wit::Message), wit::SendError>> {
        if request.expects_response.is_none() {
            return Err(anyhow::anyhow!("kernel: got invalid send_and_await_response() Request from {:?}: must expect response", self.process.metadata.our.process));
        }
        let id = self
            .process
            .handle_request(target, request, None, payload)
            .await;
        match id {
            Ok(id) => match self.process.get_specific_message_for_process(id).await {
                Ok((address, wit::Message::Response(response))) => {
                    Ok(Ok((address, wit::Message::Response(response))))
                }
                Ok((_address, wit::Message::Request(_))) => Err(anyhow::anyhow!(
                    "fatal: received Request instead of Response"
                )),
                Err((net_err, _context)) => Ok(Err(net_err)),
            },
            Err(e) => Err(e),
        }
    }
}

impl Process {
    /// save a context for a given request.
    async fn save_context(&mut self, request_id: u64, context: Option<t::Context>) {
        self.contexts.insert(
            request_id,
            t::ProcessContext {
                prompting_message: if self.prompting_message.is_some() {
                    self.prompting_message.clone()
                } else {
                    None
                },
                context,
            },
        );
    }

    /// Ingest latest message directed to this process, and mark it as the prompting message.
    /// If there is no message in the queue, wait async until one is received.
    /// The message will only be saved as the prompting-message if it's a Request.
    async fn get_next_message_for_process(
        &mut self,
    ) -> Result<(wit::Address, wit::Message), (wit::SendError, Option<wit::Context>)> {
        let res = match self.message_queue.pop_front() {
            Some(message_from_queue) => message_from_queue,
            None => self.recv_in_process.recv().await.unwrap(),
        };
        self.kernel_message_to_process_receive(res)
    }

    /// instead of ingesting latest, wait for a specific ID and queue all others
    async fn get_specific_message_for_process(
        &mut self,
        awaited_message_id: u64,
    ) -> Result<(wit::Address, wit::Message), (wit::SendError, Option<wit::Context>)> {
        // first, check if the awaited message is already in the queue and handle if so
        for (i, message) in self.message_queue.iter().enumerate() {
            match message {
                Ok(ref km) if km.id == awaited_message_id => {
                    let km = self.message_queue.remove(i).unwrap();
                    return self.kernel_message_to_process_receive(km.clone());
                }
                _ => continue,
            }
        }
        // next, wait for the awaited message to arrive
        loop {
            let res = self.recv_in_process.recv().await.unwrap();
            match res {
                Ok(ref km) if km.id == awaited_message_id => {
                    return self.kernel_message_to_process_receive(Ok(km.clone()))
                }
                Ok(km) => self.message_queue.push_back(Ok(km)),
                Err(e) if e.id == awaited_message_id => {
                    return self.kernel_message_to_process_receive(Err(e))
                }
                Err(e) => self.message_queue.push_back(Err(e)),
            }
        }
    }

    /// convert a message from the main event loop into a result for the process to receive
    /// if the message is a response or error, get context if we have one
    fn kernel_message_to_process_receive(
        &mut self,
        res: Result<t::KernelMessage, t::WrappedSendError>,
    ) -> Result<(wit::Address, wit::Message), (wit::SendError, Option<wit::Context>)> {
        let (context, km) = match res {
            Ok(km) => match self.contexts.remove(&km.id) {
                None => {
                    self.last_payload = km.payload.clone();
                    self.prompting_message = Some(km.clone());
                    (None, km)
                }
                Some(context) => {
                    self.last_payload = km.payload.clone();
                    self.prompting_message = match context.prompting_message {
                        None => Some(km.clone()),
                        Some(prompting_message) => Some(prompting_message),
                    };
                    (context.context, km)
                }
            },
            Err(e) => match self.contexts.remove(&e.id) {
                None => return Err((en_wit_send_error(e.error), None)),
                Some(context) => {
                    self.prompting_message = context.prompting_message;
                    return Err((en_wit_send_error(e.error), context.context));
                }
            },
        };

        // note: the context in the KernelMessage is not actually the one we want:
        // (in fact it should be None, possibly always)
        // we need to get *our* context for this message id
        Ok((
            en_wit_address(km.source),
            match km.message {
                t::Message::Request(request) => wit::Message::Request(en_wit_request(request)),
                t::Message::Response((response, _context)) => {
                    wit::Message::Response((en_wit_response(response), context))
                }
            },
        ))
    }

    /// Given the current process state, return the id and target that
    /// a response it emits should have. This takes into
    /// account the `rsvp` of the prompting message, if any.
    async fn make_response_id_target(&self) -> Option<(u64, t::Address)> {
        let Some(ref prompting_message) = self.prompting_message else {
            println!("need non-None prompting_message to handle Response");
            return None;
        };
        match &prompting_message.rsvp {
            None => {
                let _ = self
                    .send_to_terminal
                    .send(t::Printout {
                        verbosity: 1,
                        content: "kernel: prompting_message has no rsvp".into(),
                    })
                    .await;
                return None;
            }
            Some(address) => Some((prompting_message.id, address.clone())),
        }
    }

    /// takes Request generated by a process and sends it to the main event loop.
    /// will only fail if process does not have capability to send to target.
    async fn handle_request(
        &mut self,
        target: wit::Address,
        request: wit::Request,
        new_context: Option<wit::Context>,
        payload: Option<wit::Payload>,
    ) -> Result<u64> {
        let source = self.metadata.our.clone();
        // if request chooses to inherit context, match id to prompting_message
        // otherwise, id is generated randomly
        let request_id: u64 = if request.inherit
            && request.expects_response.is_none()
            && self.prompting_message.is_some()
        {
            self.prompting_message.as_ref().unwrap().id
        } else {
            loop {
                let id = rand::random();
                if !self.contexts.contains_key(&id) {
                    break id;
                }
            }
        };

        // rsvp is set if there was a Request expecting Response
        // followed by inheriting Request(s) not expecting Response;
        // this is done such that the ultimate request handler knows that,
        // in fact, a Response *is* expected.
        // could also be None if entire chain of Requests are
        // not expecting Response
        let kernel_message = t::KernelMessage {
            id: request_id,
            source: source.clone(),
            target: de_wit_address(target),
            rsvp: match (
                request.inherit,
                request.expects_response,
                &self.prompting_message,
            ) {
                // this request expects response, so receives any response
                (_, Some(_), _) => Some(source),
                // this request inherits, so response will be routed to prompting message
                (true, None, Some(ref prompt)) => prompt.rsvp.clone(),
                // this request doesn't inherit, and doesn't itself want a response
                (false, None, _) => None,
                // no rsvp because neither prompting message nor this request wants a response
                (_, None, None) => None,
            },
            message: t::Message::Request(de_wit_request(request.clone())),
            payload: de_wit_payload(payload),
            signed_capabilities: None,
        };

        // modify the process' context map as needed.
        // if there is a prompting message, we need to store the ultimate
        // even if there is no new context string.
        self.save_context(kernel_message.id, new_context).await;

        self.send_to_loop
            .send(kernel_message)
            .await
            .expect("fatal: kernel couldn't send request");

        Ok(request_id)
    }

    /// takes Response generated by a process and sends it to the main event loop.
    async fn send_response(&mut self, response: wit::Response, payload: Option<wit::Payload>) {
        let (id, target) = match self.make_response_id_target().await {
            Some(r) => r,
            None => {
                self.send_to_terminal
                    .send(t::Printout {
                        verbosity: 1,
                        content: format!("kernel: dropping Response {:?}", response),
                    })
                    .await
                    .unwrap();
                return;
            }
        };

        self.send_to_loop
            .send(t::KernelMessage {
                id,
                source: self.metadata.our.clone(),
                target,
                rsvp: None,
                message: t::Message::Response((
                    de_wit_response(response),
                    // the context will be set by the process receiving this Response.
                    None,
                )),
                payload: de_wit_payload(payload),
                signed_capabilities: None,
            })
            .await
            .unwrap();
    }
}

/// persist process_map state for next bootup
async fn persist_state(
    our_name: &String,
    send_to_loop: &t::MessageSender,
    process_map: &t::ProcessMap,
) -> Result<()> {
    let bytes = bincode::serialize(process_map)?;

    send_to_loop
        .send(t::KernelMessage {
            id: 0,
            source: t::Address {
                node: our_name.clone(),
                process: t::ProcessId::Name("kernel".into()),
            },
            target: t::Address {
                node: our_name.clone(),
                process: t::ProcessId::Name("filesystem".into()),
            },
            rsvp: None,
            message: t::Message::Request(t::Request {
                inherit: true,
                expects_response: Some(5), // TODO evaluate
                ipc: Some(serde_json::to_string(&t::FsAction::SetState).unwrap()),
                metadata: None,
            }),
            payload: Some(t::Payload { mime: None, bytes }),
            signed_capabilities: None,
        })
        .await?;
    Ok(())
}

/// create a specific process, and generate a task that will run it.
async fn make_process_loop(
    keypair: Arc<signature::Ed25519KeyPair>,
    home_directory_path: String,
    metadata: t::ProcessMetadata,
    send_to_loop: t::MessageSender,
    send_to_terminal: t::PrintSender,
    recv_in_process: ProcessMessageReceiver,
    wasm_bytes: &Vec<u8>,
    initial_capabilities: HashSet<t::Capability>,
    caps_oracle: t::CapMessageSender,
    engine: &Engine,
) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
    let our = metadata.our.clone();
    let wasm_bytes_handle = metadata.wasm_bytes_handle.clone();
    let on_panic = metadata.on_panic.clone();

    // let dir = std::env::current_dir().unwrap();
    let dir = cap_std::fs::Dir::open_ambient_dir(home_directory_path, cap_std::ambient_authority())
        .unwrap();

    let component =
        Component::new(&engine, wasm_bytes).expect("make_process_loop: couldn't read file");

    let mut linker = Linker::new(&engine);
    UqProcess::add_to_linker(&mut linker, |state: &mut ProcessWasi| state).unwrap();

    let mut table = Table::new();
    let wasi = WasiCtxBuilder::new()
        .push_preopened_dir(dir, DirPerms::all(), FilePerms::all(), &"")
        .build(&mut table)
        .unwrap();

    // wasmtime_wasi::preview2::command::add_to_linker(&mut linker).unwrap();
    wasmtime_wasi::preview2::bindings::clocks::wall_clock::add_to_linker(&mut linker, |t| t)
        .unwrap();
    wasmtime_wasi::preview2::bindings::clocks::monotonic_clock::add_to_linker(&mut linker, |t| t)
        .unwrap();
    wasmtime_wasi::preview2::bindings::clocks::timezone::add_to_linker(&mut linker, |t| t).unwrap();
    wasmtime_wasi::preview2::bindings::filesystem::filesystem::add_to_linker(&mut linker, |t| t)
        .unwrap();
    wasmtime_wasi::preview2::bindings::poll::poll::add_to_linker(&mut linker, |t| t).unwrap();
    wasmtime_wasi::preview2::bindings::io::streams::add_to_linker(&mut linker, |t| t).unwrap();
    // wasmtime_wasi::preview2::bindings::random::random::add_to_linker(&mut linker, |t| t).unwrap();
    wasmtime_wasi::preview2::bindings::cli_base::exit::add_to_linker(&mut linker, |t| t).unwrap();
    wasmtime_wasi::preview2::bindings::cli_base::environment::add_to_linker(&mut linker, |t| t)
        .unwrap();
    wasmtime_wasi::preview2::bindings::cli_base::preopens::add_to_linker(&mut linker, |t| t)
        .unwrap();
    wasmtime_wasi::preview2::bindings::cli_base::stdin::add_to_linker(&mut linker, |t| t).unwrap();
    wasmtime_wasi::preview2::bindings::cli_base::stdout::add_to_linker(&mut linker, |t| t).unwrap();
    wasmtime_wasi::preview2::bindings::cli_base::stderr::add_to_linker(&mut linker, |t| t).unwrap();
    let mut store = Store::new(
        engine,
        ProcessWasi {
            process: Process {
                keypair: keypair.clone(),
                metadata,
                recv_in_process,
                send_to_loop: send_to_loop.clone(),
                send_to_terminal: send_to_terminal.clone(),
                prompting_message: None,
                last_payload: None,
                contexts: HashMap::new(),
                message_queue: VecDeque::new(),
                caps_oracle,
                next_message_caps: None,
            },
            table,
            wasi,
        },
    );

    Box::pin(async move {
        let (bindings, _bindings) =
            match UqProcess::instantiate_async(&mut store, &component, &linker).await {
                Ok(b) => b,
                Err(e) => {
                    let _ = send_to_terminal
                        .send(t::Printout {
                            verbosity: 0,
                            content: format!(
                                "mk: process {:?} failed to instantiate: {:?}",
                                our.process, e,
                            ),
                        })
                        .await;
                    return Err(e);
                }
            };

        // the process will run until it returns from init()
        let is_error = match bindings
            .call_init(&mut store, &en_wit_address(our.clone()))
            .await
        {
            Ok(()) => false,
            Err(e) => {
                let _ = send_to_terminal
                    .send(t::Printout {
                        verbosity: 0,
                        content: format!("mk: process {:?} ended with error:", our.process,),
                    })
                    .await;
                for line in format!("{:?}", e).lines() {
                    let _ = send_to_terminal
                        .send(t::Printout {
                            verbosity: 0,
                            content: line.into(),
                        })
                        .await;
                }
                true
            }
        };

        // the process has completed, perform cleanup
        let our_kernel = t::Address {
            node: our.node.clone(),
            process: t::ProcessId::Name("kernel".into()),
        };

        if is_error {
            // fulfill the designated OnPanic behavior
            match on_panic {
                t::OnPanic::None => {}
                // if restart, tell ourselves to init the app again, with same capabilities
                t::OnPanic::Restart => {
                    send_to_loop
                        .send(t::KernelMessage {
                            id: rand::random(),
                            source: our_kernel.clone(),
                            target: our_kernel.clone(),
                            rsvp: None,
                            message: t::Message::Request(t::Request {
                                inherit: false,
                                expects_response: None,
                                ipc: Some(
                                    serde_json::to_string(&t::KernelCommand::StartProcess {
                                        name: match &our.process {
                                            t::ProcessId::Name(name) => Some(name.into()),
                                            t::ProcessId::Id(_) => None,
                                        },
                                        wasm_bytes_handle,
                                        on_panic,
                                        initial_capabilities,
                                    })
                                    .unwrap(),
                                ),
                                metadata: None,
                            }),
                            payload: None,
                            signed_capabilities: None,
                        })
                        .await
                        .unwrap();
                }
                // if requests, fire them
                t::OnPanic::Requests(requests) => {
                    for (address, mut request, payload) in requests {
                        request.expects_response = None;
                        send_to_loop
                            .send(t::KernelMessage {
                                id: rand::random(),
                                source: our.clone(),
                                target: address,
                                rsvp: None,
                                message: t::Message::Request(request),
                                payload,
                                signed_capabilities: None,
                            })
                            .await
                            .unwrap();
                    }
                }
            }
        }

        // always send message to tell main kernel loop to remove handler
        send_to_loop
            .send(t::KernelMessage {
                id: rand::random(),
                source: our_kernel.clone(),
                target: our_kernel.clone(),
                rsvp: None,
                message: t::Message::Request(t::Request {
                    inherit: false,
                    expects_response: None,
                    ipc: Some(
                        serde_json::to_string(&t::KernelCommand::KillProcess(our.process.clone()))
                            .unwrap(),
                    ),
                    metadata: None,
                }),
                payload: None,
                signed_capabilities: None,
            })
            .await
            .unwrap();
        Ok(())
    })
}

/// handle messages sent directly to kernel. source is always our own node.
async fn handle_kernel_request(
    our_name: String,
    km: t::KernelMessage,
    send_to_loop: t::MessageSender,
    send_to_terminal: t::PrintSender,
    senders: &mut Senders,
    process_handles: &mut ProcessHandles,
    process_map: &mut t::ProcessMap,
    caps_oracle: t::CapMessageSender,
) {
    let t::Message::Request(request) = km.message else {
        return;
    };
    let command: t::KernelCommand = match serde_json::from_str(&request.ipc.unwrap_or_default()) {
        Err(e) => {
            send_to_terminal
                .send(t::Printout {
                    verbosity: 1,
                    content: format!("kernel: couldn't parse command: {:?}", e),
                })
                .await
                .unwrap();
            return;
        }
        Ok(c) => c,
    };
    match command {
        t::KernelCommand::Shutdown => {
            for handle in process_handles.values() {
                handle.abort();
            }
        }
        //
        // initialize a new process. this is the only way to create a new process.
        // this sends a read request to filesystem, when response is received,
        // the process is spawned
        //
        t::KernelCommand::StartProcess {
            name,
            wasm_bytes_handle,
            on_panic,
            initial_capabilities,
        } => send_to_loop
            .send(t::KernelMessage {
                id: km.id,
                source: t::Address {
                    node: our_name.clone(),
                    process: t::ProcessId::Name("kernel".into()),
                },
                target: t::Address {
                    node: our_name.clone(),
                    process: t::ProcessId::Name("filesystem".into()),
                },
                rsvp: None,
                message: t::Message::Request(t::Request {
                    inherit: true,
                    expects_response: Some(5), // TODO evaluate
                    ipc: Some(
                        serde_json::to_string(&t::FsAction::Read(wasm_bytes_handle)).unwrap(),
                    ),
                    // TODO find a better way if possible: keeping process metadata
                    // in request/response roundtrip because kernel itself doesn't
                    // have contexts to rely on..
                    // filesystem has to give this back to us.
                    metadata: Some(
                        serde_json::to_string(&StartProcessMetadata {
                            source: km.source,
                            process_id: name.map(|n| t::ProcessId::Name(n)),
                            persisted: t::PersistedProcess {
                                wasm_bytes_handle,
                                on_panic,
                                capabilities: initial_capabilities,
                            },
                            reboot: false,
                        })
                        .unwrap(),
                    ),
                }),
                payload: None,
                signed_capabilities: None,
            })
            .await
            .unwrap(),
        //  reboot from persisted process.
        t::KernelCommand::RebootProcess {
            process_id,
            persisted,
        } => send_to_loop
            .send(t::KernelMessage {
                id: km.id,
                source: t::Address {
                    node: our_name.clone(),
                    process: t::ProcessId::Name("kernel".into()),
                },
                target: t::Address {
                    node: our_name.clone(),
                    process: t::ProcessId::Name("filesystem".into()),
                },
                rsvp: None,
                message: t::Message::Request(t::Request {
                    inherit: true,
                    expects_response: Some(5), // TODO evaluate
                    ipc: Some(
                        serde_json::to_string(&t::FsAction::Read(persisted.wasm_bytes_handle))
                            .unwrap(),
                    ),
                    metadata: Some(
                        serde_json::to_string(&StartProcessMetadata {
                            source: km.source,
                            process_id: Some(process_id),
                            persisted,
                            reboot: true,
                        })
                        .unwrap(),
                    ),
                }),
                payload: None,
                signed_capabilities: None,
            })
            .await
            .unwrap(),
        t::KernelCommand::KillProcess(process_id) => {
            // brutal and savage killing: aborting the task.
            // do not do this to a process if you don't want to risk
            // dropped messages / un-replied-to-requests
            let _ = senders.remove(&process_id);
            let process_handle = match process_handles.remove(&process_id) {
                Some(ph) => ph,
                None => {
                    send_to_terminal
                        .send(t::Printout {
                            verbosity: 0,
                            content: format!("kernel: no such process {:?} to Stop", process_id),
                        })
                        .await
                        .unwrap();
                    return;
                }
            };
            process_handle.abort();

            if request.expects_response.is_none() {
                return;
            }

            process_map.remove(&process_id);
            let _ = persist_state(&our_name, &send_to_loop, &process_map).await;

            send_to_loop
                .send(t::KernelMessage {
                    id: km.id,
                    source: t::Address {
                        node: our_name.clone(),
                        process: t::ProcessId::Name("kernel".into()),
                    },
                    target: km.source,
                    rsvp: None,
                    message: t::Message::Response((
                        t::Response {
                            ipc: Some(
                                serde_json::to_string(&t::KernelResponse::KilledProcess(
                                    process_id,
                                ))
                                .unwrap(),
                            ),
                            metadata: None,
                        },
                        None,
                    )),
                    payload: None,
                    signed_capabilities: None,
                })
                .await
                .unwrap();
        }
        t::KernelCommand::GrantCapability { to_process, params } => {
            caps_oracle
                .send(t::CapMessage::Add {
                    on: to_process,
                    cap: t::Capability {
                        issuer: km.source.clone(),
                        params,
                    },
                })
                .unwrap();
        }
    }
}

/// currently, the kernel only receives 2 classes of responses, file-read and set-state
/// responses from the filesystem module. it uses these to get wasm bytes of a process and
/// start that process.
async fn handle_kernel_response(
    our_name: String,
    keypair: Arc<signature::Ed25519KeyPair>,
    home_directory_path: String,
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
                verbosity: 1,
                content: "kernel: got weird Response".into(),
            })
            .await;
        return;
    };

    // ignore responses that aren't filesystem responses
    if km.source.process != t::ProcessId::Name("filesystem".into()) {
        return;
    }

    let Some(ref metadata) = response.metadata else {
        //  set-state response currently return here
        //  we might want to match on metadata type from both, and only update
        //  process map upon receiving confirmation that it's been persisted
        return;
    };

    let meta: StartProcessMetadata = match serde_json::from_str(&metadata) {
        Err(_) => {
            let _ = send_to_terminal
                .send(t::Printout {
                    verbosity: 1,
                    content: "kernel: got weird metadata from filesystem".into(),
                })
                .await;
            return;
        }
        Ok(m) => m,
    };

    let Some(ref payload) = km.payload else {
        send_to_terminal
            .send(t::Printout {
                verbosity: 0,
                content: "kernel: process startup requires bytes".into(),
            })
            .await
            .unwrap();
        return;
    };

    start_process(
        our_name,
        keypair.clone(),
        home_directory_path,
        km.id,
        &payload.bytes,
        send_to_loop,
        send_to_terminal,
        senders,
        process_handles,
        process_map,
        engine,
        caps_oracle,
        meta,
    )
    .await;
}

async fn start_process(
    our_name: String,
    keypair: Arc<signature::Ed25519KeyPair>,
    home_directory_path: String,
    km_id: u64,
    km_payload_bytes: &Vec<u8>,
    send_to_loop: t::MessageSender,
    send_to_terminal: t::PrintSender,
    senders: &mut Senders,
    process_handles: &mut ProcessHandles,
    process_map: &mut t::ProcessMap,
    engine: &Engine,
    caps_oracle: t::CapMessageSender,
    process_metadata: StartProcessMetadata,
) {
    let (send_to_process, recv_in_process) =
        mpsc::channel::<Result<t::KernelMessage, t::WrappedSendError>>(PROCESS_CHANNEL_CAPACITY);
    let process_id = match process_metadata.process_id {
        Some(t::ProcessId::Name(name)) => {
            if senders.contains_key(&t::ProcessId::Name(name.clone())) {
                // TODO: make a Response to indicate failure?
                send_to_terminal
                    .send(t::Printout {
                        verbosity: 0,
                        content: format!("kernel: process named {} already exists", name),
                    })
                    .await
                    .unwrap();
                return;
            } else {
                t::ProcessId::Name(name)
            }
        }
        Some(t::ProcessId::Id(id)) => {
            if senders.contains_key(&t::ProcessId::Id(id)) {
                // TODO: make a Response to indicate failure?
                send_to_terminal
                    .send(t::Printout {
                        verbosity: 0,
                        content: format!("kernel: process with id {} already exists", id),
                    })
                    .await
                    .unwrap();
                return;
            } else {
                t::ProcessId::Id(id)
            }
        }
        // first 2 Some cases were for reboot or start with defined name, this is for start without name
        None => {
            loop {
                // lol
                let id: u64 = rand::random();
                if senders.contains_key(&t::ProcessId::Id(id)) {
                    continue;
                } else {
                    break t::ProcessId::Id(id);
                }
            }
        }
    };

    senders.insert(
        process_id.clone(),
        ProcessSender::Userspace(send_to_process),
    );
    let metadata = t::ProcessMetadata {
        our: t::Address {
            node: our_name.clone(),
            process: process_id.clone(),
        },
        wasm_bytes_handle: process_metadata.persisted.wasm_bytes_handle.clone(),
        on_panic: process_metadata.persisted.on_panic.clone(),
    };
    process_handles.insert(
        process_id.clone(),
        tokio::spawn(
            make_process_loop(
                keypair.clone(),
                home_directory_path,
                metadata.clone(),
                send_to_loop.clone(),
                send_to_terminal.clone(),
                recv_in_process,
                &km_payload_bytes,
                process_metadata.persisted.capabilities.clone(),
                caps_oracle,
                engine,
            )
            .await,
        ),
    );

    process_map.insert(
        process_id,
        t::PersistedProcess {
            wasm_bytes_handle: process_metadata.persisted.wasm_bytes_handle,
            on_panic: process_metadata.persisted.on_panic,
            capabilities: process_metadata.persisted.capabilities,
        },
    );

    if !process_metadata.reboot {
        // if new, persist
        let _ = persist_state(&our_name, &send_to_loop, &process_map).await;
    }

    send_to_loop
        .send(t::KernelMessage {
            id: km_id,
            source: t::Address {
                node: our_name.clone(),
                process: t::ProcessId::Name("kernel".into()),
            },
            target: process_metadata.source,
            rsvp: None,
            message: t::Message::Response((
                t::Response {
                    ipc: Some(
                        serde_json::to_string(&t::KernelResponse::StartedProcess(metadata))
                            .unwrap(),
                    ),
                    metadata: None,
                },
                None,
            )),
            payload: None,
            signed_capabilities: None,
        })
        .await
        .unwrap();
}

/// process event loop. allows WASM processes to send messages to various runtime modules.
/// if this dies, it's over
async fn make_event_loop(
    our_name: String,
    keypair: Arc<signature::Ed25519KeyPair>,
    home_directory_path: String,
    mut process_map: t::ProcessMap,
    caps_oracle_sender: t::CapMessageSender,
    mut caps_oracle_receiver: t::CapMessageReceiver,
    mut recv_in_loop: t::MessageReceiver,
    mut network_error_recv: t::NetworkErrorReceiver,
    mut recv_debug_in_loop: t::DebugReceiver,
    send_to_loop: t::MessageSender,
    send_to_net: t::MessageSender,
    send_to_fs: t::MessageSender,
    send_to_http_server: t::MessageSender,
    send_to_http_client: t::MessageSender,
    send_to_eth_rpc: t::MessageSender,
    send_to_vfs: t::MessageSender,
    send_to_encryptor: t::MessageSender,
    send_to_terminal: t::PrintSender,
    engine: Engine,
) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
    Box::pin(async move {
        let mut senders: Senders = HashMap::new();
        senders.insert(
            t::ProcessId::Name("eth_rpc".into()),
            ProcessSender::Runtime(send_to_eth_rpc),
        );
        senders.insert(
            t::ProcessId::Name("filesystem".into()),
            ProcessSender::Runtime(send_to_fs),
        );
        senders.insert(
            t::ProcessId::Name("http_server".into()),
            ProcessSender::Runtime(send_to_http_server),
        );
        senders.insert(
            t::ProcessId::Name("http_client".into()),
            ProcessSender::Runtime(send_to_http_client),
        );
        senders.insert(
            t::ProcessId::Name("encryptor".into()),
            ProcessSender::Runtime(send_to_encryptor),
        );
        senders.insert(
            t::ProcessId::Name("net".into()),
            ProcessSender::Runtime(send_to_net.clone()),
        );
        senders.insert(
            t::ProcessId::Name("vfs".into()),
            ProcessSender::Runtime(send_to_vfs.clone()),
        );

        // each running process is stored in this map
        let mut process_handles: ProcessHandles = HashMap::new();

        let mut is_debug: bool = false;

        // will boot every wasm module inside /modules
        // currently have an exclude list to avoid broken modules
        // modules started manually by users will bootup automatically
        // TODO remove once the modules compile!

        let exclude_list: Vec<t::ProcessId> = vec![
            t::ProcessId::Name("explorer".into()),
            t::ProcessId::Name("file_transfer".into()),
            t::ProcessId::Name("file_transfer_one_off".into()),
        ];

        for (process_id, persisted) in &process_map {
            if !exclude_list.contains(&process_id) && persisted.on_panic.is_restart() {
                send_to_loop
                    .send(t::KernelMessage {
                        id: rand::random(),
                        source: t::Address {
                            node: our_name.clone(),
                            process: t::ProcessId::Name("kernel".into()),
                        },
                        target: t::Address {
                            node: our_name.clone(),
                            process: t::ProcessId::Name("kernel".into()),
                        },
                        rsvp: None,
                        message: t::Message::Request(t::Request {
                            inherit: false,
                            expects_response: None,
                            ipc: Some(
                                serde_json::to_string(&t::KernelCommand::RebootProcess {
                                    process_id: process_id.clone(),
                                    persisted: persisted.clone(),
                                })
                                .unwrap(),
                            ),
                            metadata: None,
                        }),
                        payload: None,
                        signed_capabilities: None,
                    })
                    .await
                    .unwrap();
            }
            if let t::OnPanic::Requests(requests) = &persisted.on_panic {
                // if a persisted process had on-death-requests, we should perform them now
                for (address, request, payload) in requests {
                    // the process that made the request is dead, so never expects response
                    let mut request = request.clone();
                    request.expects_response = None;
                    send_to_loop
                        .send(t::KernelMessage {
                            id: rand::random(),
                            source: t::Address {
                                node: our_name.clone(),
                                process: process_id.clone(),
                            },
                            target: address.clone(),
                            rsvp: None,
                            message: t::Message::Request(request),
                            payload: payload.clone(),
                            signed_capabilities: None,
                        })
                        .await
                        .unwrap();
                }
            }
        }

        // main message loop
        loop {
            tokio::select! {
                // debug mode toggle: when on, this loop becomes a manual step-through
                debug = recv_debug_in_loop.recv() => {
                    if let Some(t::DebugCommand::Toggle) = debug {
                        is_debug = !is_debug;
                    }
                },
                ne = network_error_recv.recv() => {
                    let Some(wrapped_network_error) = ne else { return Ok(()) };
                    let _ = send_to_terminal.send(
                        t::Printout {
                            verbosity: 1,
                            content: format!("event loop: got network error: {:?}", wrapped_network_error)
                        }
                    ).await;
                    // forward the error to the relevant process
                    match senders.get(&wrapped_network_error.source.process) {
                        Some(ProcessSender::Userspace(sender)) => {
                            // TODO: this failing should crash kernel
                            sender.send(Err(wrapped_network_error)).await.unwrap();
                        }
                        Some(ProcessSender::Runtime(_sender)) => {
                            // TODO should runtime modules get these? no
                            // this will change if a runtime process ever makes
                            // a message directed to not-our-node
                        }
                        None => {
                            send_to_terminal
                                .send(t::Printout {
                                    verbosity: 0,
                                    content: format!(
                                        "event loop: don't have {:?} amongst registered processes: {:?}",
                                        wrapped_network_error.source.process,
                                        senders.keys().collect::<Vec<_>>()
                                    )
                                })
                                .await
                                .unwrap();
                        }
                    }
                },
                kernel_message = recv_in_loop.recv() => {
                    let kernel_message = kernel_message.expect("fatal: event loop died");
                    //
                    // here: are the special kernel-level capabilities checks!
                    //
                    // enforce capabilities by matching from our set based on fixed format
                    // enforce that if message is directed over the network, process has capability to do so
                    if kernel_message.target.node != our_name {
                        if !process_map.get(&kernel_message.source.process).unwrap().capabilities.contains(
                                &t::Capability {
                                    issuer: t::Address {
                                    node: our_name.clone(),
                                    process: t::ProcessId::Name("kernel".into()),
                                },
                                params: "\"network\"".into(),
                        }) {
                            // capabilities are not correct! skip this message.
                            // TODO some kind of error thrown
                            let _ = send_to_terminal.send(
                                t::Printout {
                                    verbosity: 0,
                                    content: format!(
                                        "event loop: process {:?} doesn't have capability to send networked messages",
                                        kernel_message.source.process
                                    )
                                }
                            ).await;
                            continue;
                        }
                    } else {
                        // enforce that process has capability to message a target process of this name
                        match process_map.get(&kernel_message.source.process) {
                            None => {}
                            Some(persisted) => {
                                if !persisted.capabilities.contains(&t::Capability {
                                    issuer: t::Address {
                                        node: our_name.clone(),
                                        process: kernel_message.target.process.clone(),
                                    },
                                    params: format!(
                                        "{{\"messaging\": \"{}\"}}",
                                        serde_json::to_string(&kernel_message.target.process.clone()).unwrap()
                                    ),
                                }) {
                                    // capabilities are not correct! skip this message.
                                    // TODO do some kind of error or something
                                    let _ = send_to_terminal.send(
                                        t::Printout {
                                            verbosity: 0,
                                            content: format!(
                                                "event loop: process {:?} doesn't have capability to message process {:?}",
                                                kernel_message.source.process, kernel_message.target.process
                                            )
                                        }
                                    ).await;
                                    continue;
                                }
                            }
                        }
                    }
                    // end capabilities checks
                    while is_debug {
                        let debug = recv_debug_in_loop.recv().await.unwrap();
                        match debug {
                            t::DebugCommand::Toggle => is_debug = !is_debug,
                            t::DebugCommand::Step => break,
                        }
                    }
                    // display every single event when verbose
                    let _ = send_to_terminal.send(
                            t::Printout {
                                verbosity: 1,
                                content: format!("event loop: got message: {}", kernel_message)
                            }
                        ).await;
                    if our_name != kernel_message.target.node {
                        // unrecoverable if fails
                        send_to_net.send(kernel_message).await.expect("fatal: net module died");
                    } else {
                        if kernel_message.target.process == "kernel" {
                            // kernel only accepts messages from our own node
                            if our_name != kernel_message.source.node {
                                continue;
                            }
                            match kernel_message.message {
                                t::Message::Request(_) => {
                                    handle_kernel_request(
                                        our_name.clone(),
                                        kernel_message,
                                        send_to_loop.clone(),
                                        send_to_terminal.clone(),
                                        &mut senders,
                                        &mut process_handles,
                                        &mut process_map,
                                        caps_oracle_sender.clone(),
                                    ).await;
                                }
                                t::Message::Response(_) => {
                                    handle_kernel_response(
                                        our_name.clone(),
                                        keypair.clone(),
                                        home_directory_path.clone(),
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
                                    // TODO: this failing should crash kernel
                                    sender.send(Ok(kernel_message)).await.unwrap();
                                }
                                Some(ProcessSender::Runtime(sender)) => {
                                    sender.send(kernel_message).await.unwrap();
                                }
                                None => {
                                    send_to_terminal
                                        .send(t::Printout {
                                            verbosity: 0,
                                            content: format!(
                                                "event loop: don't have {:?} amongst registered processes: {:?}",
                                                kernel_message.target.process,
                                                senders.keys().collect::<Vec<_>>()
                                            )
                                        })
                                        .await
                                        .unwrap();
                                }
                            }
                        }
                    }
                },
                // capabilities oracle!!!
                Some(cap_message) = caps_oracle_receiver.recv() => {
                    match cap_message {
                        t::CapMessage::Add { on, cap } => {
                            // insert cap in process map
                            process_map.get_mut(&on).unwrap().capabilities.insert(cap);
                            let _ = persist_state(&our_name, &send_to_loop, &process_map).await;
                        },
                        t::CapMessage::Drop { on, cap } => {
                            // remove cap from process map
                            process_map.get_mut(&on).unwrap().capabilities.remove(&cap);
                            let _ = persist_state(&our_name, &send_to_loop, &process_map).await;
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
                            // return all caps on responder
                            let _ = responder.send(
                                match process_map.get(&on) {
                                    None => HashSet::new(),
                                    Some(p) => p.capabilities.clone(),
                                }
                            );
                        },
                    }
                }
            }
        }
    })
}

/// kernel entry point. creates event loop which contains all WASM processes
pub async fn kernel(
    our: t::Identity,
    keypair: Arc<signature::Ed25519KeyPair>,
    home_directory_path: String,
    process_map: t::ProcessMap,
    caps_oracle_sender: t::CapMessageSender,
    caps_oracle_receiver: t::CapMessageReceiver,
    send_to_loop: t::MessageSender,
    send_to_terminal: t::PrintSender,
    recv_in_loop: t::MessageReceiver,
    network_error_recv: t::NetworkErrorReceiver,
    recv_debug_in_loop: t::DebugReceiver,
    send_to_wss: t::MessageSender,
    send_to_fs: t::MessageSender,
    send_to_http_server: t::MessageSender,
    send_to_http_client: t::MessageSender,
    send_to_eth_rpc: t::MessageSender,
    send_to_vfs: t::MessageSender,
    send_to_encryptor: t::MessageSender,
) -> Result<()> {
    let mut config = Config::new();
    config.cache_config_load_default().unwrap();
    config.wasm_backtrace_details(WasmBacktraceDetails::Enable);
    config.wasm_component_model(true);
    config.async_support(true);
    let engine = Engine::new(&config).unwrap();

    let event_loop_handle = tokio::spawn(
        make_event_loop(
            our.name,
            keypair,
            home_directory_path,
            process_map,
            caps_oracle_sender,
            caps_oracle_receiver,
            recv_in_loop,
            network_error_recv,
            recv_debug_in_loop,
            send_to_loop,
            send_to_wss,
            send_to_fs,
            send_to_http_server,
            send_to_http_client,
            send_to_eth_rpc,
            send_to_vfs,
            send_to_encryptor,
            send_to_terminal,
            engine,
        )
        .await,
    );
    event_loop_handle.await.unwrap()
}
