use kinode_process_lib::{
    await_message, call_init, get_blob, println, vfs, Address, Message, Request,
};

wit_bindgen::generate!({
    path: "wit",
    world: "process",
    exports: {
        world: Component,
    },
});

call_init!(init);

fn init(_our: Address) {
    // TODO will need to package this up into a process lib function that makes it easy
    let Ok(Message::Request { body, .. }) = await_message() else {
        println!("got send error, failing out");
        return;
    };

    let Ok(file_path) = String::from_utf8(body) else {
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
    println!(
        "{}",
        String::from_utf8(blob.bytes).unwrap_or("could not stringify file".to_string())
    );
}
