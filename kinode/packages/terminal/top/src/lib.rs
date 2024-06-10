use kinode_process_lib::kernel_types::{
    KernelCommand, KernelPrint, KernelPrintResponse, KernelResponse, PersistedProcess,
};
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

    let Ok(Message::Response { body, .. }) = Request::new()
        .target(("our", "kernel", "distro", "sys"))
        .body(if proc_id.is_empty() {
            serde_json::to_vec(&KernelCommand::Debug(KernelPrint::ProcessMap)).unwrap()
        } else {
            match proc_id.parse::<ProcessId>() {
                Ok(proc_id) => {
                    serde_json::to_vec(&KernelCommand::Debug(KernelPrint::Process(proc_id)))
                        .unwrap()
                }
                Err(_) => {
                    println!("invalid process id");
                    return;
                }
            }
        })
        .send_and_await_response(60)
        .unwrap()
    else {
        println!("failed to get response from kernel");
        return;
    };
    let Ok(KernelResponse::Debug(kernel_print_response)) =
        serde_json::from_slice::<KernelResponse>(&body)
    else {
        println!("failed to parse kernel response");
        return;
    };

    match kernel_print_response {
        KernelPrintResponse::ProcessMap(process_map) => {
            let len = process_map.len();
            let printout = process_map
                .iter()
                .map(|(proc_id, process)| print_process(proc_id, process))
                .collect::<Vec<_>>()
                .join("\r\n");
            println!("\r\n{printout}\r\n\r\ntop: {len} running processes");
        }
        KernelPrintResponse::Process(process) => match process {
            None => {
                println!("process {} not running", proc_id);
                return;
            }
            Some(process) => {
                println!("{}", print_process(&proc_id.parse().unwrap(), &process));
            }
        },
        KernelPrintResponse::HasCap(_) => {
            println!("kernel gave wrong kind of response");
        }
    }
}

fn print_process(id: &ProcessId, process: &PersistedProcess) -> String {
    format!(
        "{}:\r\n    {}\r\n    wit: {}\r\n    on-exit: {:?}\r\n    public: {}\r\n    capabilities: {:?}",
        id,
        if process.wasm_bytes_handle.is_empty() {
            "(runtime)"
        } else {
            &process.wasm_bytes_handle
        },
        process.wit_version.unwrap_or_default(),
        process.on_exit,
        process.public,
        process
            .capabilities
            .iter()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
    )
}
