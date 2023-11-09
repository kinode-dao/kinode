use crate::types::*;
use anyhow::Result;
use llama_cpp_rs::{
    options::{ModelOptions, PredictOptions},
    LLama,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
// use serde_json::json;
// use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
enum LlamaAction {
    Prompt(String), // TODO add grammar
}

#[derive(Debug, Serialize, Deserialize)]
pub enum LlamaError {
    NoRsvp,
    BadJson,
    NoJson,
    PromptFailed,
}


pub async fn llm(
    our: String,
    send_to_loop: MessageSender,
    mut recv_in_client: MessageReceiver,
    print_tx: PrintSender,
) -> Result<()> {
    while let Some(message) = recv_in_client.recv().await {
        let our = our.clone();
        let send_to_loop = send_to_loop.clone();
        let print_tx = print_tx.clone();

        let KernelMessage {
            ref source,
            ref rsvp,
            message:
                Message::Request(Request {
                    expects_response,
                    ipc: ref json_bytes,
                    ..
                }),
            ..
        } = message
        else {
            panic!("llm: bad message");
        };

        // let target = if expects_response.is_some() {
        //     Address {
        //         node: our.clone(),
        //         process: source.process.clone(),
        //     }
        // } else {
        //     let Some(rsvp) = rsvp else {
        //         send_to_loop
        //             .send(make_error_message(
        //                 our.clone(),
        //                 &message,
        //                 LlamaError::NoRsvp,
        //             ))
        //             .await
        //             .unwrap();
        //         continue;
        //     };
        //     rsvp.clone()
        // };

        // let Ok(action) = serde_json::from_slice::<LlamaAction>(&json_bytes) else {
        //     send_to_loop
        //         .send(make_error_message(
        //             our.clone(),
        //             &message,
        //             LlamaError::BadJson,
        //         ))
        //         .await
        //         .unwrap();
        //     continue;
        // };

        let _ = print_tx.send(Printout {
            verbosity: 0,
            content: "prompting".to_string(),
        }).await.unwrap();

        let llama = LLama::new(
            "./WizardLM-7B-uncensored.Q4_0.gguf".into(),
            &ModelOptions {
                n_gpu_layers: 0,
                ..Default::default()
            },
        ).unwrap();

        let res = llama
            .predict(
                "what are the national animals of ".into(),
                PredictOptions {
                    token_callback: {
                        Some(Box::new(move |token| {
                            let print_tx_for_async = print_tx.clone();
                            tokio::spawn(async move {
                                print_tx_for_async.send(Printout {
                                    verbosity: 0,
                                    content: format!("next token: {}", token)
                                }).await.unwrap();
                            });
                            true // The callback still synchronously returns a bool
                        }))
                    },
                    ..Default::default()
                },
            )
            .unwrap();
    }

    Ok(())
}

//
//  helpers
//

fn make_error_message(our_name: String, km: &KernelMessage, error: LlamaError) -> KernelMessage {
    KernelMessage {
        id: km.id,
        source: Address {
            node: our_name.clone(),
            process: FILESYSTEM_PROCESS_ID.clone(),
        },
        target: match &km.rsvp {
            None => km.source.clone(),
            Some(rsvp) => rsvp.clone(),
        },
        rsvp: None,
        message: Message::Response((
            Response {
                inherit: false,
                ipc: serde_json::to_vec::<Result<u64, LlamaError>>(&Err(error)).unwrap(),
                metadata: None,
            },
            None,
        )),
        payload: None,
        signed_capabilities: None,
    }
}
