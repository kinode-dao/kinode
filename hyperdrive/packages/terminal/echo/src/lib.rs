use hyperware_process_lib::{script, Address};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v1",
});

script!(init);
fn init(_our: Address, args: String) -> String {
    args
}
