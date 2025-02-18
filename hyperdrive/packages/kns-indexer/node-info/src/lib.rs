use hyperware::process::kns_indexer::{IndexerRequest, IndexerResponse, NodeInfoRequest};
use hyperware_process_lib::{println, script, Address, Request};
use std::str::FromStr;

wit_bindgen::generate!({
    path: "target/wit",
    world: "kns-indexer-sys-v0",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize, process_macros::SerdeJsonInto],
});

script!(init);
fn init(_our: Address, args: String) -> String {
    let node_name = args.split_whitespace().next().unwrap_or("").to_string();

    let kns = Address::from_str("our@kns-indexer:kns-indexer:sys").unwrap();

    let resp = Request::to(kns)
        .body(IndexerRequest::NodeInfo(NodeInfoRequest {
            name: node_name,
            block: 0,
        }))
        .send_and_await_response(5)
        .unwrap()
        .unwrap();

    let resp = serde_json::from_slice::<IndexerResponse>(&resp.body()).unwrap();

    match resp {
        IndexerResponse::NodeInfo(node_info) => {
            format!("node info: {node_info:#?}")
        }
        _ => "node info: name not found".to_string(),
    }
}
