use kinode_process_lib::{await_message, call_init, println, Address, Message, Request};

wit_bindgen::generate!({
    path: "wit",
    world: "process",
    exports: {
        world: Component,
    },
});

call_init!(init);

fn init(_our: Address) {
    // TODO will need to package this up into a process lib function that makes it easy
    let Ok(Message::Request { body, .. }) = await_message() else {
        println!("got send error, failing out");
        return;
    };

    let tail = String::from_utf8(body).unwrap();

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
        Err(e) => {
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
