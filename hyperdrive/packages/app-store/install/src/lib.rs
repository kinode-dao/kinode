//! install:app-store:sys
//! terminal script for installing apps from the app store.
//!
//! Usage:
//!     install:app-store:sys <package_id> <version_hash>
//!
//! Arguments:
//!     <package_id>    The package ID of the app (e.g., app:publisher.os)
//!     <version_hash>  The version hash of the app to install
//!
use crate::hyperware::process::main::{
    InstallPackageRequest, InstallResponse, LocalRequest, LocalResponse,
};
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
        println!("install: failed to get args!");
        return;
    };

    let arg = String::from_utf8(body).unwrap_or_default();
    let args: Vec<&str> = arg.split_whitespace().collect();

    if args.len() != 2 {
        println!(
            "install: 2 arguments required, the package id of the app and desired version_hash"
        );
        println!("example: install app:publisher.os f5d374ab50e66888a7c2332b22d0f909f2e3115040725cfab98dcae488916990");
        return;
    }

    let Ok(package_id) = args[0].parse::<PackageId>() else {
        println!("install: invalid package id, make sure to include package name and publisher");
        println!("example: app_name:publisher_name");
        return;
    };

    let version_hash = args[1].to_string();

    let Ok(Ok(Message::Response { body, .. })) =
        Request::to((our.node(), ("main", "app-store", "sys")))
            .body(LocalRequest::Install(InstallPackageRequest {
                package_id: crate::hyperware::process::main::PackageId {
                    package_name: package_id.package_name.clone(),
                    publisher_node: package_id.publisher_node.clone(),
                },
                version_hash,
                metadata: None,
            }))
            .send_and_await_response(5)
    else {
        println!("install: failed to get a response from app-store..!");
        return;
    };

    let Ok(response) = body.try_into() else {
        println!("install: failed to parse response from app-store..!");
        return;
    };

    match response {
        LocalResponse::InstallResponse(InstallResponse::Success) => {
            println!("successfully installed package {package_id}");
        }
        LocalResponse::InstallResponse(InstallResponse::Failure) => {
            println!("failed to install package {package_id}");
            println!("make sure that the package has been downloaded!")
        }
        _ => {
            println!("install: unexpected response from app-store..!");
            return;
        }
    }
}
