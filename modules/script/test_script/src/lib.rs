use nectar_process_lib::{await_message, call_init, println, Address, Message, Request};

wit_bindgen::generate!({
    path: "wit",
    world: "process",
    exports: {
        world: Component,
    },
});

call_init!(init);

fn init(our: Address) {
    println!("{}: this is a dummy script!", our);

    // TODO will need to package this up into a process lib function that makes it easy
    let Ok(Message::Request { body, .. }) = await_message() else {
        println!("got send error, failing out");
        return;
    };

    println!("here are the args: {:?}", String::from_utf8(body));
}
