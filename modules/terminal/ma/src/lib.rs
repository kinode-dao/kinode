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
    let Ok(args) = await_next_request_body() else {
        println!("m: failed to get args, aborting");
        return;
    };

    let tail = String::from_utf8(args).unwrap();

    let (await_time, tail) = match tail.split_once(" ") {
        Some((a, p)) => (a, p),
        None => {
            println!(
                "m: invalid command, please provide an await time, adddress, and json message"
            );
            return;
        }
    };

    let Ok(await_time) = await_time.parse::<u64>() else {
        println!("m: invalid await time: \"{await_time}\"");
        return;
    };

    let (target, body) = match tail.split_once(" ") {
        Some((a, p)) => (a, p),
        None => {
            println!(
                "m: invalid command, please provide an await time, adddress, and json message"
            );
            return;
        }
    };
    // TODO aliasing logic...maybe we can read from terminal state since we have root?
    let target = match target.parse::<Address>() {
        Ok(t) => t,
        Err(_) => {
            println!("invalid address: \"{target}\"");
            return;
        } // match state.aliases.get(target) {
          //     Some(pid) => Address::new("our", pid.clone()),
          //     None => {
          //         return Err(anyhow!("invalid address: \"{target}\""));
          //     }
          // },
    };
    match Request::new()
        .target(target)
        .body(body)
        .send_and_await_response(await_time)
    {
        Ok(res) => match res {
            Ok(res) => {
                println!("ma: response: {:?}", res);
            }
            Err(e) => {
                println!("ma: SendError: {:?}", e.kind);
            }
        },
        Err(_) => {
            println!("ma: unexpected send error");
        }
    };
}
