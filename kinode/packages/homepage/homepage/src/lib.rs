#![feature(let_chains)]
use kinode_process_lib::{
    await_message, call_init,
    http::{
        bind_http_path, bind_http_static_path, send_response, serve_ui, HttpServerError,
        HttpServerRequest, StatusCode,
    },
    println, Address, Message, ProcessId,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The request format to add or remove an app from the homepage. You must have messaging
/// access to `homepage:homepage:sys` in order to perform this. Serialize using serde_json.
#[derive(Serialize, Deserialize)]
enum HomepageRequest {
    /// the package and process name will come from request source.
    /// the path will automatically have the process_id prepended.
    /// the icon is a base64 encoded image.
    Add {
        label: String,
        icon: String,
        path: String,
    },
    Remove,
}

#[derive(Serialize, Deserialize)]
struct HomepageApp {
    package_name: String,
    path: String,
    label: String,
    base64_icon: String,
}

wit_bindgen::generate!({
    path: "wit",
    world: "process",
});

call_init!(init);
fn init(our: Address) {
    let mut app_data: HashMap<ProcessId, HomepageApp> = HashMap::new();

    serve_ui(&our, "ui", true, false, vec!["/"]).expect("failed to serve ui");

    bind_http_static_path(
        "/our",
        false,
        false,
        Some("text/html".to_string()),
        our.node.clone().as_bytes().to_vec(),
    )
    .expect("failed to bind to /our");

    bind_http_static_path(
        "/our.js",
        false,
        false,
        Some("application/javascript".to_string()),
        format!("window.our = {{}}; window.our.node = '{}';", &our.node)
            .as_bytes()
            .to_vec(),
    )
    .expect("failed to bind to /our.js");

    bind_http_path("/apps", true, true).expect("failed to bind /apps");

    loop {
        let Ok(ref message) = await_message() else {
            // we never send requests, so this will never happen
            continue;
        };
        if let Message::Response { source, body, .. } = message
            && source.process == "http_server:distro:sys"
        {
            match serde_json::from_slice::<Result<(), HttpServerError>>(&body) {
                Ok(Ok(())) => continue,
                Ok(Err(e)) => println!("got error from http_server: {e}"),
                Err(_e) => println!("got malformed message from http_server!"),
            }
        } else {
            // handle messages to add or remove an app from the homepage.
            // they must have messaging access to us in order to perform this.
            if let Ok(request) = serde_json::from_slice::<HomepageRequest>(message.body()) {
                match request {
                    HomepageRequest::Add { label, icon, path } => {
                        println!("adding app {label} {path}");
                        app_data.insert(
                            message.source().process.clone(),
                            HomepageApp {
                                package_name: message.source().clone().package().to_string(),
                                path: format!(
                                    "/{}:{}:{}/{}",
                                    message.source().clone().process().to_string(),
                                    message.source().clone().package().to_string(),
                                    message.source().clone().publisher().to_string(),
                                    path.strip_prefix('/').unwrap_or(&path)
                                ),
                                label: label.clone(),
                                base64_icon: icon.clone(),
                            },
                        );
                    }
                    HomepageRequest::Remove => {
                        app_data.remove(&message.source().process);
                    }
                }
            } else if let Ok(request) = serde_json::from_slice::<HttpServerRequest>(message.body())
            {
                match request {
                    HttpServerRequest::Http(incoming) => {
                        let path = incoming.bound_path(None);
                        println!("on path: {}", path);
                        if path == "/apps" {
                            send_response(
                                StatusCode::OK,
                                Some(HashMap::from([(
                                    "Content-Type".to_string(),
                                    "application/json".to_string(),
                                )])),
                                format!(
                                    "[{}]",
                                    app_data
                                        .values()
                                        .map(|app| serde_json::to_string(app).unwrap())
                                        .collect::<Vec<String>>()
                                        .join(",")
                                )
                                .as_bytes()
                                .to_vec(),
                            );
                        } else {
                            send_response(
                                StatusCode::OK,
                                Some(HashMap::new()),
                                "yes hello".as_bytes().to_vec(),
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
