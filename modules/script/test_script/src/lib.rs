use nectar_process_lib::{await_message, call_init, println, Address, Request};

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
}
