use crate::hyperware::process::hns_indexer::{GetStateRequest, IndexerRequest, IndexerResponse};
use hyperware_process_lib::{eth, script, Address, Message, Request};

wit_bindgen::generate!({
    path: "target/wit",
    world: "hns-indexer-sys-v0",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize, process_macros::SerdeJsonInto],
});

script!(init);
fn init(_our: Address, _args: String) -> String {
    // we don't take any args

    let Ok(Message::Response { body, .. }) =
        Request::to(("our", "hns-indexer", "hns-indexer", "sys"))
            .body(IndexerRequest::GetState(GetStateRequest { block: 0 }))
            .send_and_await_response(10)
            .unwrap()
    else {
        return "failed to get state from hns-indexer".to_string();
    };
    let Ok(IndexerResponse::GetState(state)) = body.try_into() else {
        return "failed to deserialize state".to_string();
    };
    // can change later, but for now, just print every known node name
    let mut names = state
        .names
        .iter()
        .map(|(_k, v)| v.clone())
        .collect::<Vec<_>>();
    names.sort();
    let contract_address: [u8; 20] = state
        .contract_address
        .try_into()
        .expect("invalid contract addess: doesn't have 20 bytes");
    let contract_address: eth::Address = contract_address.into();
    format!(
        "\nrunning on chain id {}\nCA: {}\n{} known nodes as of block {}\n     {}",
        state.chain_id,
        contract_address,
        names.len(),
        state.last_block,
        names.join("\n     ")
    )
}
