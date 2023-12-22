use crate::http::types::HttpServerAction;
use crate::types::*;
use anyhow::Result;
use ethers::core::types::Filter;
use ethers::prelude::Provider;
use ethers::types::{ValueOrArray, U256, U64};
use ethers_providers::{Middleware, StreamExt, Ws};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

pub async fn provider(
    our: String,
    rpc_url: String,
    send_to_loop: MessageSender,
    mut recv_in_client: MessageReceiver,
    print_tx: PrintSender,
) -> Result<()> {
    println!("eth_rpc: starting");

    let open_ws = KernelMessage {
        id: rand::random(),
        source: Address {
            node: our.clone(),
            process: ETH_PROCESS_ID.clone(),
        },
        target: Address {
            node: our.clone(),
            process: HTTP_SERVER_PROCESS_ID.clone(),
        },
        rsvp: None,
        message: Message::Request(Request {
            inherit: false,
            ipc: serde_json::to_vec(&HttpServerAction::WebSocketBind {
                path: "/".to_string(),
                authenticated: false,
                encrypted: false,
            })
            .unwrap(),
            metadata: None,
            expects_response: None,
        }),
        payload: None,
        signed_capabilities: None,
    };

    send_to_loop.send(open_ws).await;

    while let Some(km) = recv_in_client.recv().await {
        match km.message {
            Message::Request(Request { ref ipc, .. }) => {
                println!("eth request");
                handle_request(ipc)?;
            }
            Message::Response((Response { ref ipc, .. }, ..)) => {
                println!("eth response");
                handle_response(ipc)?;
            }
            Message::Response(_) => todo!(),
            _ => {}
        }

        continue;
    }

    Ok(())
}

fn handle_request(ipc: &Vec<u8>) -> Result<()> {
    let Ok(message) = serde_json::from_slice::<HttpServerAction>(ipc) else {
        return Ok(());
    };

    println!("request message {:?}", message);

    Ok(())
}

fn handle_response(ipc: &Vec<u8>) -> Result<()> {
    let Ok(message) = serde_json::from_slice::<HttpServerAction>(ipc) else {
        return Ok(());
    };

    println!("response message {:?}", message);

    Ok(())
}
