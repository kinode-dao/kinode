use crate::http::types::HttpServerAction;
use crate::eth::types::EthRpcAction;
use crate::types::*;
use anyhow::Result;
use ethers::core::types::Filter;
use ethers::prelude::Provider;
use ethers::types::{ValueOrArray, U256, U64};
use ethers_providers::{Middleware, StreamExt, Ws};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::connect_async;
use futures::SinkExt;
use std::future::Future;
use std::pin::Pin;
use url::Url;

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

    // let dispatch = get_dispatch(rpc_url, send_to_loop.clone()).await;

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

async fn get_dispatch(rpc_url: String, send_to_loop: MessageSender) -> Box<dyn Fn(EthRpcAction) -> Pin<Box<dyn Future<Output = ()> + Send >>> {

    let parsed = Url::parse(&rpc_url).unwrap();

    match parsed.scheme() {
        "http" | "https" => { unreachable!() }
        "ws" | "wss" => { return ws_dispatch(rpc_url.clone(), send_to_loop).await }
        _ => { unreachable!() }
    }

}

async fn ws_dispatch(rpc_url: String, send_to_loop: MessageSender) -> Box<dyn Fn(EthRpcAction) -> Pin<Box<dyn Future<Output = ()> + Send >>> {

    let provider = Provider::<Ws>::connect(&rpc_url).await;

    Box::new(move |action| {
        let send_to_loop = send_to_loop.clone();
        let rpc_url = rpc_url.clone();
        Box::pin(async move {
            match action {
                EthRpcAction::JsonRpcRequest(json) => {
                    let (mut ws_stream, _) = connect_async(&rpc_url).await.expect("failed to connect");
                    ws_stream.send(tokio_tungstenite::tungstenite::Message::Text(json)).await.unwrap();

                    while let Some(msg) = ws_stream.next().await {

                    };
                }
                EthRpcAction::Eth(method) => {}
                EthRpcAction::Debug(method) => {}
                EthRpcAction::Net(method) => {}
                EthRpcAction::Trace(method) => {}
                EthRpcAction::TxPool(method) => {}
            };
        })
    })
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
