use serde::{Deserialize, Serialize};
use std::str::FromStr;

use nectar_process_lib::{
    await_message, call_init, println, Address, Message, ProcessId, Request, Response,
};

wit_bindgen::generate!({
    path: "wit",
    world: "process",
    exports: {
        world: Component,
    },
});

#[derive(Debug, Serialize, Deserialize)]
pub enum ScriptRequest {
    Run {
        path: String, // vfs path
        args: String, // first message, in json
    },
    Inject {
        process: String, // ProcessId
        args: String,    // next message, in json
    },
    Terminate(String), // ProcessId string encoded
}

call_init!(init);
fn init(our: Address) {
    println!("script_runner: begin");

    loop {
        match handle_message(&our) {
            Ok(()) => {}
            Err(e) => {
                println!("script_runner: error: {:?}", e);
            }
        };
    }
}

fn handle_message(our: &Address) -> anyhow::Result<()> {
    let message = await_message()?;

    match message {
        Message::Response { .. } => {
            return Err(anyhow::anyhow!("unexpected Response: {:?}", message));
        }
        Message::Request {
            ref source,
            ref body,
            ..
        } => match serde_json::from_slice::<ScriptRequest>(body)? {
            ScriptRequest::Run { path, args } => {
                println!("script_runner: got run request");
            }
            ScriptRequest::Inject { process, args } => {
                println!("script_runner: got inject request");
            }
            ScriptRequest::Terminate(process) => {
                println!("script_runner: got terminate request");
            }
        },
    }
    Ok(())
}
