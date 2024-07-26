#![feature(let_chains)]
use crate::kinode::process::homepage::{AddRequest, Request as HomepageRequest};
use kinode_process_lib::{
    await_message, call_init, get_blob,
    http::{
        bind_http_path, bind_http_static_path, send_response, serve_ui, HttpServerError,
        HttpServerRequest, Method, StatusCode,
    },
    println, Address, Message,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Fetching OS version from main package.. LMK if there's a better way...
const CARGO_TOML: &str = include_str!("../../../../Cargo.toml");

const DEFAULT_FAVES: &[&str] = &[
    "chess:chess:sys",
    "main:app_store:sys",
    "settings:settings:sys",
];

#[derive(Serialize, Deserialize)]
struct HomepageApp {
    id: String,
    process: String,
    package: String,
    publisher: String,
    path: Option<String>,
    label: String,
    base64_icon: Option<String>,
    widget: Option<String>,
    order: Option<u32>,
    favorite: bool,
}

wit_bindgen::generate!({
    path: "target/wit",
    world: "homepage-sys-v0",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize],
});

call_init!(init);
fn init(our: Address) {
    let mut app_data: BTreeMap<String, HomepageApp> = BTreeMap::new();

    serve_ui(&our, "ui", true, false, vec!["/"]).expect("failed to serve ui");

    bind_http_static_path(
        "/our",
        false,
        false,
        Some("text/html".to_string()),
        our.node().into(),
    )
    .expect("failed to bind to /our");

    bind_http_static_path(
        "/amionline",
        false,
        false,
        Some("text/html".to_string()),
        "yes".into(),
    )
    .expect("failed to bind to /amionline");

    bind_http_static_path(
        "/our.js",
        false,
        false,
        Some("application/javascript".to_string()),
        format!("window.our = {{}}; window.our.node = '{}';", &our.node).into(),
    )
    .expect("failed to bind to /our.js");

    bind_http_static_path(
        "/kinode.css",
        true,
        false,
        Some("text/css".to_string()),
        include_str!("../../pkg/kinode.css").into(),
    )
    .expect("failed to bind /kinode.css");

    bind_http_static_path(
        "/kinode.svg",
        true,
        false,
        Some("image/svg+xml".to_string()),
        include_str!("../../pkg/kinode.svg").into(),
    )
    .expect("failed to bind /kinode.svg");

    bind_http_static_path(
        "/bird-orange.svg",
        true,
        false,
        Some("image/svg+xml".to_string()),
        include_str!("../../pkg/bird-orange.svg").into(),
    )
    .expect("failed to bind /bird-orange.svg");

    bind_http_static_path(
        "/bird-plain.svg",
        true,
        false,
        Some("image/svg+xml".to_string()),
        include_str!("../../pkg/bird-plain.svg").into(),
    )
    .expect("failed to bind /bird-plain.svg");

    bind_http_static_path(
        "/version",
        true,
        false,
        Some("text/plain".to_string()),
        version_from_cargo_toml().into(),
    )
    .expect("failed to bind /version");

    bind_http_path("/apps", true, false).expect("failed to bind /apps");
    bind_http_path("/favorite", true, false).expect("failed to bind /favorite");

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
                    HomepageRequest::Add(AddRequest {
                        label,
                        icon,
                        path,
                        widget,
                    }) => {
                        app_data.insert(
                            message.source().process.to_string(),
                            HomepageApp {
                                id: message.source().process.to_string(),
                                process: message.source().process().to_string(),
                                package: message.source().package().to_string(),
                                publisher: message.source().publisher().to_string(),
                                path: path.map(|path| {
                                    format!(
                                        "/{}/{}",
                                        message.source().process,
                                        path.strip_prefix('/').unwrap_or(&path)
                                    )
                                }),
                                label,
                                base64_icon: icon,
                                widget,
                                order: None,
                                favorite: DEFAULT_FAVES
                                    .contains(&message.source().process.to_string().as_str()),
                            },
                        );
                    }
                    HomepageRequest::Remove => {
                        app_data.remove(&message.source().process.to_string());
                    }
                }
            } else if let Ok(req) = serde_json::from_slice::<HttpServerRequest>(message.body()) {
                match req {
                    HttpServerRequest::Http(incoming) => {
                        let path = incoming.bound_path(None);
                        match path {
                            "/apps" => {
                                send_response(
                                    StatusCode::OK,
                                    Some(HashMap::from([(
                                        "Content-Type".to_string(),
                                        "application/json".to_string(),
                                    )])),
                                    serde_json::to_vec(
                                        &app_data.values().collect::<Vec<&HomepageApp>>(),
                                    )
                                    .unwrap(),
                                );
                            }
                            "/favorite" => {
                                let Ok(Method::POST) = incoming.method() else {
                                    send_response(
                                        StatusCode::BAD_REQUEST,
                                        Some(HashMap::new()),
                                        vec![],
                                    );
                                    return;
                                };
                                // POST of a list of package names.
                                // go through the list and update each app in app_data to have the index of its name in the list as its order
                                let Some(body) = get_blob() else {
                                    send_response(
                                        StatusCode::BAD_REQUEST,
                                        Some(HashMap::new()),
                                        vec![],
                                    );
                                    return;
                                };
                                let Ok(favorite_toggle) =
                                    serde_json::from_slice::<(String, u32, bool)>(&body.bytes)
                                else {
                                    send_response(
                                        StatusCode::BAD_REQUEST,
                                        Some(HashMap::new()),
                                        vec![],
                                    );
                                    return;
                                };
                                if let Some(app) = app_data.get_mut(&favorite_toggle.0) {
                                    app.order = Some(favorite_toggle.1);
                                    app.favorite = favorite_toggle.2;
                                }
                                send_response(
                                    StatusCode::OK,
                                    Some(HashMap::from([(
                                        "Content-Type".to_string(),
                                        "application/json".to_string(),
                                    )])),
                                    vec![],
                                );
                            }
                            _ => {
                                send_response(StatusCode::NOT_FOUND, Some(HashMap::new()), vec![]);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn version_from_cargo_toml() -> String {
    let version = CARGO_TOML
        .lines()
        .find(|line| line.starts_with("version = "))
        .expect("Failed to find version in Cargo.toml");

    version
        .split('=')
        .last()
        .expect("Failed to parse version from Cargo.toml")
        .trim()
        .trim_matches('"')
        .to_string()
}
