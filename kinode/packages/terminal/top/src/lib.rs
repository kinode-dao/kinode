use kinode_process_lib::kernel_types::{KernelCommand, KernelPrint};
use kinode_process_lib::{
    await_next_message_body, call_init, println, Address, ProcessId, Request,
};

wit_bindgen::generate!({
    path: "wit",
    world: "process",
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

    if proc_id.is_empty() {
        let _ = Request::new()
            .target(("our", "kernel", "distro", "sys"))
            .body(serde_json::to_vec(&KernelCommand::Debug(KernelPrint::ProcessMap)).unwrap())
            .send();
    } else {
        match proc_id.parse::<ProcessId>() {
            Ok(proc_id) => {
                let _ = Request::new()
                    .target(("our", "kernel", "distro", "sys"))
                    .body(
                        serde_json::to_vec(&KernelCommand::Debug(KernelPrint::Process(proc_id)))
                            .unwrap(),
                    )
                    .send();
            }
            Err(_) => {
                println!("invalid process id");
            }
        }
    }
}
