use crate::eth::types::{ ProviderAction, EthRpcAction };
use crate::http::types::HttpServerAction;
use crate::types::*;
use anyhow::Result;
use ethers::core::types::Filter;
use ethers::prelude::Provider;
use ethers::types::{ValueOrArray, U256, U64};
use ethers_providers::{Middleware, StreamExt, Ws, Http};
use futures::SinkExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tokio::net::TcpStream;
use url::Url;

struct Connections {
    ws_stream: Option<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    ws_provider: Option<Provider<Ws>>,
    http_provider: Option<Provider<Http>>,
    uq_provider: Option<NodeId>
}

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

    let mut connections = Connections {
        ws_stream: None,
        ws_provider: None,
        http_provider: None,
        uq_provider: None
    };

    match Url::parse(&rpc_url).unwrap().scheme() {
        "http" | "https" => { unreachable!() }
        "ws" | "wss" => {
            let (_ws_stream, _) = connect_async(&rpc_url).await.expect("failed to connect");
            connections.ws_stream = Some(_ws_stream);
            connections.ws_provider = Some(Provider::<Ws>::connect(rpc_url.clone()).await?);
        }
        _ => { unreachable!() }
    }

    while let Some(km) = recv_in_client.recv().await {
        match km.message {
            Message::Request(Request { ref ipc, .. }) => {
                println!("eth request");
                handle_request(ipc, km.payload, &connections)?;
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

fn handle_request(ipc: &Vec<u8>, payload: Option<Payload>, connections: &Connections) -> Result<()> {

    match serde_json::from_slice::<ProviderAction>(ipc)? {
        ProviderAction::HttpServerAction(action) => {
            println!("http server action {:?}", action);
        }
        ProviderAction::EthRpcAction(action) => {
            println!("eth rpc action {:?}", action);
        }
    }

    Ok(())
}

fn handle_response(ipc: &Vec<u8>) -> Result<()> {
    let Ok(message) = serde_json::from_slice::<HttpServerAction>(ipc) else {
        return Ok(());
    };

    println!("response message {:?}", message);

    Ok(())
}
