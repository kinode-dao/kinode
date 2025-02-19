use std::str::FromStr;

use hyperware::process::hns_indexer::IndexerRequest;
use hyperware_process_lib::{call_init, Address, Request};

wit_bindgen::generate!({
    path: "target/wit",
    world: "hns-indexer-sys-v0",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize, process_macros::SerdeJsonInto],
});

call_init!(init);
fn init(_our: Address) {
    // request timeout of 5s
    let hns = Address::from_str("our@hns-indexer:hns-indexer:sys").unwrap();

    let _resp = Request::to(hns)
        .body(IndexerRequest::Reset)
        .send_and_await_response(5)
        .unwrap()
        .unwrap();
}
