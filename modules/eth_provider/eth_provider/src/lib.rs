use kinode_process_lib::{
    await_message, call_init,
    http::{
        self, open_ws_connection_and_await, HttpClientError, HttpClientResponse, WsMessageType,
    },
    println, Address, LazyLoadBlob as Blob, Message, ProcessId, Request, Response,
};
use serde::{Deserialize, Serialize};

wit_bindgen::generate!({
    path: "../../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

enum RpcProvider {
    Address(Address),
    WS(WsConnection),
    // http?
}

struct ProviderState {
    rpc: Option<RpcProvider>,
}

#[derive(Debug, Serialize, Deserialize)]
struct WsConnection {
    our: Address,
    channel: u32,
}

impl WsConnection {
    fn new(our: Address, channel: u32) -> Self {
        Self { our, channel }
    }

    fn send(&self, blob: Blob) {
        http::send_ws_client_push(
            self.our.node.clone(),
            self.channel,
            WsMessageType::Text,
            blob,
        );
    }
}

fn main(our: Address, state: &mut ProviderState) -> anyhow::Result<()> {
    // first, await a message from the kernel which will contain
    // rpc url or node.
    let mut rpc_url: Option<String> = None;
    loop {
        let Ok(Message::Request { source, body, .. }) = await_message() else {
            continue;
        };
        if source.process != "kernel:distro:sys" {
            continue;
        }
        rpc_url = Some(std::str::from_utf8(&body).unwrap().to_string());
        break;
    }

    println!(
        "eth_provider: starting with the rpc {}",
        rpc_url.as_ref().unwrap()
    );

    let channel: u32 = 6969;
    let msg =
        http::open_ws_connection_and_await(our.node.clone(), rpc_url.unwrap(), None, channel)??;

    loop {
        match await_message() {
            Ok(msg) => {
                handle_message(&our, msg, state);
                continue;
            }
            Err(e) => {
                println!("eth_provider: error: {:?}", e);
                continue;
            }
        }
    }
}

fn handle_message(our: &Address, msg: Message, state: &mut ProviderState) -> anyhow::Result<()> {
    match msg {
        Message::Request {
            source,
            expects_response,
            body,
            metadata,
            capabilities,
        } => {}
        Message::Response {
            source,
            body,
            metadata,
            context,
            capabilities,
        } => {}
    }

    Ok(())
}

call_init!(init);
fn init(our: Address) {
    println!("eth_provider: begin");

    let mut state = ProviderState { rpc: None };

    match main(our, &mut state) {
        Ok(_) => {}
        Err(e) => {
            println!("eth_provider: error: {:?}", e);
        }
    }
}
