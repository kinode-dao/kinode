use kinode_process_lib::{
    await_next_request_body, call_init, eth::get_block_number, println, Address, Request, SendError,
};

wit_bindgen::generate!({
    path: "wit",
    world: "process",
    exports: {
        world: Component,
    },
});

call_init!(init);

fn init(our: Address) {
    let Ok(_args) = await_next_request_body() else {
        println!("get_block: failed to get args, aborting");
        return;
    };

    match get_block_number() {
        Ok(block_number) => {
            println!("latest block number: {block_number}");
        }
        Err(e) => {
            println!("get_block: failed to get block number: {}", e);
        }
    }
}
