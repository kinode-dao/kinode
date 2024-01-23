use kinode_process_lib::{
    Address,
    LazyLoadBlob as Blob,
    Message,
    ProcessId,
    Request, 
    Response,
    await_message,
    http,
    println
};
use kinode_process_lib::http::{
    WsMessageType,
    open_ws_connection_and_await,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
enum EthAction {
    Path,
}

wit_bindgen::generate!({
    path: "../../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

#[derive(Debug, Serialize, Deserialize)]
struct RpcPath {
    pub process_addr: Address,
    pub rpc_url: Option<String>,
}

enum ProviderAction {
    RpcPath(RpcPath)
}

struct State {
    conn: WsConnection
}

struct WsConnection {
    our: Address,
    channel: u32
}

impl WsConnection {

    fn new (our: Address, channel: u32) -> Self {
        Self {
            our,
            channel
        }
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

struct Component;
impl Guest for Component {
    fn init(our: String) {

        let our: Address = our.parse().unwrap();

        let _ = Request::new()
            .target(Address::new(&our.node, ProcessId::new(Some("eth"), "distro", "sys")))
            .body(serde_json::to_vec(&EthAction::Path).unwrap())
            .send();

        match main(our) {
            Ok(_) => {}
            Err(e) => {
                println!(": error: {:?}", e);
            }
        }
    }
}

fn main(our: Address) -> anyhow::Result<()> {

    let mut state = State {
        fulfillment: None,
        external_url: "".to_string(),
    };

    loop {
        match await_message() {
            Ok(msg) =>  {
                handle_message(&our, msg, &mut state);
                continue;
            }
            Err(e) => {
                break;
            }
            _ => {}
        }
    }

    Ok(())

}


fn handle_message(our: &Address, msg: Message, state: &mut State) -> anyhow::Result<()> {

    match msg {

        Message::Request { source, expects_response, body, metadata, capabilities } => { }
        Message::Response { source, body, metadata, context, capabilities } => {

            let rpc_path = serde_json::from_slice::<RpcPath>(&body).unwrap();

            if let Ok(msg) = http::open_ws_connection_and_await(
                our.node.clone(),
                rpc_path.rpc_url.unwrap(),
                None,
                123454321
            ) {

                state.conn = WsConnection::new(rpc_path.process_addr, msg.channel);
                
            } else {

            };

        }

        _ => {}
    }

    Ok(())
}