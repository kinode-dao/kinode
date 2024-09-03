use crate::kinode::process::downloads::DownloadRequests;
use kinode::process::downloads::LocalDownloadRequest;
use kinode_process_lib::{
    await_next_message_body, call_init, println, Address, PackageId, Request,
};

wit_bindgen::generate!({
    path: "target/wit",
    generate_unused_types: true,
    world: "app-store-sys-v1",
    additional_derives: [PartialEq, serde::Deserialize, serde::Serialize],
});

call_init!(init);
fn init(our: Address) {
    let Ok(body) = await_next_message_body() else {
        println!("download: failed to get args!");
        return;
    };

    let args = String::from_utf8(body).unwrap_or_default();
    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.len() != 3 {
        println!("download: 3 arguments required, the node id to download from, the package id of the app, and the version hash");
        println!("example: download my-friend.os app:publisher.os f5d374ab50e66888a7c2332b22d0f909f2e3115040725cfab98dcae488916990");
        return;
    }
    let (arg1, arg2, arg3) = (parts[0], parts[1], parts[2]);

    let download_from: String = arg1.to_string();

    let Ok(package_id) = arg2.parse::<PackageId>() else {
        println!("download: invalid package id, make sure to include package name and publisher");
        println!("example: app_name:publisher_name");
        return;
    };

    let version_hash: String = arg3.to_string();

    let Ok(_) = Request::to((our.node(), ("downloads", "app_store", "sys")))
        .body(
            serde_json::to_vec(&DownloadRequests::LocalDownload(LocalDownloadRequest {
                package_id: crate::kinode::process::main::PackageId {
                    package_name: package_id.package_name.clone(),
                    publisher_node: package_id.publisher_node.clone(),
                },
                download_from: download_from.clone(),
                desired_version_hash: version_hash.clone(),
            }))
            .expect("Failed to serialize LocalDownloadRequest"),
        )
        .send()
    else {
        println!("download: failed to send request to downloads:app_store!");
        return;
    };

    println!("download: request sent, started download from {download_from}");
}
