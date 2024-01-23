use kinode_process_lib::{
    Address,
    Message,
    ProcessId,
    Request, 
    Response,
    await_message,
    println
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
    fulfillment: Option<Address>,
    external_url: String,
}

struct Component;
impl Guest for Component {
    fn init(our: String) {

        let our: Address = our.parse().unwrap();

        Request::new()
            .target(Address::new( "our", ProcessId::new(Some("eth"), "distro", "sys")))
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
                handle_message(msg, &mut state);
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


fn handle_message(msg: Message, state: &mut State) -> anyhow::Result<()> {

    match msg {

        Message::Request { source, expects_response, body, metadata, capabilities } => {
            // Now you can use source, expects_response, body, metadata, capabilities directly
        }
        Message::Response { source, body, metadata, context, capabilities } => {

            let rpc_path = serde_json::from_slice::<RpcPath>(&body).unwrap();

            state.fulfillment = Some(rpc_path.process_addr.clone());
            state.external_url = rpc_path.rpc_url.unwrap_or("".to_string());
            
            println!("RPC_PATH {:?}", rpc_path);

            // Now you can use source, body, metadata, context, capabilities directly
        }

        _ => {}
    }

    Ok(())
}