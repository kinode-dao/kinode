use kinode_process_lib::{
    await_message, call_init, homepage, http, println, vfs, Address, LazyLoadBlob,
};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v0",
});

const ICON: &str = include_str!("icon");

call_init!(init);
fn init(our: Address) {
    println!("begin");

    let mut server = http::server::HttpServer::new(5);
    // Serve the docs book dynamically from /docs:docs:sys/
    server
        .bind_http_path("/", http::server::HttpBindingConfig::default())
        .expect("failed to bind /");

    homepage::add_to_homepage("Docs", Some(ICON), Some("index.html"), None);

    loop {
        match await_message() {
            Err(send_error) => println!("got SendError: {send_error}"),
            Ok(ref message) => {
                // handle http requests
                // no need to validate source since capabilities limit to vfs/http_server
                let Ok(request) = server.parse_request(message.body()) else {
                    continue;
                };

                server.handle_request(
                    request,
                    |incoming| {
                        // client frontend sent an HTTP request, process it and
                        // return an HTTP response
                        // these functions can reuse the logic from handle_local_request
                        // after converting the request into the appropriate format!
                        match incoming.method().unwrap_or_default() {
                            http::Method::GET => {
                                // serve the page they requested
                                match vfs::File::new(
                                    format!(
                                        "{}/pkg/ui{}",
                                        our.package_id(),
                                        incoming.path().unwrap_or_default()
                                    ),
                                    5,
                                )
                                .read()
                                {
                                    Ok(file) => {
                                        let mime_type = format!(
                                            "text/{}",
                                            incoming
                                                .path()
                                                .unwrap_or_default()
                                                .split('.')
                                                .last()
                                                .unwrap_or("plain")
                                        );
                                        (
                                            http::server::HttpResponse::new(http::StatusCode::OK)
                                                .header("Content-Type", mime_type),
                                            Some(LazyLoadBlob::new(None::<String>, file)),
                                        )
                                    }
                                    Err(e) => (
                                        http::server::HttpResponse::new(
                                            http::StatusCode::NOT_FOUND,
                                        )
                                        .header("Content-Type", "text/html"),
                                        Some(LazyLoadBlob::new(None::<String>, e.to_string())),
                                    ),
                                }
                            }
                            _ => (
                                http::server::HttpResponse::new(
                                    http::StatusCode::METHOD_NOT_ALLOWED,
                                ),
                                None,
                            ),
                        }
                    },
                    |_channel_id, _message_type, _message| {
                        // client frontend sent a websocket message, ignore
                    },
                )
            }
        }
    }
}
