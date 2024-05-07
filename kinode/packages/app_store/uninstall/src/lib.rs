use kinode_process_lib::{
    await_next_message_body, call_init, println, Address, Message, PackageId, Request,
};

mod api;
use api::*;

wit_bindgen::generate!({
    path: "target/wit",
    world: "process",
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
        Request::to((our.node(), ("main", "app_store", "sys")))
            .body(serde_json::to_vec(&LocalRequest::Uninstall(package_id.clone())).unwrap())
            .send_and_await_response(5)
    else {
        println!("uninstall: failed to get a response from app_store..!");
        return;
    };

    let Ok(response) = serde_json::from_slice::<LocalResponse>(&body) else {
        println!("uninstall: failed to parse response from app_store..!");
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
            println!("uninstall: unexpected response from app_store..!");
            return;
        }
    }
}
