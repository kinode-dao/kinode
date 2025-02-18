//! uninstall:app-store:sys
//! terminal script for uninstalling apps from the app store.
//!
//! Usage:
//!     uninstall:app-store:sys <package_id>
//!
//! Arguments:
//!     <package_id>    The package ID of the app (e.g., app:publisher.os)
//!
use crate::hyperware::process::main::{LocalRequest, LocalResponse, UninstallResponse};
use hyperware_process_lib::{
    await_next_message_body, call_init, println, Address, Message, PackageId, Request,
};

wit_bindgen::generate!({
    path: "target/wit",
    generate_unused_types: true,
    world: "app-store-sys-v1",
    additional_derives: [PartialEq, serde::Deserialize, serde::Serialize, process_macros::SerdeJsonInto],
});

call_init!(init);
fn init(our: Address) {
    let Ok(body) = await_next_message_body() else {
        println!("uninstall: failed to get args!");
        return;
    };

    let arg = String::from_utf8(body).unwrap_or_default();

    if arg.is_empty() {
        println!("uninstall: 1 argument required, the package id of the app");
        println!("example: uninstall app:publisher.os");
        return;
    };

    let Ok(package_id) = arg.parse::<PackageId>() else {
        println!("uninstall: invalid package id, make sure to include package name and publisher");
        println!("example: app_name:publisher_name");
        return;
    };

    let Ok(Ok(Message::Response { body, .. })) =
        Request::to((our.node(), ("main", "app-store", "sys")))
            .body(LocalRequest::Uninstall(
                crate::hyperware::process::main::PackageId {
                    package_name: package_id.package_name.clone(),
                    publisher_node: package_id.publisher_node.clone(),
                },
            ))
            .send_and_await_response(5)
    else {
        println!("uninstall: failed to get a response from app-store..!");
        return;
    };

    let Ok(response) = body.try_into() else {
        println!("uninstall: failed to parse response from app-store..!");
        return;
    };

    match response {
        LocalResponse::UninstallResponse(UninstallResponse::Success) => {
            println!("successfully uninstalled package {package_id}");
        }
        LocalResponse::UninstallResponse(UninstallResponse::Failure) => {
            println!("failed to uninstall package {package_id}!");
        }
        _ => {
            println!("uninstall: unexpected response from app-store..!");
            return;
        }
    }
}
