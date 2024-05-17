use kinode_process_lib::{call_init, net, println, Address, Message, Request};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

wit_bindgen::generate!({
    path: "target/wit",
    world: "process",
});

/// From main kns_indexer process
#[derive(Clone, Debug, Serialize, Deserialize)]
struct State {
    chain_id: u64,
    // what contract this state pertains to
    contract_address: String,
    // namehash to human readable name
    names: HashMap<String, String>,
    // human readable name to most recent on-chain routing information as json
    // NOTE: not every namehash will have a node registered
    nodes: HashMap<String, net::KnsUpdate>,
    // last block we have an update from
    block: u64,
}

call_init!(init);
fn init(_our: Address) {
    let Ok(Message::Response { body, .. }) =
        Request::to(("our", "kns_indexer", "kns_indexer", "sys"))
            .body(
                serde_json::json!({
                    "GetState": {
                        "block": 0
                    }
                })
                .to_string()
                .as_bytes()
                .to_vec(),
            )
            .send_and_await_response(10)
            .unwrap()
    else {
        println!("failed to get state from kns_indexer");
        return;
    };
    let state = serde_json::from_slice::<State>(&body).expect("failed to deserialize state");
    // can change later, but for now, just print every known node name
    let mut names = state.names.values().map(AsRef::as_ref).collect::<Vec<_>>();
    names.sort();
    println!(
        "\nrunning on chain id {}\nCA: {}\n{} known nodes as of block {}\n     {}",
        state.chain_id,
        state.contract_address,
        names.len(),
        state.block,
        names.join("\n     ")
    );
}
