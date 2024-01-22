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
    Download {
        package: PackageId,
        install_from: NodeId,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum LocalResponse {
    DownloadResponse(DownloadResponse),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum DownloadResponse {
    Started,
    Failure,
}

call_init!(init);

fn init(our: Address) {
    let Ok(body) = await_next_request_body() else {
        println!("download: failed to get args!");
        return;
    };

    let args = String::from_utf8(body).unwrap_or_default();

    let Some((arg1, arg2)) = args.split_once(" ") else {
        println!("download: 2 arguments required, the node id to download from and the package id of the app");
        println!("example: download my-friend.os app:publisher.os");
        return;
    };

    let download_from: NodeId = arg1.to_string();

    let Ok(package_id) = arg2.parse::<PackageId>() else {
        println!("download: invalid package id, make sure to include package name and publisher");
        println!("example: app_name:publisher_name");
        return;
    };

    let Ok(Ok(Message::Response { body, .. })) =
        Request::to((our.node(), ("main", "app_store", "sys")))
            .body(
                serde_json::to_vec(&LocalRequest::Download {
                    package: package_id.clone(),
                    install_from: download_from.clone(),
                })
                .unwrap(),
            )
            .send_and_await_response(5)
    else {
        println!("download: failed to get a response from app_store..!");
        return;
    };

    let Ok(response) = serde_json::from_slice::<LocalResponse>(&body) else {
        println!("download: failed to parse response from app_store..!");
        return;
    };

    match response {
        LocalResponse::DownloadResponse(DownloadResponse::Started) => {
            println!("started downloading package {package_id} from {download_from}");
        }
        LocalResponse::DownloadResponse(DownloadResponse::Failure) => {
            println!("failed to download package {package_id} from {download_from}");
        }
    }
}
