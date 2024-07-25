#![feature(let_chains)]
use kinode_process_lib::{
    await_message, call_init, get_blob,
    http::{
        bind_http_path, bind_http_static_path, send_response, serve_ui, HttpServerError,
        HttpServerRequest, StatusCode,
    },
    println, Address, Message,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

use crate::kinode::process::homepage::{AddRequest, Request as HomepageRequest};

/// Fetching OS version from main package.. LMK if there's a better way...
const CARGO_TOML: &str = include_str!("../../../../Cargo.toml");

#[derive(Serialize, Deserialize)]
struct HomepageApp {
    package_name: String,
    path: Option<String>,
    label: String,
    base64_icon: Option<String>,
    widget: Option<String>,
    order: Option<u16>,
}

wit_bindgen::generate!({
    path: "target/wit",
    world: "homepage-sys-v0",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize],
});

const ICON_0: &str = include_str!("./icons/bird-white.svg");
const ICON_1: &str = include_str!("./icons/bird-orange.svg");
const ICON_2: &str = include_str!("./icons/bird-plain.svg");
const ICON_3: &str = include_str!("./icons/k-orange.svg");
const ICON_4: &str = include_str!("./icons/k-plain.svg");
const ICON_5: &str = include_str!("./icons/k-white.svg");
const ICON_6: &str = include_str!("./icons/kbird-orange.svg");
const ICON_7: &str = include_str!("./icons/kbird-plain.svg");
const ICON_8: &str = include_str!("./icons/kbird-white.svg");
const ICON_9: &str = include_str!("./icons/kbranch-orange.svg");
const ICON_A: &str = include_str!("./icons/kbranch-plain.svg");
const ICON_B: &str = include_str!("./icons/kbranch-white.svg");
const ICON_C: &str = include_str!("./icons/kflower-orange.svg");
const ICON_D: &str = include_str!("./icons/kflower-plain.svg");
const ICON_E: &str = include_str!("./icons/kflower-white.svg");

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

    bind_http_path("/apps", true, false).expect("failed to bind /apps");
    bind_http_path("/version", true, false).expect("failed to bind /version");
    bind_http_path("/order", true, false).expect("failed to bind /order");
    bind_http_path("/icons/:id", true, false).expect("failed to bind /icons/:id");

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
                                package_name: message.source().package().to_string(),
                                path: path.map(|path| {
                                    format!(
                                        "/{}:{}:{}/{}",
                                        message.source().process(),
                                        message.source().package(),
                                        message.source().publisher(),
                                        path.strip_prefix('/').unwrap_or(&path)
                                    )
                                }),
                                label,
                                base64_icon: icon,
                                widget,
                                order: None,
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
                                    {
                                        let mut apps: Vec<_> = app_data.values().collect();
                                        apps.sort_by_key(|app| app.order.unwrap_or(255));
                                        serde_json::to_vec(&apps).unwrap_or_else(|_| Vec::new())
                                    },
                                );
                            }
                            "/version" => {
                                send_response(
                                    StatusCode::OK,
                                    Some(HashMap::new()),
                                    version_from_cargo_toml().as_bytes().to_vec(),
                                );
                            }
                            "/order" => {
                                // POST of a list of package names.
                                // go through the list and update each app in app_data to have the index of its name in the list as its order
                                if let Some(body) = get_blob() {
                                    let apps: Vec<String> =
                                        serde_json::from_slice(&body.bytes).unwrap();
                                    for (i, app) in apps.iter().enumerate() {
                                        if let Some(app) = app_data.get_mut(app) {
                                            app.order = Some(i as u16);
                                        }
                                    }
                                    send_response(
                                        StatusCode::OK,
                                        Some(HashMap::from([(
                                            "Content-Type".to_string(),
                                            "application/json".to_string(),
                                        )])),
                                        vec![],
                                    );
                                } else {
                                    send_response(
                                        StatusCode::BAD_REQUEST,
                                        Some(HashMap::new()),
                                        vec![],
                                    );
                                }
                            }
                            "/icons/:id" => {
                                let id = incoming
                                    .url_params()
                                    .get("id")
                                    .unwrap_or(&"0".to_string())
                                    .clone();
                                let icon = match id.to_uppercase().as_str() {
                                    "0" => ICON_0,
                                    "1" => ICON_1,
                                    "2" => ICON_2,
                                    "3" => ICON_3,
                                    "4" => ICON_4,
                                    "5" => ICON_5,
                                    "6" => ICON_6,
                                    "7" => ICON_7,
                                    "8" => ICON_8,
                                    "9" => ICON_9,
                                    "A" => ICON_A,
                                    "B" => ICON_B,
                                    "C" => ICON_C,
                                    "D" => ICON_D,
                                    "E" => ICON_E,
                                    _ => ICON_0,
                                };
                                send_response(
                                    StatusCode::OK,
                                    Some(HashMap::from([(
                                        "Content-Type".to_string(),
                                        "image/svg+xml".to_string(),
                                    )])),
                                    icon.as_bytes().to_vec(),
                                );
                            }
                            _ => {
                                send_response(
                                    StatusCode::OK,
                                    Some(HashMap::from([(
                                        "Content-Type".to_string(),
                                        "text/plain".to_string(),
                                    )])),
                                    "yes hello".as_bytes().to_vec(),
                                );
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
