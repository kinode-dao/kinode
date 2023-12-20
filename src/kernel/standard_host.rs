use crate::kernel::process;
use crate::kernel::process::uqbar::process::standard as wit;
use crate::types as t;
use crate::types::STATE_PROCESS_ID;
use crate::KERNEL_PROCESS_ID;
use crate::VFS_PROCESS_ID;
use anyhow::Result;
use ring::signature::{self, KeyPair};
use std::collections::HashSet;

use crate::kernel::process::StandardHost;

///
/// create the process API. this is where the functions that a process can use live.
///
#[async_trait::async_trait]
impl StandardHost for process::ProcessWasi {
    //
    // system utils:
    //
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

    async fn get_eth_block(&mut self) -> Result<u64> {
        // TODO connect to eth RPC
        unimplemented!()
    }

    //
    // process management:
    //

    ///  TODO critical: move to kernel logic to enable persistence of choice made here
    async fn set_on_exit(&mut self, on_exit: wit::OnExit) -> Result<()> {
        self.process.metadata.on_exit = t::OnExit::de_wit(on_exit);
        Ok(())
    }

    async fn get_on_exit(&mut self) -> Result<wit::OnExit> {
        Ok(self.process.metadata.on_exit.en_wit())
    }

    /// create a message from the *kernel* to the filesystem,
    /// asking it to fetch the current state saved under this process
    async fn get_state(&mut self) -> Result<Option<Vec<u8>>> {
        let old_last_payload = self.process.last_payload.clone();
        let res = match process::send_and_await_response(
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
                ipc: serde_json::to_vec(&t::StateAction::GetState(
                    self.process.metadata.our.process.clone(),
                ))
                .unwrap(),
                metadata: Some(self.process.metadata.our.process.to_string()),
            },
            None,
        )
        .await
        {
            Ok(Ok(_resp)) => {
                // basically assuming filesystem responding properly here
                match &self.process.last_payload {
                    None => Ok(None),
                    Some(payload) => Ok(Some(payload.bytes.clone())),
                }
            }
            _ => Ok(None),
        };
        self.process.last_payload = old_last_payload;
        return res;
    }

    /// create a message from the *kernel* to the filesystem,
    /// asking it to replace the current state saved under
    /// this process with these bytes
    async fn set_state(&mut self, bytes: Vec<u8>) -> Result<()> {
        let old_last_payload = self.process.last_payload.clone();
        let res = match process::send_and_await_response(
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
                ipc: serde_json::to_vec(&t::StateAction::SetState(
                    self.process.metadata.our.process.clone(),
                ))
                .unwrap(),
                metadata: Some(self.process.metadata.our.process.to_string()),
            },
            Some(wit::Payload { mime: None, bytes }),
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
        self.process.last_payload = old_last_payload;
        return res;
    }

    /// create a message from the *kernel* to the filesystem,
    /// asking it to delete the current state saved under this process
    async fn clear_state(&mut self) -> Result<()> {
        let old_last_payload = self.process.last_payload.clone();
        let res = match process::send_and_await_response(
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
                ipc: serde_json::to_vec(&t::StateAction::DeleteState(
                    self.process.metadata.our.process.clone(),
                ))
                .unwrap(),
                metadata: None,
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
        self.process.last_payload = old_last_payload;
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
        capabilities: wit::Capabilities,
        public: bool,
    ) -> Result<Result<wit::ProcessId, wit::SpawnError>> {
        // save existing payload to restore later
        let old_last_payload = self.process.last_payload.clone();
        let vfs_address = wit::Address {
            node: self.process.metadata.our.node.clone(),
            process: VFS_PROCESS_ID.en_wit(),
        };
        let Ok(Ok((_, hash_response))) = process::send_and_await_response(
            self,
            None,
            vfs_address.clone(),
            wit::Request {
                inherit: false,
                expects_response: Some(5),
                ipc: serde_json::to_vec(&t::VfsRequest {
                    path: wasm_path.clone(),
                    action: t::VfsAction::Read,
                })
                .unwrap(),
                metadata: None,
            },
            None,
        )
        .await
        else {
            println!("spawn: GetHash fail");
            // reset payload to what it was
            self.process.last_payload = old_last_payload;
            return Ok(Err(wit::SpawnError::NoFileAtPath));
        };
        let wit::Message::Response((wit::Response { ipc, .. }, _)) = hash_response else {
            // reset payload to what it was
            self.process.last_payload = old_last_payload;
            return Ok(Err(wit::SpawnError::NoFileAtPath));
        };
        let t::VfsResponse::Read = serde_json::from_slice(&ipc).unwrap() else {
            // reset payload to what it was
            self.process.last_payload = old_last_payload;
            return Ok(Err(wit::SpawnError::NoFileAtPath));
        };
        let Some(t::Payload { mime: _, ref bytes }) = self.process.last_payload else {
            // reset payload to what it was
            self.process.last_payload = old_last_payload;
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
        );
        let Ok(Ok((_, _response))) = process::send_and_await_response(
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
                ipc: serde_json::to_vec(&t::KernelCommand::InitializeProcess {
                    id: new_process_id.clone(),
                    wasm_bytes_handle: wasm_path,
                    on_exit: t::OnExit::de_wit(on_exit),
                    initial_capabilities: match capabilities {
                        wit::Capabilities::None => HashSet::new(),
                        wit::Capabilities::All => {
                            let (tx, rx) = tokio::sync::oneshot::channel();
                            let _ = self
                                .process
                                .caps_oracle
                                .send(t::CapMessage::GetAll {
                                    on: self.process.metadata.our.process.clone(),
                                    responder: tx,
                                })
                                .await;
                            rx.await.unwrap()
                        }
                        wit::Capabilities::Some(caps) => caps
                            .into_iter()
                            .map(|cap| t::SignedCapability {
                                issuer: t::Address::de_wit(cap.issuer),
                                params: cap.params,
                                signature: cap.signature,
                            })
                            .collect(),
                    },
                    public,
                })
                .unwrap(),
                metadata: None,
            },
            Some(wit::Payload {
                mime: None,
                bytes: bytes.to_vec(),
            }),
        )
        .await
        else {
            // reset payload to what it was
            self.process.last_payload = old_last_payload;
            return Ok(Err(wit::SpawnError::NameTaken));
        };
        // finally, send the command to run the new process
        let Ok(Ok((_, response))) = process::send_and_await_response(
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
                ipc: serde_json::to_vec(&t::KernelCommand::RunProcess(new_process_id.clone()))
                    .unwrap(),
                metadata: None,
            },
            None,
        )
        .await
        else {
            // reset payload to what it was
            self.process.last_payload = old_last_payload;
            return Ok(Err(wit::SpawnError::NameTaken));
        };
        // reset payload to what it was
        self.process.last_payload = old_last_payload;
        let wit::Message::Response((wit::Response { ipc, .. }, _)) = response else {
            return Ok(Err(wit::SpawnError::NoFileAtPath));
        };
        let t::KernelResponse::StartedProcess = serde_json::from_slice(&ipc).unwrap() else {
            return Ok(Err(wit::SpawnError::NoFileAtPath));
        };
        // child processes are always able to Message parent
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.process
            .caps_oracle
            .send(t::CapMessage::Add {
                on: new_process_id.clone(),
                cap: t::Capability {
                    issuer: self.process.metadata.our.clone(),
                    params: "\"messaging\"".into(),
                },
                responder: tx,
            })
            .await
            .unwrap();
        let _ = rx.await.unwrap();

        // parent process is always able to Message child
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.process
            .caps_oracle
            .send(t::CapMessage::Add {
                on: self.process.metadata.our.process.clone(),
                cap: t::Capability {
                    issuer: t::Address {
                        node: self.process.metadata.our.node.clone(),
                        process: new_process_id.clone(),
                    },
                    params: "\"messaging\"".into(),
                },
                responder: tx,
            })
            .await
            .unwrap();
        let _ = rx.await.unwrap();
        Ok(Ok(new_process_id.en_wit().to_owned()))
    }

    //
    // capabilities management
    //
    async fn get_capabilities(&mut self) -> Result<Vec<wit::SignedCapability>> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = self
            .process
            .caps_oracle
            .send(t::CapMessage::GetAll {
                on: self.process.metadata.our.process.clone(),
                responder: tx,
            })
            .await;
        Ok(rx
            .await
            .unwrap()
            .into_iter()
            .map(|cap| wit::SignedCapability {
                issuer: cap.issuer.en_wit(),
                params: cap.params,
                signature: cap.signature,
            })
            .collect())
    }

    async fn get_capability(
        &mut self,
        issuer: wit::Address,
        params: String,
    ) -> Result<Option<wit::SignedCapability>> {
        let cap = t::Capability {
            issuer: t::Address::de_wit(issuer),
            params,
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = self
            .process
            .caps_oracle
            .send(t::CapMessage::Has {
                on: self.process.metadata.our.process.clone(),
                cap: cap.clone(),
                responder: tx,
            })
            .await;
        if rx.await.unwrap() {
            let sig = self
                .process
                .keypair
                .sign(&rmp_serde::to_vec(&cap).unwrap_or_default());
            return Ok(Some(wit::SignedCapability {
                issuer: cap.issuer.en_wit().to_owned(),
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
                self.process.next_message_caps =
                    Some(vec![t::de_wit_signed_capability(capability)]);
                Ok(())
            }
            Some(ref mut v) => {
                v.push(t::de_wit_signed_capability(capability));
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
                issuer: t::Address::de_wit(signed_cap.issuer),
                params: signed_cap.params,
            };
            pk.verify(
                &rmp_serde::to_vec(&cap).unwrap_or_default(),
                &signed_cap.signature,
            )?;

            let (tx, rx) = tokio::sync::oneshot::channel();
            let _ = self
                .process
                .caps_oracle
                .send(t::CapMessage::Add {
                    on: self.process.metadata.our.process.clone(),
                    cap: cap.clone(),
                    responder: tx,
                })
                .await?;
            let _ = rx.await?;
        }
        Ok(())
    }

    async fn has_capability(&mut self, from: wit::ProcessId, params: String) -> Result<bool> {
        if self.process.prompting_message.is_none() {
            return Err(anyhow::anyhow!(
                "kernel: has_capability() called with no prompting_message"
            ));
        }
        let prompt = self.process.prompting_message.as_ref().unwrap();
        if prompt.source.node == self.process.metadata.our.node {
            // if local, need to ask them
            let cap = t::Capability {
                issuer: self.process.metadata.our.clone(),
                params,
            };
            let (tx, rx) = tokio::sync::oneshot::channel();
            let _ = self
                .process
                .caps_oracle
                .send(t::CapMessage::Has {
                    on: t::ProcessId::de_wit(from),
                    cap,
                    responder: tx,
                })
                .await;
            Ok(rx.await.unwrap_or(false))
        } else {
            // if remote, just check prompting_message
            if prompt.signed_capabilities.is_none() {
                return Ok(false);
            }
            let addy = t::Address::de_wit(wit::Address {
                node: self.process.metadata.our.node.clone(),
                process: from,
            });
            for cap in prompt.signed_capabilities.as_ref().unwrap() {
                if cap.issuer == addy && cap.params == params {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
    }

    /// generate a new cap with this process as the issuer and send to caps oracle
    async fn create_capability(&mut self, params: String) -> Result<wit::SignedCapability> {
        let cap = t::Capability {
            issuer: self.process.metadata.our.clone(),
            params: params.clone(),
        };
        let sig = self
            .process
            .keypair
            .sign(&rmp_serde::to_vec(&cap).unwrap_or_default());
        return Ok(wit::SignedCapability {
            issuer: cap.issuer.en_wit().to_owned(),
            params: params,
            signature: sig.as_ref().to_vec(),
        });
    }

    async fn share_capability(
        &mut self,
        to: wit::ProcessId,
        signed_cap: wit::SignedCapability,
    ) -> Result<()> {
        let pk = signature::UnparsedPublicKey::new(
            &signature::ED25519,
            self.process.keypair.public_key(),
        );
        let cap = t::Capability {
            issuer: t::Address::de_wit(signed_cap.issuer),
            params: signed_cap.params,
        };
        pk.verify(
            &rmp_serde::to_vec(&cap).unwrap_or_default(),
            &signed_cap.signature,
        )?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = self
            .process
            .caps_oracle
            .send(t::CapMessage::Add {
                on: t::ProcessId::de_wit(to),
                cap,
                responder: tx,
            })
            .await?;
        let _ = rx.await?;
        Ok(())
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
        Ok(t::en_wit_payload(self.process.last_payload.clone()))
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
            .handle_request(None, target, request, context, payload)
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
                .handle_request(None, request.0, request.1, request.2, request.3)
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
        process::send_and_await_response(self, None, target, request, payload).await
    }
}
