use kinode_process_lib::{
    await_next_request_body, call_init, println, Address, Message, NodeId, PackageId, Request,
};
use serde::{Deserialize, Serialize};

wit_bindgen::generate!({
    path: "../../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

/// grabbed from main:app_store:sys
#[derive(Debug, Serialize, Deserialize)]
pub enum LocalRequest {
    Install(PackageId),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum LocalResponse {
    InstallResponse(InstallResponse),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum InstallResponse {
    Success,
    Failure,
}

call_init!(init);

fn init(our: Address) {
    let Ok(body) = await_next_request_body() else {
        println!("install: failed to get args!");
        return;
    };

    let arg = String::from_utf8(body).unwrap_or_default();

    if arg.is_empty() {
        println!("install: 1 argument required, the package id of the app");
        println!("example: install app:publisher.os");
        return;
    };

    let Ok(package_id) = arg.parse::<PackageId>() else {
        println!("install: invalid package id, make sure to include package name and publisher");
        println!("example: app_name:publisher_name");
        return;
    };

    let Ok(Ok(Message::Response { body, .. })) =
        Request::to((our.node(), ("main", "app_store", "sys")))
            .body(serde_json::to_vec(&LocalRequest::Install(package_id.clone())).unwrap())
            .send_and_await_response(5)
    else {
        println!("install: failed to get a response from app_store..!");
        return;
    };

    let Ok(response) = serde_json::from_slice::<LocalResponse>(&body) else {
        println!("install: failed to parse response from app_store..!");
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
    }
}
