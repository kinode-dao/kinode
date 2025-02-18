use clap::{Arg, Command};
use hyperware_process_lib::{println, script, Address, Request, SendErrorKind};
use regex::Regex;

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v1",
});

const USAGE: &str = "\x1b[1mUsage:\x1b[0m m <target> <body> [-a <await_time>]";

script!(init);
fn init(our: Address, args: String) -> String {
    if args.is_empty() {
        return format!("Send a request to a process.\n{USAGE}");
    }

    let mut args: Vec<String> = Regex::new(r#"'[^']*'|\S+"#)
        .unwrap()
        .find_iter(&args)
        .map(|mat| {
            let match_str = mat.as_str();
            // Remove the surrounding single quotes for the JSON string
            if match_str.starts_with('\'') && match_str.ends_with('\'') {
                match_str[1..match_str.len() - 1].to_string()
            } else {
                match_str.to_string()
            }
        })
        .collect();

    args.insert(0, "m".to_string());

    let Ok(parsed) = Command::new("m")
        .disable_help_flag(true)
        .arg(Arg::new("target").index(1).required(true))
        .arg(Arg::new("body").index(2).required(true))
        .arg(
            Arg::new("await")
                .short('a')
                .long("await")
                .value_parser(clap::value_parser!(u64)),
        )
        .try_get_matches_from(args)
    else {
        return format!("Failed to parse args.\n{USAGE}");
    };

    let Some(target) = parsed.get_one::<String>("target") else {
        return format!("No target given.\n{USAGE}");
    };

    let Ok(target) = target.parse::<Address>() else {
        return format!("Invalid address: \"{target}\"\n{USAGE}");
    };

    let Some(body) = parsed.get_one::<String>("body") else {
        return format!("No body given.\n{USAGE}");
    };

    let target = if target.node() != "our" {
        target
    } else {
        Address::new(our.node(), target.process)
    };

    let req = Request::to(&target)
        .body(body.as_bytes().to_vec())
        .try_attach_all()
        .unwrap();

    match parsed.get_one::<u64>("await") {
        Some(s) => {
            println!("Awaiting response for {s}s");
            match req.send_and_await_response(*s).unwrap() {
                Ok(res) => String::from_utf8_lossy(res.body()).to_string(),
                Err(e) => {
                    format!(
                        "{}",
                        match e.kind {
                            SendErrorKind::Timeout =>
                                "Target did not send response in time, try increasing the await time",
                            SendErrorKind::Offline =>
                                "Failed to send message because the target is offline",
                        }
                    )
                }
            }
        }
        None => {
            // still wait for a response, but don't do anything with it
            // do this so caps checks don't fail
            let _ = req.send_and_await_response(5).unwrap();
            "".to_string()
        }
    }
}
