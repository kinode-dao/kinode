use std::str::FromStr;

use hyperware::process::kns_indexer::IndexerRequest;
use hyperware_process_lib::{call_init, Address, Request};

wit_bindgen::generate!({
    path: "target/wit",
    world: "kns-indexer-sys-v0",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize, process_macros::SerdeJsonInto],
});

call_init!(init);
fn init(_our: Address) {
    // request timeout of 5s
    let kns = Address::from_str("our@kns-indexer:kns-indexer:sys").unwrap();

    let _resp = Request::to(kns)
        .body(IndexerRequest::Reset)
        .send_and_await_response(5)
        .unwrap()
        .unwrap();
}
