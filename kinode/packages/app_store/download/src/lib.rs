use crate::kinode::process::downloads::{DownloadRequest, DownloadResponse};
use kinode_process_lib::{
    await_next_message_body, call_init, println, Address, Message, PackageId, Request,
};

wit_bindgen::generate!({
    path: "target/wit",
    generate_unused_types: true,
    world: "app-store-sys-v0",
    additional_derives: [PartialEq, serde::Deserialize, serde::Serialize],
});

call_init!(init);
fn init(our: Address) {
    let Ok(body) = await_next_message_body() else {
        println!("download: failed to get args!");
        return;
    };

    let args = String::from_utf8(body).unwrap_or_default();

    let Some((arg1, arg2)) = args.split_once(" ") else {
        println!("download: 2 arguments required, the node id to download from and the package id of the app");
        println!("example: download my-friend.os app:publisher.os");
        return;
    };

    let download_from: String = arg1.to_string();

    let Ok(package_id) = arg2.parse::<PackageId>() else {
        println!("download: invalid package id, make sure to include package name and publisher");
        println!("example: app_name:publisher_name");
        return;
    };

    let Ok(Ok(Message::Response { body, .. })) =
        Request::to((our.node(), ("downloads", "app_store", "sys")))
            .body(
                serde_json::to_vec(&DownloadRequest {
                    package_id: crate::kinode::process::main::PackageId {
                        package_name: package_id.package_name.clone(),
                        publisher_node: package_id.publisher_node.clone(),
                    },
                    download_from: Some(download_from.clone()),
                    desired_version_hash: "".to_string(), // TODO FIX
                })
                .unwrap(),
            )
            .send_and_await_response(5)
    else {
        println!("download: failed to get a response from app_store..!");
        return;
    };

    let Ok(response) = serde_json::from_slice::<DownloadResponse>(&body) else {
        println!("download: failed to parse response from app_store..!");
        return;
    };

    // TODO FIX
    match response {
        _ => {
            println!("download: unexpected response from app_store..!");
            return;
        }
    }
}
