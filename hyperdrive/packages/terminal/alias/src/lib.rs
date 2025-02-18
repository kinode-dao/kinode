use crate::hyperware::process::terminal::{EditAliasRequest, Request as TerminalRequest};
use hyperware_process_lib::{script, Address, ProcessId, Request};

wit_bindgen::generate!({
    path: "target/wit",
    world: "terminal-sys-v0",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize],
});

const USAGE: &str = "\x1b[1mUsage:\x1b[0m alias <alias_name> <process_id>";

script!(init);
fn init(_our: Address, args: String) -> String {
    if args.is_empty() {
        return format!("Change alias for a process.\n{USAGE}");
    }

    let (alias, process_str) = args.split_once(" ").unwrap_or((&args, ""));

    if alias.is_empty() {
        return format!("No alias given.\n{USAGE}");
    }

    if process_str.is_empty() {
        Request::to(("our", "terminal", "terminal", "sys"))
            .body(
                serde_json::to_vec(&TerminalRequest::EditAlias(EditAliasRequest {
                    alias: alias.to_string(),
                    process: None,
                }))
                .unwrap(),
            )
            .send()
            .unwrap();
    } else {
        match process_str.parse::<ProcessId>() {
            Ok(_parsed_process) => {
                Request::to(("our", "terminal", "terminal", "sys"))
                    .body(
                        serde_json::to_vec(&TerminalRequest::EditAlias(EditAliasRequest {
                            alias: alias.to_string(),
                            process: Some(process_str.to_string()),
                        }))
                        .unwrap(),
                    )
                    .send()
                    .unwrap();
            }
            Err(_) => {
                return format!("Invalid process ID.\n{USAGE}");
            }
        }
    }
    "alias set".to_string()
}
