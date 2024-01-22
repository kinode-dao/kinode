pub mod device;
pub mod end;
pub mod link;
pub mod mixtral_sharded;
pub mod origin;
pub mod processor;
pub mod test;
pub mod token_output_stream;
pub mod util;
pub mod with_tracing;

use crate::MLError;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::types::*;

// TODO: Zen: This will have to be changed
const BASE_PATH: &str = "src/ml/models";

pub async fn ml(
    our: String,
    kernel_message_sender: MessageSender,
    mut ml_receiver: MessageReceiver,
    print_tx: PrintSender,
) -> Result<()> {
    let ml = Arc::new(Mutex::new(Ml::new()));

    loop {
        tokio::select! {
            Some(km) = ml_receiver.recv() => {
                if km.source.node != our { continue };
                let Message::Request(ref req) = km.message else { continue };
                let our = our.clone();
                let kernel_message_sender = kernel_message_sender.clone();
                let print_tx = print_tx.clone();
                let ml_clone = ml.clone();

                tokio::spawn(async move {
                    let mut ml_locked = ml_clone.lock().await;
                    let _ = handle_request(our.clone(), km.clone(), kernel_message_sender.clone(), print_tx.clone(), &mut *ml_locked).await;
                });
            }
        }
    }
}

async fn handle_request(
    our_node: String,
    km: KernelMessage,
    kernel_message_sender: MessageSender,
    print_itx: PrintSender,
    ml: &mut Ml,
) -> Result<(), MLError> {
    let KernelMessage {
        id,
        source,
        message,
        lazy_load_blob: blob,
        ..
    } = km.clone();
    let Message::Request(Request {
        body,
        expects_response,
        metadata,
        ..
    }) = message.clone()
    else {
        return Err(MLError::Temp); // TODO: Zen: Better error handling
    };

    let request: MLRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            println!("ml: got invalid Request: {}", e);
            return Err(MLError::Temp); // TODO: Zen: Better error handling
        }
    };

    // TODO: Zen: body, bytes like in vfs?
    let (body, bytes) = match request.action {
        MLAction::ListModel => {
            match ml.list_models() {
                Ok(models) => {
                    println!("ml: listing models: {:?}", models);
                    // TODO: Zen: Send response
                }
                Err(e) => return Err(MLError::Temp), // TODO: Zen: Better error handling
            }
            (vec![], None) // TODO: Zen
        }
        MLAction::LoadModel { model_name } => {
            ml.load_model(model_name);
            (vec![], None) // TODO: Zen
        }
        MLAction::UnloadModel => {
            ml.unload_model();
            (vec![], None) // TODO: Zen
        }
        MLAction::Infer { input } => {
            ml.forward();
            (vec![], None) // TODO: Zen
        }
    };

    if let Some(target) = km.rsvp.or_else(|| {
        expects_response.map(|_| Address {
            node: our_node.clone(),
            process: source.process.clone(),
        })
    }) {
        let response = KernelMessage {
            id,
            source: Address {
                node: our_node.clone(),
                process: ML_PROCESS_ID.clone(),
            },
            target,
            rsvp: None,
            message: Message::Response((
                Response {
                    inherit: false,
                    body,
                    metadata,
                    capabilities: vec![],
                },
                None,
            )),
            lazy_load_blob: bytes.map(|bytes| LazyLoadBlob {
                mime: Some("application/octet-stream".into()),
                bytes,
            }),
        };

        let _ = kernel_message_sender.send(response).await;
    }

    // TODO: Zen: Terminal logging like in vfs?

    Ok(())
}

pub struct Ml {}

impl Ml {
    pub fn new() -> Self {
        Self {}
    }

    /// List all available models that can be loaded.
    pub fn list_models(&self) -> Result<Vec<String>> {
        fs::read_dir(BASE_PATH)?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| model_dir_name_if_valid(entry.path()))
            .map(Ok)
            .collect()
    }

    pub fn load_model(&mut self, model_name: String) {}
    pub fn unload_model(&mut self) {}
    pub fn forward(&mut self) {}
}

/// Checks if dir exists and contains at least one file with the ".safetensors" extension.
/// Returns the name of the directory as a String if it is valid, otherwise None.
fn model_dir_name_if_valid(path: PathBuf) -> Option<String> {
    if !path.is_dir() {
        return None;
    }

    let has_safetensors = fs::read_dir(&path).ok().map_or(false, |mut entries| {
        entries.any(|entry| {
            entry
                .ok()
                .and_then(|e| e.path().extension().map(|ext| ext == "safetensors"))
                .unwrap_or(false)
        })
    });

    if !has_safetensors {
        return None;
    }

    path.file_name().and_then(|n| n.to_str()).map(String::from)
}

/*
TODO: Zen: Flesh out whether to use vfs, and which folder to use for holding models.
This should probably be on the node level. Let's use fs for now and get this done.
*/
