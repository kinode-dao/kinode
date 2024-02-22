use kinode_process_lib::{
    await_next_request_body, call_init, eth::get_block_number, println, Address,
};

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
        println!("get_block: failed to get args, aborting");
        return;
    };

    // incoming args bytes are a string we parse to u64, if none provided, default to 1
    let chain_id = std::str::from_utf8(&args)
        .unwrap_or("1")
        .parse::<u64>()
        .unwrap_or(1);

    match get_block_number(chain_id) {
        Ok(block_number) => {
            println!("latest block number: {block_number}");
        }
        Err(e) => {
            println!("get_block: failed to get block number: {}", e);
        }
    }
}
