use hyperware_process_lib::{eth, script, Address};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v1",
});

script!(init);
fn init(_our: Address, args: String) -> String {
    // call get_block with the chain id provided in the args
    let chain_id = args
        .split_whitespace()
        .next()
        .unwrap_or("1")
        .parse::<u64>()
        .unwrap_or(1);

    // request timeout of 5s
    let provider = eth::Provider::new(chain_id, 5);

    match provider.get_block_number() {
        Ok(block_number) => format!("latest block number: {block_number}"),
        Err(e) => format!("failed to get block number: {e:?}"),
    }
}
