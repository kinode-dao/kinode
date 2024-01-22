use kinode_process_lib::{await_next_request_body, call_init, println, Address, Request};

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
        println!("m: failed to get args, aborting");
        return;
    };

    let tail = String::from_utf8(args).unwrap();

    let (target, body) = match tail.split_once(" ") {
        Some((a, p)) => (a, p),
        None => {
            println!("invalid command: \"{tail}\"");
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
    let _ = Request::new().target(target).body(body).send();
}
