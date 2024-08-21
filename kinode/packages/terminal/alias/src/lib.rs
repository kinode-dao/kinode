use kinode_process_lib::{script, Address, ProcessId, Request};
use serde::{Deserialize, Serialize};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v0",
});

#[derive(Debug, Serialize, Deserialize)]
enum TerminalAction {
    EditAlias {
        alias: String,
        process: Option<ProcessId>,
    },
}

const USAGE: &str = "\x1b[1mUsage:\x1b[0m alias <alias_name> <process_id>";

script!(init);
fn init(_our: Address, args: String) -> String {
    if args.is_empty() {
        return format!("Change alias for a process.\n{USAGE}");
    }

    let (alias, process) = args.split_once(" ").unwrap_or((&args, ""));

    if alias.is_empty() {
        return format!("No alias given.\n{USAGE}");
    }

    if process.is_empty() {
        Request::to(("our", "terminal", "terminal", "sys"))
            .body(
                serde_json::to_vec(&TerminalAction::EditAlias {
                    alias: alias.to_string(),
                    process: None,
                })
                .unwrap(),
            )
            .send()
            .unwrap();
    } else {
        match process.parse::<ProcessId>() {
            Ok(process) => {
                Request::to(("our", "terminal", "terminal", "sys"))
                    .body(
                        serde_json::to_vec(&TerminalAction::EditAlias {
                            alias: alias.to_string(),
                            process: Some(process),
                        })
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
