use kinode_process_lib::{await_next_request_body, call_init, println, Address};

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
        println!("failed to get args, aborting");
        return;
    };

    match String::from_utf8(args.clone()) {
        Ok(s) => println!("{}", s),
        Err(_) => println!("{:?}", args),
    }
}
