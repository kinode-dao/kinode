use kinode_process_lib::{
    await_next_message_body, call_init, get_blob, println, vfs, Address, Request,
};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process",
});

call_init!(init);
fn init(_our: Address) {
    let Ok(args) = await_next_message_body() else {
        println!("failed to get args");
        return;
    };

    let Ok(file_path) = String::from_utf8(args) else {
        println!("argument must be a single string");
        return;
    };

    if file_path.is_empty() {
        println!("Print the contents of a file to the terminal");
        println!("\x1b[1mUsage:\x1b[0m cat <file_path>");
        return;
    }

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
    println!("{}", String::from_utf8(blob.bytes).unwrap());
}
