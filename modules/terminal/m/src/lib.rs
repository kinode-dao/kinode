use clap::{Arg, Command};
use kinode_process_lib::{await_next_request_body, call_init, println, Address, Request};

wit_bindgen::generate!({
    path: "../../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

call_init!(init);

fn init(_our: Address) {
    let Ok(body) = await_next_request_body() else {
        println!("m: failed to get args, aborting");
        return;
    };
    let body_string = String::from_utf8(body).unwrap();

    let mut args: Vec<&str> = body_string.split_whitespace().collect();
    args.insert(0, "m");

    println!("{:?}", args);

    let parsed = Command::new("m")
        .disable_help_flag(true)
        .arg(Arg::new("target").index(1).required(true))
        .arg(Arg::new("body").index(2).required(true))
        .arg(
            Arg::new("await")
                .short('a')
                .value_parser(clap::value_parser!(u64)),
        )
        .get_matches_from(args);

    let Some(target) = parsed.get_one::<String>("target") else {
        println!("m: no target");
        return;
    };

    let Ok(target) = target.parse::<Address>() else {
        println!("invalid address: \"{target}\"");
        return;
    };

    let Some(body) = parsed.get_one::<String>("body") else {
        println!("m: no body");
        return;
    };

    let req = Request::new().target(target).body(body.as_bytes().to_vec());

    match parsed.get_one::<u64>("await") {
        Some(s) => {
            println!("m: awaiting response for {}s", s);
            match req.send_and_await_response(*s) {
                Ok(res) => match res {
                    Ok(res) => {
                        println!("m: {:?}", res);
                    }
                    Err(e) => {
                        println!("m: SendError: {:?}", e.kind);
                    }
                },
                Err(_) => {
                    println!("m: unexpected error sending request");
                }
            }
        }
        None => {
            let _ = req.send().unwrap();
        }
    }
}
