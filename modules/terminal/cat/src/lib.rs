use kinode_process_lib::{
    await_next_request_body, call_init, get_blob, println, vfs, Address, Request, Response,
};

wit_bindgen::generate!({
    path: "../../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

call_init!(init);

fn init(_our: Address) {
    let Ok(args) = await_next_request_body() else {
        println!("cat: failed to get args, aborting");
        return;
    };

    let Ok(file_path) = String::from_utf8(args) else {
        println!("bad file path");
        return;
    };

    Request::new()
        .target(("our", "vfs", "distro", "sys"))
        .body(
            serde_json::to_vec(&vfs::VfsRequest {
                path: file_path.clone(),
                action: vfs::VfsAction::Read,
            })
            .unwrap(),
        )
        .send_and_await_response(5)
        .unwrap()
        .unwrap();
    let Some(blob) = get_blob() else {
        println!("no file found at {}", file_path);
        return;
    };
    let _ = Response::new().body(blob.bytes).send();
}
