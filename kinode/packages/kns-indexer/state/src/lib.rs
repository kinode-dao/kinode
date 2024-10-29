use kinode_process_lib::{eth, net, script, Address, Message, Request};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v0",
});

/// From main kns-indexer process
#[derive(Clone, Debug, Serialize, Deserialize)]
struct State {
    chain_id: u64,
    // what contract this state pertains to
    contract_address: eth::Address,
    // namehash to human readable name
    names: HashMap<String, String>,
    // human readable name to most recent on-chain routing information as json
    // TODO: optional params knsUpdate? also include tba.
    nodes: HashMap<String, net::KnsUpdate>,
    // last block we have an update from
    last_block: u64,
}

script!(init);
fn init(_our: Address, _args: String) -> String {
    // we don't take any args

    let Ok(Message::Response { body, .. }) =
        Request::to(("our", "kns-indexer", "kns-indexer", "sys"))
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
        return "failed to get state from kns-indexer".to_string();
    };
    let Ok(state) = serde_json::from_slice::<State>(&body) else {
        return "failed to deserialize state".to_string();
    };
    // can change later, but for now, just print every known node name
    let mut names = state.names.values().map(AsRef::as_ref).collect::<Vec<_>>();
    names.sort();
    format!(
        "\nrunning on chain id {}\nCA: {}\n{} known nodes as of block {}\n     {}",
        state.chain_id,
        state.contract_address,
        names.len(),
        state.last_block,
        names.join("\n     ")
    )
}
