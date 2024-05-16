use kinode_process_lib::{await_next_message_body, call_init, eth, println, Address};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process",
});

call_init!(init);
fn init(_our: Address) {
    let Ok(args) = await_next_message_body() else {
        println!("failed to get args");
        return;
    };

    // incoming args bytes are a string we parse to u64, if none provided, default to 1
    let chain_id = std::str::from_utf8(&args)
        .unwrap_or("1")
        .parse::<u64>()
        .unwrap_or(1);

    // request timeout of 5s
    let provider = eth::Provider::new(chain_id, 5);

    match provider.get_block_number() {
        Ok(block_number) => {
            println!("latest block number: {block_number}");
        }
        Err(e) => {
            println!("failed to get block number: {e:?}");
        }
    }
}
