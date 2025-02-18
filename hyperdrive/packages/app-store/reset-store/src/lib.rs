//! reset:app-store:sys
//! terminal script for resetting the app store.
//!
//! Usage:
//!     reset:app-store:sys
//!
//! Arguments:
//!
//!
use crate::hyperware::process::chain::{ChainRequest, ChainResponse};
use hyperware_process_lib::{call_init, println, Address, Message, Request};

wit_bindgen::generate!({
    path: "target/wit",
    generate_unused_types: true,
    world: "app-store-sys-v1",
    additional_derives: [PartialEq, serde::Deserialize, serde::Serialize, process_macros::SerdeJsonInto],
});

call_init!(init);
fn init(_our: Address) {
    // no args

    let Ok(Ok(Message::Response { body, .. })) = Request::to(("our", "chain", "app-store", "sys"))
        .body(ChainRequest::Reset)
        .send_and_await_response(5)
    else {
        println!("reset: failed to get a response from app-store..!");
        return;
    };

    let Ok(response) = body.try_into() else {
        println!("reset: failed to parse response from app-store..!");
        return;
    };

    match response {
        ChainResponse::ResetOk => {
            println!("successfully reset app-store");
        }
        _ => {
            println!("reset: unexpected response from app-store..!");
            return;
        }
    }
}
