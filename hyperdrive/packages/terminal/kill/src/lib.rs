use hyperware_process_lib::kernel_types::{KernelCommand, KernelResponse};
use hyperware_process_lib::{script, Address, Message, ProcessId, Request};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v1",
});

script!(init);
fn init(_our: Address, args: String) -> String {
    let process_id = match args.parse::<ProcessId>() {
        Ok(id) => id,
        Err(_) => {
            return "Invalid process ID.\n\x1b[1mUsage:\x1b[0m kill <process_id>".to_string();
        }
    };

    let Ok(Message::Response { body, .. }) = Request::to(("our", "kernel", "distro", "sys"))
        .body(serde_json::to_vec(&KernelCommand::KillProcess(process_id)).unwrap())
        .send_and_await_response(60)
        .unwrap()
    else {
        return "failed to get response from kernel".to_string();
    };
    let Ok(KernelResponse::KilledProcess(proc_id)) =
        serde_json::from_slice::<KernelResponse>(&body)
    else {
        return "failed to parse kernel response".to_string();
    };

    format!("killed process {proc_id}")
}
