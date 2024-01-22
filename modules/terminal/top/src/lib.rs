use kinode_process_lib::kernel_types::{KernelCommand, KernelPrint};
use kinode_process_lib::{
    await_next_request_body, call_init, println, Address, ProcessId, Request,
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
    let Ok(args) = await_next_request_body() else {
        println!("top: failed to get args, aborting");
        return;
    };

    let proc_id = String::from_utf8(args).unwrap();

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
