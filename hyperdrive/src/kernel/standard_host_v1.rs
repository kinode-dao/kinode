use crate::kernel::process;
use anyhow::Result;
use lib::types::core::{self as t, KERNEL_PROCESS_ID, STATE_PROCESS_ID, VFS_PROCESS_ID};
use lib::v1::wit;
use lib::v1::wit::Host as StandardHost;
use ring::signature::{self, KeyPair};

async fn print_debug(proc: &process::ProcessState, content: &str) {
    t::Printout::new(
        2,
        &proc.metadata.our.process,
        format!(
            "{}:{}: {}",
            proc.metadata.our.process.package(),
            proc.metadata.our.process.publisher(),
            content
        ),
    )
    .send(&proc.send_to_terminal)
    .await;
}

impl process::ProcessState {
    /// Ingest latest message directed to this process, and save it as the current message.
    /// If there is no message in the queue, wait async until one is received.
    async fn get_next_message_for_process(
        &mut self,
    ) -> Result<(wit::Address, wit::Message), (wit::SendError, Option<wit::Context>)> {
        let res = match self.message_queue.pop_front() {
            Some(message_from_queue) => message_from_queue,
            None => self.ingest_message().await,
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
                    return self.kernel_message_to_process_receive(km);
                }
                _ => continue,
            }
        }
        // next, wait for the awaited message to arrive
        loop {
            let res = self.ingest_message().await;
            let id = match &res {
                Ok(km) => km.id,
                Err(e) => e.id,
            };
            if id == awaited_message_id {
                return self.kernel_message_to_process_receive(res);
            } else {
                self.message_queue.push_back(res);
            }
        }
    }

    /// ingest next valid message from kernel.
    /// cancel any timeout task associated with this message.
    /// if the message is a response, only enqueue if we have an outstanding request for it.
    async fn ingest_message(&mut self) -> Result<t::KernelMessage, t::WrappedSendError> {
        loop {
            let message = self
                .recv_in_process
                .recv()
                .await
                .expect("fatal: process couldn't receive next message");

            match &message {
                Ok(km) => match &km.message {
                    t::Message::Response(_) => {
                        if let Some((_context, timeout_handle)) = self.contexts.get_mut(&km.id) {
                            timeout_handle.abort();
                            return message;
                        }
                    }
                    _ => {
                        return message;
                    }
                },
                Err(e) => {
                    if let Some((_context, timeout_handle)) = self.contexts.get_mut(&e.id) {
                        timeout_handle.abort();
                        return message;
                    }
                }
            }
        }
    }

    /// Convert a message from the main event loop into a result for the process to receive.
    /// If the message is a response or error, get context if we have one.
    fn kernel_message_to_process_receive(
        &mut self,
        incoming: Result<t::KernelMessage, t::WrappedSendError>,
    ) -> Result<(wit::Address, wit::Message), (wit::SendError, Option<wit::Context>)> {
        let (mut km, context) = match incoming {
            Ok(mut km) => match km.message {
                t::Message::Request(t::Request {
                    ref expects_response,
                    ..
                }) => {
                    if km.lazy_load_blob.is_some() {
                        self.last_blob = km.lazy_load_blob;
                        km.lazy_load_blob = None;
                        self.last_message_blobbed = true;
                    } else {
                        self.last_message_blobbed = false;
                    }
                    if expects_response.is_some() || km.rsvp.is_some() {
                        // update prompting_message iff there is someone to reply to
                        self.prompting_message = Some(km.clone());
                    }
                    (km, None)
                }
                t::Message::Response(_) => match self.contexts.remove(&km.id) {
                    Some((context, _timeout_handle)) => {
                        if km.lazy_load_blob.is_some() {
                            self.last_blob = km.lazy_load_blob;
                            km.lazy_load_blob = None;
                            self.last_message_blobbed = true;
                        } else {
                            self.last_message_blobbed = false;
                        }
                        self.prompting_message = context.prompting_message;
                        (km, context.context)
                    }
                    None => {
                        if km.lazy_load_blob.is_some() {
                            self.last_blob = km.lazy_load_blob;
                            km.lazy_load_blob = None;
                            self.last_message_blobbed = true;
                        } else {
                            self.last_message_blobbed = false;
                        }
                        self.prompting_message = Some(km.clone());
                        (km, None)
                    }
                },
            },
            Err(e) => {
                self.last_message_blobbed = false;
                match self.contexts.remove(&e.id) {
                    None => return Err((t::en_wit_send_error(e.error), None)),
                    Some((context, _timeout_handle)) => {
                        self.prompting_message = context.prompting_message;
                        return Err((t::en_wit_send_error(e.error), context.context));
                    }
                }
            }
        };

        let pk = signature::UnparsedPublicKey::new(
            &signature::ED25519,
            self.keypair.as_ref().public_key(),
        );

        // prune any invalid capabilities before handing to process
        // where invalid = supposedly issued by us, but not signed properly by us
        match &mut km.message {
            t::Message::Request(request) => {
                request.capabilities.retain(|(cap, sig)| {
                    // The only time we verify a cap's signature is when a foreign node
                    // sends us a cap that we (allegedly) issued
                    if km.source.node != self.metadata.our.node
                        && cap.issuer.node == self.metadata.our.node
                    {
                        match pk.verify(&rmp_serde::to_vec(&cap).unwrap_or_default(), sig) {
                            Ok(_) => true,
                            Err(_) => false,
                        }
                    } else {
                        return true;
                    }
                });
            }
            t::Message::Response((response, _)) => {
                response.capabilities.retain(|(cap, sig)| {
                    // The only time we verify a cap's signature is when a foreign node
                    // sends us a cap that we (allegedly) issued
                    if km.source.node != self.metadata.our.node
                        && cap.issuer.node == self.metadata.our.node
                    {
                        match pk.verify(&rmp_serde::to_vec(&cap).unwrap_or_default(), sig) {
                            Ok(_) => true,
                            Err(_) => false,
                        }
                    } else {
                        return true;
                    }
                });
            }
        };

        Ok((
            km.source.en_wit(),
            match km.message {
                t::Message::Request(request) => wit::Message::Request(t::en_wit_request(request)),
                // NOTE: we throw away whatever context came from the sender, that's not ours
                t::Message::Response((response, _sent_context)) => {
                    wit::Message::Response((t::en_wit_response(response), context))
                }
            },
        ))
    }

    /// takes Request generated by a process and sends it to the main event loop.
    /// will only fail if process does not have capability to send to target.
    /// if the request has a timeout (expects response), start a task to track
    /// that timeout and return timeout error if it expires.
    async fn send_request(
        &mut self,
        // only used when kernel steps in to get/set state
        fake_source: Option<t::Address>,
        target: wit::Address,
        request: wit::Request,
        new_context: Option<wit::Context>,
        blob: Option<wit::LazyLoadBlob>,
    ) -> Result<u64> {
        let source = fake_source.unwrap_or(self.metadata.our.clone());
        let mut request = t::de_wit_request(request);

        // if request chooses to inherit, it means to take the ID and lazy_load_blob,
        // if any, from the last message it ingested

        // if request chooses to inherit, match id to precedessor
        // otherwise, id is generated randomly
        let request_id: u64 = if request.inherit && self.prompting_message.is_some() {
            self.prompting_message.as_ref().unwrap().id
        } else {
            loop {
                let id = rand::random();
                if !self.contexts.contains_key(&id) {
                    break id;
                }
            }
        };

        // if a blob is provided, it will be used; otherwise, if inherit is true,
        // and a predecessor exists, its blob will be used; otherwise, no blob will be used.
        let blob = match blob {
            Some(p) => Some(t::LazyLoadBlob {
                mime: p.mime,
                bytes: p.bytes,
            }),
            None => match request.inherit {
                true => self.last_blob.clone(),
                false => None,
            },
        };

        if !request.capabilities.is_empty() {
            request.capabilities = {
                let (tx, rx) = tokio::sync::oneshot::channel();
                self.caps_oracle
                    .send(t::CapMessage::FilterCaps {
                        on: self.metadata.our.process.clone(),
                        caps: request
                            .capabilities
                            .into_iter()
                            .map(|(cap, _)| cap)
                            .collect(),
                        responder: tx,
                    })
                    .await
                    .expect("fatal: process couldn't access capabilities oracle");
                rx.await
                    .expect("fatal: process couldn't receive capabilities")
            };
        }

        // if the request expects a response, modify the process' context map as needed
        // and set a timer.
        // TODO optimize this SIGNIFICANTLY: stop spawning tasks
        // and use a global clock + garbage collect step to check for timeouts
        if let Some(timeout_secs) = request.expects_response {
            let this_request = request.clone();
            let this_blob = blob.clone();
            let self_sender = self.self_sender.clone();
            let original_target = t::Address::de_wit(target.clone());
            let timeout_handle = tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(timeout_secs)).await;
                let _ = self_sender
                    .send(Err(t::WrappedSendError {
                        id: request_id,
                        source: original_target.clone(),
                        error: t::SendError {
                            kind: t::SendErrorKind::Timeout,
                            target: original_target,
                            message: t::Message::Request(this_request),
                            lazy_load_blob: this_blob,
                        },
                    }))
                    .await;
            });
            self.contexts.insert(
                request_id,
                (
                    process::ProcessContext {
                        prompting_message: self.prompting_message.clone(),
                        context: new_context,
                    },
                    timeout_handle,
                ),
            );
        }

        // rsvp is set based on this priority:
        // 1. whether this request expects a response -- if so, rsvp = our address, always
        // 2. whether this request inherits -- if so, rsvp = prompting message's rsvp
        // 3. if neither, rsvp = None
        let kernel_message = t::KernelMessage {
            id: request_id,
            source,
            target: t::Address::de_wit(target),
            rsvp: match (
                request.expects_response,
                request.inherit,
                &self.prompting_message,
            ) {
                (Some(_), _, _) => {
                    // this request expects response, so receives any response
                    // make sure to use the real source, not a fake injected-by-kernel source
                    Some(self.metadata.our.clone())
                }
                (None, true, Some(ref prompt)) => {
                    // this request inherits, so response will be routed to prompting message
                    prompt.rsvp.clone()
                }
                _ => None,
            },
            message: t::Message::Request(request),
            lazy_load_blob: blob,
        };

        self.send_to_loop
            .send(kernel_message)
            .await
            .expect("fatal: kernel couldn't send request");

        Ok(request_id)
    }

    /// takes Response generated by a process and sends it to the main event loop.
    async fn send_response(&mut self, response: wit::Response, blob: Option<wit::LazyLoadBlob>) {
        let mut response = t::de_wit_response(response);

        // the process requires a prompting_message in order to issue a response
        let Some(ref prompting_message) = self.prompting_message else {
            t::Printout::new(
                0,
                KERNEL_PROCESS_ID.clone(),
                format!("kernel: need non-None prompting_message to handle Response {response:?}"),
            )
            .send(&self.send_to_terminal)
            .await;
            return;
        };

        // given the current process state, produce the id and target that
        // a response it emits should have.
        let (id, target) = (
            prompting_message.id,
            match &prompting_message.rsvp {
                None => prompting_message.source.clone(),
                Some(rsvp) => rsvp.clone(),
            },
        );

        let blob = match response.inherit {
            true => self.last_blob.clone(),
            false => t::de_wit_blob(blob),
        };

        if !response.capabilities.is_empty() {
            response.capabilities = {
                let (tx, rx) = tokio::sync::oneshot::channel();
                self.caps_oracle
                    .send(t::CapMessage::FilterCaps {
                        on: self.metadata.our.process.clone(),
                        caps: response
                            .capabilities
                            .into_iter()
                            .map(|(cap, _)| cap)
                            .collect(),
                        responder: tx,
                    })
                    .await
                    .expect("fatal: process couldn't access capabilities oracle");
                rx.await
                    .expect("fatal: process couldn't receive capabilities")
            };
        }

        self.send_to_loop
            .send(t::KernelMessage {
                id,
                source: self.metadata.our.clone(),
                target,
                rsvp: None,
                message: t::Message::Response((
                    response,
                    // the context will be set by the process receiving this Response.
                    None,
                )),
                lazy_load_blob: blob,
            })
            .await
            .expect("fatal: kernel couldn't send response");
    }
}

async fn send_and_await_response(
    process: &mut process::ProcessWasiV1,
    source: Option<t::Address>,
    target: wit::Address,
    request: wit::Request,
    blob: Option<wit::LazyLoadBlob>,
) -> Result<Result<(wit::Address, wit::Message), wit::SendError>> {
    if request.expects_response.is_none() {
        return Err(anyhow::anyhow!(
            "kernel: got invalid send_and_await_response() Request from {:?}: must expect response",
            process.process.metadata.our.process
        ));
    }
    if t::Address::de_wit(target.clone()) == process.process.metadata.our {
        return Err(anyhow::anyhow!(
            "kernel: got invalid send_and_await_response() Request from and to {}: cannot await a Request to `our`: will deadlock",
            process.process.metadata.our,
        ));
    }
    let id = process
        .process
        .send_request(source, target, request, None, blob)
        .await;
    match id {
        Ok(id) => match process.process.get_specific_message_for_process(id).await {
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

///
/// create the process API. this is where the functions that a process can use live.
///
#[async_trait::async_trait]
impl StandardHost for process::ProcessWasiV1 {
    //
    // system utils:
    //

    /// Print a message to the runtime terminal. Add the name of the process to the
    /// beginning of the string, so user can verify source.
    async fn print_to_terminal(&mut self, verbosity: u8, content: String) -> Result<()> {
        self.process
            .send_to_terminal
            .send(t::Printout::new(
                verbosity,
                &self.process.metadata.our.process,
                format!(
                    "{}:{}: {}",
                    self.process.metadata.our.process.package(),
                    self.process.metadata.our.process.publisher(),
                    content
                ),
            ))
            .await
            .map_err(|e| anyhow::anyhow!("fatal: couldn't send to terminal: {e:?}"))
    }

    async fn our(&mut self) -> Result<wit::Address> {
        Ok(self.process.metadata.our.en_wit())
    }

    //
    // process management:
    //

    async fn set_on_exit(&mut self, on_exit: wit::OnExit) -> Result<()> {
        let on_exit = t::OnExit::de_wit(on_exit);
        self.process.metadata.on_exit = on_exit.clone();
        match self
            .process
            .send_request(
                Some(t::Address {
                    node: self.process.metadata.our.node.clone(),
                    process: KERNEL_PROCESS_ID.clone(),
                }),
                wit::Address {
                    node: self.process.metadata.our.node.clone(),
                    process: KERNEL_PROCESS_ID.en_wit(),
                },
                wit::Request {
                    inherit: false,
                    expects_response: None,
                    body: serde_json::to_vec(&t::KernelCommand::SetOnExit {
                        target: self.process.metadata.our.process.clone(),
                        on_exit,
                    })
                    .unwrap(),
                    metadata: None,
                    capabilities: vec![],
                },
                None,
                None,
            )
            .await
        {
            Ok(_) => {
                print_debug(&self.process, "set new on-exit behavior").await;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    async fn get_on_exit(&mut self) -> Result<wit::OnExit> {
        Ok(self.process.metadata.on_exit.en_wit())
    }

    /// create a message from the *kernel* to the filesystem,
    /// asking it to fetch the current state saved under this process
    async fn get_state(&mut self) -> Result<Option<Vec<u8>>> {
        let old_last_blob = self.process.last_blob.clone();
        let res = match send_and_await_response(
            self,
            Some(t::Address {
                node: self.process.metadata.our.node.clone(),
                process: KERNEL_PROCESS_ID.clone(),
            }),
            wit::Address {
                node: self.process.metadata.our.node.clone(),
                process: STATE_PROCESS_ID.en_wit(),
            },
            wit::Request {
                inherit: false,
                expects_response: Some(5),
                body: serde_json::to_vec(&t::StateAction::GetState(
                    self.process.metadata.our.process.clone(),
                ))
                .unwrap(),
                metadata: Some(self.process.metadata.our.process.to_string()),
                capabilities: vec![],
            },
            None,
        )
        .await
        {
            Ok(Ok(_resp)) => {
                // basically assuming filesystem responding properly here
                if self.process.last_message_blobbed {
                    match &self.process.last_blob {
                        None => Ok(None),
                        Some(blob) => Ok(Some(blob.bytes.clone())),
                    }
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        };
        self.process.last_blob = old_last_blob;
        return res;
    }

    /// create a message from the *kernel* to the filesystem,
    /// asking it to replace the current state saved under
    /// this process with these bytes
    async fn set_state(&mut self, bytes: Vec<u8>) -> Result<()> {
        let old_last_blob = self.process.last_blob.clone();
        let res = match send_and_await_response(
            self,
            Some(t::Address {
                node: self.process.metadata.our.node.clone(),
                process: KERNEL_PROCESS_ID.clone(),
            }),
            wit::Address {
                node: self.process.metadata.our.node.clone(),
                process: STATE_PROCESS_ID.en_wit(),
            },
            wit::Request {
                inherit: false,
                expects_response: Some(5),
                body: serde_json::to_vec(&t::StateAction::SetState(
                    self.process.metadata.our.process.clone(),
                ))
                .unwrap(),
                metadata: Some(self.process.metadata.our.process.to_string()),
                capabilities: vec![],
            },
            Some(wit::LazyLoadBlob { mime: None, bytes }),
        )
        .await
        {
            Ok(Ok(_resp)) => {
                // basically assuming filesystem responding properly here
                Ok(())
            }
            _ => Err(anyhow::anyhow!(
                "filesystem did not respond properly to SetState!!"
            )),
        };
        self.process.last_blob = old_last_blob;
        print_debug(&self.process, "persisted state").await;
        return res;
    }

    /// create a message from the *kernel* to the filesystem,
    /// asking it to delete the current state saved under this process
    async fn clear_state(&mut self) -> Result<()> {
        let old_last_blob = self.process.last_blob.clone();
        let res = match send_and_await_response(
            self,
            Some(t::Address {
                node: self.process.metadata.our.node.clone(),
                process: KERNEL_PROCESS_ID.clone(),
            }),
            wit::Address {
                node: self.process.metadata.our.node.clone(),
                process: STATE_PROCESS_ID.en_wit(),
            },
            wit::Request {
                inherit: false,
                expects_response: Some(5),
                body: serde_json::to_vec(&t::StateAction::DeleteState(
                    self.process.metadata.our.process.clone(),
                ))
                .unwrap(),
                metadata: None,
                capabilities: vec![],
            },
            None,
        )
        .await
        {
            Ok(Ok(_resp)) => {
                // basically assuming filesystem responding properly here
                Ok(())
            }
            _ => Err(anyhow::anyhow!(
                "filesystem did not respond properly to ClearState!!"
            )),
        };
        self.process.last_blob = old_last_blob;
        print_debug(&self.process, "cleared persisted state").await;
        return res;
    }

    /// shortcut to spawn a new process. the child process will automatically
    /// be able to send messages to the parent process, and vice versa.
    /// the .wasm file for the process must already be in VFS.
    async fn spawn(
        &mut self,
        name: Option<String>,
        wasm_path: String, // must be located within package's drive
        on_exit: wit::OnExit,
        request_capabilities: Vec<wit::Capability>,
        grant_capabilities: Vec<(wit::ProcessId, wit::Json)>,
        public: bool,
    ) -> Result<Result<wit::ProcessId, wit::SpawnError>> {
        // save existing blob to restore later
        let old_last_blob = self.process.last_blob.clone();
        let vfs_address = wit::Address {
            node: self.process.metadata.our.node.clone(),
            process: VFS_PROCESS_ID.en_wit(),
        };
        let Ok(Ok((_, hash_response))) = send_and_await_response(
            self,
            None,
            vfs_address.clone(),
            wit::Request {
                inherit: false,
                expects_response: Some(5),
                body: serde_json::to_vec(&t::VfsRequest {
                    path: wasm_path.clone(),
                    action: t::VfsAction::Read,
                })
                .unwrap(),
                metadata: None,
                capabilities: vec![],
            },
            None,
        )
        .await
        else {
            println!("spawn: GetHash fail");
            // reset blob to what it was
            self.process.last_blob = old_last_blob;
            return Ok(Err(wit::SpawnError::NoFileAtPath));
        };
        let wit::Message::Response((wit::Response { body, .. }, _)) = hash_response else {
            // reset blob to what it was
            self.process.last_blob = old_last_blob;
            return Ok(Err(wit::SpawnError::NoFileAtPath));
        };
        let t::VfsResponse::Read = serde_json::from_slice(&body).unwrap() else {
            // reset blob to what it was
            self.process.last_blob = old_last_blob;
            return Ok(Err(wit::SpawnError::NoFileAtPath));
        };
        let Some(t::LazyLoadBlob { mime: _, ref bytes }) = self.process.last_blob else {
            // reset blob to what it was
            self.process.last_blob = old_last_blob;
            return Ok(Err(wit::SpawnError::NoFileAtPath));
        };

        let name = match name {
            Some(name) => name,
            None => rand::random::<u64>().to_string(),
        };
        let new_process_id = t::ProcessId::new(
            Some(&name),
            self.process.metadata.our.process.package(),
            self.process.metadata.our.process.publisher(),
        )
        .check()?;

        let request_capabilities_filtered = {
            let (tx, rx) = tokio::sync::oneshot::channel();
            self.process
                .caps_oracle
                .send(t::CapMessage::FilterCaps {
                    on: self.process.metadata.our.process.clone(),
                    caps: request_capabilities
                        .into_iter()
                        .map(|cap| t::de_wit_capability(cap).0)
                        .collect(),
                    responder: tx,
                })
                .await
                .expect("fatal: process couldn't access capabilities oracle");
            rx.await
                .expect("fatal: process couldn't receive capabilities")
        };

        let Ok(Ok((_, _response))) = send_and_await_response(
            self,
            Some(t::Address {
                node: self.process.metadata.our.node.clone(),
                process: KERNEL_PROCESS_ID.clone(),
            }),
            wit::Address {
                node: self.process.metadata.our.node.clone(),
                process: KERNEL_PROCESS_ID.en_wit(),
            },
            wit::Request {
                inherit: false,
                expects_response: Some(5), // TODO evaluate
                body: serde_json::to_vec(&t::KernelCommand::InitializeProcess {
                    id: new_process_id.clone(),
                    wasm_bytes_handle: wasm_path,
                    wit_version: self.process.metadata.wit_version,
                    on_exit: t::OnExit::de_wit(on_exit),
                    initial_capabilities: request_capabilities_filtered
                        .into_iter()
                        .map(|(cap, _sig)| cap)
                        .collect(),
                    public,
                })
                .unwrap(),
                metadata: None,
                capabilities: vec![],
            },
            Some(wit::LazyLoadBlob {
                mime: None,
                bytes: bytes.to_vec(),
            }),
        )
        .await
        else {
            // reset blob to what it was
            self.process.last_blob = old_last_blob;
            return Ok(Err(wit::SpawnError::NameTaken));
        };
        // insert messaging capabilities into requested processes
        for (process_id, params) in grant_capabilities {
            let (tx, rx) = tokio::sync::oneshot::channel();
            self.process
                .caps_oracle
                .send(t::CapMessage::Add {
                    on: t::ProcessId::de_wit(process_id),
                    caps: vec![t::Capability::new(
                        (self.process.metadata.our.node.clone(), &new_process_id),
                        params,
                    )],
                    responder: Some(tx),
                })
                .await
                .unwrap();
            let _ = rx.await.unwrap();
        }
        // finally, send the command to run the new process
        let Ok(Ok((_, response))) = send_and_await_response(
            self,
            Some(t::Address {
                node: self.process.metadata.our.node.clone(),
                process: KERNEL_PROCESS_ID.clone(),
            }),
            wit::Address {
                node: self.process.metadata.our.node.clone(),
                process: KERNEL_PROCESS_ID.en_wit(),
            },
            wit::Request {
                inherit: false,
                expects_response: Some(5), // TODO evaluate
                body: serde_json::to_vec(&t::KernelCommand::RunProcess(new_process_id.clone()))
                    .unwrap(),
                metadata: None,
                capabilities: vec![],
            },
            None,
        )
        .await
        else {
            // reset blob to what it was
            self.process.last_blob = old_last_blob;
            return Ok(Err(wit::SpawnError::NameTaken));
        };
        // reset blob to what it was
        self.process.last_blob = old_last_blob;
        let wit::Message::Response((wit::Response { body, .. }, _)) = response else {
            return Ok(Err(wit::SpawnError::NoFileAtPath));
        };
        let t::KernelResponse::StartedProcess = serde_json::from_slice(&body).unwrap() else {
            return Ok(Err(wit::SpawnError::NoFileAtPath));
        };
        // child processes are always able to Message parent
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.process
            .caps_oracle
            .send(t::CapMessage::Add {
                on: new_process_id.clone(),
                caps: vec![t::Capability::messaging(self.process.metadata.our.clone())],
                responder: Some(tx),
            })
            .await
            .unwrap();
        rx.await.unwrap();

        // parent process is always able to Message child
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.process
            .caps_oracle
            .send(t::CapMessage::Add {
                on: self.process.metadata.our.process.clone(),
                caps: vec![t::Capability::messaging((
                    self.process.metadata.our.node.clone(),
                    &new_process_id,
                ))],
                responder: Some(tx),
            })
            .await
            .unwrap();
        rx.await.unwrap();
        print_debug(&self.process, "spawned a new process").await;
        Ok(Ok(new_process_id.en_wit().to_owned()))
    }

    //
    // capabilities management
    //

    async fn save_capabilities(&mut self, caps: Vec<wit::Capability>) -> Result<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = self
            .process
            .caps_oracle
            .send(t::CapMessage::Add {
                on: self.process.metadata.our.process.clone(),
                caps: caps
                    .iter()
                    .map(|cap| t::de_wit_capability(cap.clone()).0)
                    .collect(),
                responder: Some(tx),
            })
            .await?;
        let _ = rx.await?;
        Ok(())
    }

    async fn drop_capabilities(&mut self, caps: Vec<wit::Capability>) -> Result<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = self
            .process
            .caps_oracle
            .send(t::CapMessage::Drop {
                on: self.process.metadata.our.process.clone(),
                caps: caps
                    .iter()
                    .map(|cap| t::de_wit_capability(cap.clone()).0)
                    .collect(),
                responder: Some(tx),
            })
            .await?;
        let _ = rx.await?;
        Ok(())
    }

    async fn our_capabilities(&mut self) -> Result<Vec<wit::Capability>> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = self
            .process
            .caps_oracle
            .send(t::CapMessage::GetAll {
                on: self.process.metadata.our.process.clone(),
                responder: tx,
            })
            .await?;
        let caps = rx.await?;
        Ok(caps
            .into_iter()
            .map(|cap| t::en_wit_capability(cap))
            .collect())
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

    /// from a process: check if the last message received had a blob.
    async fn has_blob(&mut self) -> Result<bool> {
        Ok(self.process.last_message_blobbed)
    }

    /// from a process: grab the blob part of the last message received.
    /// if the last message did not have a blob, will return None.
    async fn get_blob(&mut self) -> Result<Option<wit::LazyLoadBlob>> {
        Ok(if self.process.last_message_blobbed {
            t::en_wit_blob(self.process.last_blob.clone())
        } else {
            None
        })
    }

    /// from a process: grab the **most recent** blob that has ever been received.
    /// if no blobs have ever been received, will return None.
    async fn last_blob(&mut self) -> Result<Option<wit::LazyLoadBlob>> {
        Ok(t::en_wit_blob(self.process.last_blob.clone()))
    }

    async fn send_request(
        &mut self,
        target: wit::Address,
        request: wit::Request,
        context: Option<wit::Context>,
        blob: Option<wit::LazyLoadBlob>,
    ) -> Result<()> {
        let id = self
            .process
            .send_request(None, target, request, context, blob)
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
            Option<wit::LazyLoadBlob>,
        )>,
    ) -> Result<()> {
        for request in requests {
            let id = self
                .process
                .send_request(None, request.0, request.1, request.2, request.3)
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
        blob: Option<wit::LazyLoadBlob>,
    ) -> Result<()> {
        self.process.send_response(response, blob).await;
        Ok(())
    }

    async fn send_and_await_response(
        &mut self,
        target: wit::Address,
        request: wit::Request,
        blob: Option<wit::LazyLoadBlob>,
    ) -> Result<Result<(wit::Address, wit::Message), wit::SendError>> {
        send_and_await_response(self, None, target, request, blob).await
    }
}
