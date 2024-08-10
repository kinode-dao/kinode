use kinode_process_lib::{
    await_message, call_init,
    homepage::add_to_homepage,
    http::server::{HttpBindingConfig, HttpServer},
    println, Address,
};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v0",
});

const ICON: &str = include_str!("icon");

call_init!(init);
fn init(our: Address) {
    println!("begin");

    let mut server = HttpServer::new(5);
    server
        .serve_ui(&our, "ui", vec!["/"], HttpBindingConfig::default())
        .unwrap();

    add_to_homepage("Docs", Some(ICON), Some("index.html"), None);

    loop {
        match await_message() {
            Err(send_error) => println!("got SendError: {send_error}"),
            Ok(ref _message) => println!("got message"),
        }
    }
}
