use kinode_process_lib::{
    await_next_request_body, call_init, println, Address, ProcessId, Request,
};
use serde::{Deserialize, Serialize};

wit_bindgen::generate!({
    path: "wit",
    world: "process",
    exports: {
        world: Component,
    },
});

#[derive(Debug, Serialize, Deserialize)]
enum TerminalAction {
    EditAlias {
        alias: String,
        process: Option<ProcessId>,
    },
}

call_init!(init);

fn init(_our: Address) {
    let Ok(args) = await_next_request_body() else {
        println!("failed to get args, aborting");
        return;
    };

    let line = String::from_utf8(args).unwrap_or("error".into());
    if line.is_empty() {
        println!("Change alias for a process");
        println!("\x1b[1mUsage:\x1b[0m alias <alias_name> <process_id>");
        return;
    }

    let (alias, process) = line.split_once(" ").unwrap_or((&line, ""));

    if alias.is_empty() {
        println!("no alias given");
        return;
    }

    if process.is_empty() {
        let _ = Request::new()
            .target(("our", "terminal", "terminal", "sys"))
            .body(
                serde_json::to_vec(&TerminalAction::EditAlias {
                    alias: alias.to_string(),
                    process: None,
                })
                .unwrap(),
            )
            .send();
    } else {
        match process.parse::<ProcessId>() {
            Ok(process) => {
                let _ = Request::new()
                    .target(("our", "terminal", "terminal", "sys"))
                    .body(
                        serde_json::to_vec(&TerminalAction::EditAlias {
                            alias: alias.to_string(),
                            process: Some(process),
                        })
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
