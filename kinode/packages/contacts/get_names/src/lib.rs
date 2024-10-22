use crate::kinode::process::contacts::{Capabilities as ContactsCapability, Request as ContactsRequest, Response as ContactsResponse};
use kinode_process_lib::{call_init, println, Address, Capability, Request};

wit_bindgen::generate!({
    path: "target/wit",
    world: "contacts-sys-v0",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize, process_macros::SerdeJsonInto],
});

call_init!(init);
fn init(our: Address) {
    let contacts_process = Address::from((our.node(), ("contacts", "contacts", "sys")));

    let read_names_cap = Capability::new(
        &contacts_process,
        serde_json::to_string(&ContactsCapability::ReadNameOnly).unwrap()
    );

    let Ok(Ok(response)) = Request::to(&contacts_process)
        .body(ContactsRequest::GetNames)
        .capabilities(vec![read_names_cap])
        .send_and_await_response(5) else
    {
        println!("did not receive expected Response from contacts:contacts:sys");
        return;
    };

    let Ok(ContactsResponse::GetNames(names)) = response.body().try_into() else {
        println!("did not receive GetNames resposne from contacts:contacts:sys");
        return;
    };

    println!("{names:?}");
}
