#![feature(let_chains)]
use kinode_process_lib::{
    await_message, call_init, http::bind_http_static_path, http::HttpServerError, println, Address,
    Message, ProcessId,
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

wit_bindgen::generate!({
    path: "wit",
    world: "process",
    exports: {
        world: Component,
    },
});

const HOME_PAGE: &str = include_str!("index.html");

const APP_TEMPLATE: &str = r#"
<a class="app-link" id="${package_name}" href="/${path}">
  <img
    src="${base64_icon}" />
  <h6>${label}</h6>
</a>"#;

call_init!(main);

/// bind to root path on http_server (we have special dispensation to do so!)
fn bind_index(our: &str, apps: &HashMap<ProcessId, String>) {
    bind_http_static_path(
        "/",
        true,
        false,
        Some("text/html".to_string()),
        HOME_PAGE
            .replace("${our}", our)
            .replace(
                "${apps}",
                &apps
                    .values()
                    .map(String::as_str)
                    .collect::<Vec<&str>>()
                    .join("\n"),
            )
            .to_string()
            .as_bytes()
            .to_vec(),
    )
    .expect("failed to bind to /");
}

fn main(our: Address) {
    let mut apps: HashMap<ProcessId, String> = HashMap::new();

    bind_index(&our.node, &apps);

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
                        apps.insert(
                            message.source().process.clone(),
                            APP_TEMPLATE
                                .replace(
                                    "${package_name}",
                                    &format!(
                                        "{}:{}",
                                        message.source().package(),
                                        message.source().publisher()
                                    ),
                                )
                                .replace(
                                    "${path}",
                                    &format!(
                                        "{}/{}",
                                        message.source().process,
                                        path.strip_prefix('/').unwrap_or(&path)
                                    ),
                                )
                                .replace("${label}", &label)
                                .replace("${base64_icon}", &icon),
                        );
                        bind_index(&our.node, &apps);
                    }
                    HomepageRequest::Remove => {
                        apps.remove(&message.source().process);
                        bind_index(&our.node, &apps);
                    }
                }
            }
        }
    }
}
