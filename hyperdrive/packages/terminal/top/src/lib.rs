use clap::{Arg, Command};
use hyperware_process_lib::kernel_types::{
    KernelCommand, KernelPrint, KernelPrintResponse, KernelResponse, PersistedProcess,
};
use hyperware_process_lib::{script, Address, Message, ProcessId, Request};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v1",
});

const USAGE: &str = "\x1b[1mUsage:\x1b[0m
    \ntop [-c <show-caps>] <- to view all processes
    \ntop <process_id> [-c <show-caps>] <- to view one process";

script!(init);
fn init(_our: Address, args: String) -> String {
    let body_string = format!("top {args}");

    let Ok(parsed) = Command::new("top")
        .disable_help_flag(true)
        .arg(Arg::new("target").index(1))
        .arg(
            Arg::new("show-caps")
                .short('c')
                .long("show-caps")
                .action(clap::ArgAction::SetTrue),
        )
        .try_get_matches_from(body_string.split_whitespace())
    else {
        return format!("Failed to parse args.\n{USAGE}");
    };

    let target = parsed
        .get_one::<String>("target")
        .map(|s| s.parse::<ProcessId>());
    let show_caps = parsed.get_flag("show-caps");

    let Ok(Message::Response { body, .. }) = Request::to(("our", "kernel", "distro", "sys"))
        .body(if let Some(target) = &target {
            match target {
                Ok(proc_id) => {
                    serde_json::to_vec(&KernelCommand::Debug(KernelPrint::Process(proc_id.clone())))
                        .unwrap()
                }
                Err(e) => return format!("invalid process id: {e}\n{USAGE}"),
            }
        } else {
            serde_json::to_vec(&KernelCommand::Debug(KernelPrint::ProcessMap)).unwrap()
        })
        .send_and_await_response(60)
        .unwrap()
    else {
        return "Failed to get response from kernel".to_string();
    };
    let Ok(KernelResponse::Debug(kernel_print_response)) =
        serde_json::from_slice::<KernelResponse>(&body)
    else {
        return "Failed to parse kernel response".to_string();
    };

    match kernel_print_response {
        KernelPrintResponse::ProcessMap(process_map) => {
            let len = process_map.len();
            let printout = process_map
                .iter()
                .map(|(proc_id, process)| print_process(proc_id, process, show_caps))
                .collect::<Vec<_>>()
                .join("\r\n");
            format!("\r\n{printout}\r\n\r\ntop: {len} running processes")
        }
        KernelPrintResponse::Process(process) => match process {
            None => {
                format!(
                    "process {} not running",
                    target.map_or("(all)".to_string(), |t| t
                        .map(|t| t.to_string())
                        .unwrap_or_default())
                )
            }
            Some(process) => {
                format!(
                    "{}",
                    print_process(&target.unwrap().unwrap(), &process, show_caps)
                )
            }
        },
        KernelPrintResponse::HasCap(_) => {
            format!("kernel gave wrong kind of response")
        }
    }
}

fn print_process(id: &ProcessId, process: &PersistedProcess, show_caps: bool) -> String {
    format!(
        "{}:\r\n    {}\r\n    wit: {}\r\n    on-exit: {:?}\r\n    public: {}\r\n    capabilities:\r\n        {}",
        id,
        if process.wasm_bytes_handle.is_empty() {
            "(runtime)"
        } else {
            &process.wasm_bytes_handle
        },
        process.wit_version.unwrap_or_default(),
        process.on_exit,
        process.public,
        if show_caps {
            process
                .capabilities
                .iter()
                .map(|c| format!("{}\r\n        ", c.to_string()))
                .collect::<String>()
        } else {
            format!("{}, use -c to display", process.capabilities.len())
        }
    )
}
