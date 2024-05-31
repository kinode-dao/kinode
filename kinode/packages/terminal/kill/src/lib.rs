use kinode_process_lib::kernel_types::{KernelCommand, KernelPrint, KernelResponse};
use kinode_process_lib::{
    await_next_message_body, call_init, println, Address, Message, ProcessId, Request,
};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v0",
});

call_init!(init);
fn init(_our: Address) {
    let Ok(args) = await_next_message_body() else {
        println!("failed to get args");
        return;
    };

    let Ok(proc_id) = String::from_utf8(args) else {
        println!("failed to stringify arguments");
        return;
    };

    let body = match proc_id.parse::<ProcessId>() {
        Ok(proc_id) => serde_json::to_vec(&KernelCommand::KillProcess(proc_id)).unwrap(),
        Err(_) => {
            println!("invalid process id");
            return;
        }
    };

    let Ok(Message::Response { body, .. }) = Request::new()
        .target(("our", "kernel", "distro", "sys"))
        .body(body)
        .send_and_await_response(60)
        .unwrap()
    else {
        println!("failed to get response from kernel");
        return;
    };
    let Ok(KernelResponse::KilledProcess(proc_id)) =
        serde_json::from_slice::<KernelResponse>(&body)
    else {
        println!("failed to parse kernel response");
        return;
    };

    println!("killed process {}", proc_id);
}
