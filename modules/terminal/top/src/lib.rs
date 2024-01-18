use kinode_process_lib::kernel_types::{KernelCommand, KernelPrint};
use kinode_process_lib::{await_message, call_init, println, Address, Message, ProcessId, Request};

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

    let proc_id = String::from_utf8(body).unwrap();

    let kernel_addr = Address::new("our", ("kernel", "distro", "sys"));
    let _ = Request::new()
        .target(kernel_addr)
        .body(
            serde_json::to_vec(&match proc_id.parse::<ProcessId>() {
                Ok(proc_id) => KernelCommand::Debug(KernelPrint::Process(proc_id)),
                Err(_) => KernelCommand::Debug(KernelPrint::ProcessMap),
            })
            .unwrap(),
        )
        .send();
}
