use hyperware_process_lib::{net, script, Address, Message, Request};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v1",
});

script!(init);
fn init(_our: Address, _args: String) -> String {
    let Ok(Ok(Message::Response { body, .. })) = Request::to(("our", "net", "distro", "sys"))
        .body(rmp_serde::to_vec(&net::NetAction::GetDiagnostics).unwrap())
        .send_and_await_response(60)
    else {
        return "Failed to get diagnostics from networking module".to_string();
    };
    let Ok(net::NetResponse::Diagnostics(printout)) = rmp_serde::from_slice(&body) else {
        return "Got malformed response from networking module".to_string();
    };
    format!("\r\n{printout}")
}
