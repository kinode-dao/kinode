use kinode_process_lib::eth_alloy::{
    AlloySubscribeLogsRequest
};
use kinode_process_lib::{
    Address,
    LazyLoadBlob as Blob,
    Message,
    ProcessId,
    Request, 
    await_message,
    http,
    println
};
use kinode_process_lib::http::{
    WsMessageType,
    HttpClientError,
    HttpClientResponse,
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


#[derive(Debug, Serialize, Deserialize)]
struct State {
    conn: WsConnection
}

#[derive(Debug, Serialize, Deserialize)]
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

        match main(our) {
            Ok(_) => {}
            Err(e) => {
                println!(": error: {:?}", e);
            }
        }
    }
}

fn main(our: Address) -> anyhow::Result<()> {

    println!("OUR! {:?}", our);

    let msg = Request::new()
        .target(Address::new(&our.node, ProcessId::new(Some("eth"), "distro", "sys")))
        .body(serde_json::to_vec(&EthAction::Path).unwrap())
        .send_and_await_response(5)
        .unwrap().unwrap();

    let rpc_path = serde_json::from_slice::<RpcPath>(&msg.body()).unwrap();

    let channel = 123454321;

    let msg = http::open_ws_connection_and_await
        (our.node.clone(), rpc_path.rpc_url.unwrap(), None, channel)
            .unwrap().unwrap();
    
    let mut state = match serde_json::from_slice::<Result<HttpClientResponse, HttpClientError>>(msg.body()) {
        Ok(Ok(HttpClientResponse::WebSocketAck)) => {
            State { conn: WsConnection::new(rpc_path.process_addr, channel) }
        },
        _ => {
            return Err(anyhow::anyhow!(": failed to open ws connection"))
        }
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

        Message::Request { source, expects_response, body, metadata, capabilities } => { 

            println!("~\n~\n~\n got request: {:?},{:?},{:?}", source, body, metadata);

        }

        Message::Response { source, body, metadata, context, capabilities } => { }

        _ => {}
    }

    Ok(())
}