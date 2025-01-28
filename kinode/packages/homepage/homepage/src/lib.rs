use crate::kinode::process::homepage;
use kinode_process_lib::{
    await_message, call_init, get_blob,
    http::{self, server},
    println, Address, Capability, LazyLoadBlob, Response,
};
use std::collections::{BTreeMap, HashMap};

/// Fetching OS version from main package
const CARGO_TOML: &str = include_str!("../../../../Cargo.toml");

const DEFAULT_FAVES: &[&str] = &[
    "chess:chess:sys",
    "main:app-store:sys",
    "settings:settings:sys",
];

type PersistedAppOrder = HashMap<String, u32>;

wit_bindgen::generate!({
    path: "target/wit",
    world: "homepage-sys-v1",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize],
});

call_init!(init);
fn init(our: Address) {
    println!("started");

    let mut app_data: BTreeMap<String, homepage::App> = BTreeMap::new();

    let mut http_server = server::HttpServer::new(5);
    let http_config = server::HttpBindingConfig::default();

    http_server
        .serve_ui("ui", vec!["/"], http_config.clone())
        .expect("failed to serve ui");

    http_server
        .bind_http_static_path(
            "/our",
            false,
            false,
            Some("text/html".to_string()),
            our.node().into(),
        )
        .expect("failed to bind to /our");

    http_server
        .bind_http_static_path(
            "/amionline",
            false,
            false,
            Some("text/html".to_string()),
            "yes".into(),
        )
        .expect("failed to bind to /amionline");

    http_server
        .bind_http_static_path(
            "/our.js",
            false,
            false,
            Some("application/javascript".to_string()),
            format!("window.our = {{}}; window.our.node = '{}';", &our.node).into(),
        )
        .expect("failed to bind to /our.js");

    // the base version gets written over on-bootstrap, so we look for
    // the persisted (user-customized) version first.
    // if it doesn't exist, we use the bootstrapped version and save it here.
    let stylesheet = kinode_process_lib::vfs::File {
        path: "/homepage:sys/pkg/persisted-kinode.css".to_string(),
        timeout: 5,
    }
    .read()
    .unwrap_or_else(|_| {
        kinode_process_lib::vfs::File {
            path: "/homepage:sys/pkg/kinode.css".to_string(),
            timeout: 5,
        }
        .read()
        .expect("failed to get kinode.css")
    });

    // save the stylesheet to the persisted file
    kinode_process_lib::vfs::File {
        path: "/homepage:sys/pkg/persisted-kinode.css".to_string(),
        timeout: 5,
    }
    .write(&stylesheet)
    .expect("failed to write to /persisted-kinode.css");

    http_server
        .bind_http_static_path(
            "/kinode.css",
            false, // kinode.css is not auth'd so that apps on subdomains can use it too!
            false,
            Some("text/css".to_string()),
            stylesheet,
        )
        .expect("failed to bind /kinode.css");

    http_server
        .bind_http_static_path(
            "/kinode.svg",
            false, // kinode.svg is not auth'd so that apps on subdomains can use it too!
            false,
            Some("image/svg+xml".to_string()),
            include_str!("../../pkg/kinode.svg").into(),
        )
        .expect("failed to bind /kinode.svg");

    http_server
        .bind_http_static_path(
            "/bird-orange.svg",
            false, // bird-orange.svg is not auth'd so that apps on subdomains can use it too!
            false,
            Some("image/svg+xml".to_string()),
            include_str!("../../pkg/bird-orange.svg").into(),
        )
        .expect("failed to bind /bird-orange.svg");

    http_server
        .bind_http_static_path(
            "/bird-plain.svg",
            false, // bird-plain.svg is not auth'd so that apps on subdomains can use it too!
            false,
            Some("image/svg+xml".to_string()),
            include_str!("../../pkg/bird-plain.svg").into(),
        )
        .expect("failed to bind /bird-plain.svg");

    // because boot uses this path to check if homepage is served yet,
    // it's best to respond dynamically and only serve this path once
    // all of the apps/widgets have populated.
    http_server
        .bind_http_path("/version", http_config.clone())
        .expect("failed to bind /version");

    http_server
        .bind_http_path("/apps", http_config.clone())
        .expect("failed to bind /apps");
    http_server
        .bind_http_path("/favorite", http_config.clone())
        .expect("failed to bind /favorite");
    http_server
        .bind_http_path("/order", http_config)
        .expect("failed to bind /order");

    kinode_process_lib::homepage::add_to_homepage("Clock", None, None, Some(&make_clock_widget()));

    // load persisted app order
    let mut persisted_app_order =
        kinode_process_lib::get_typed_state(|bytes| serde_json::from_slice(bytes))
            .unwrap_or(PersistedAppOrder::new());

    loop {
        let Ok(ref message) = await_message() else {
            // we never send requests, so this will never happen
            continue;
        };
        if message.source().process == "http-server:distro:sys" {
            if message.is_request() {
                let Ok(request) = http_server.parse_request(message.body()) else {
                    continue;
                };
                http_server.handle_request(
                    request,
                    |incoming| {
                        let path = incoming.bound_path(None);
                        match path {
                            "/apps" => (
                                server::HttpResponse::new(http::StatusCode::OK),
                                Some(LazyLoadBlob::new(
                                    Some("application/json"),
                                    serde_json::to_vec(
                                        &app_data.values().collect::<Vec<&homepage::App>>(),
                                    )
                                    .unwrap(),
                                )),
                            ),
                            "/version" => {
                                // hacky way to ensure that the homepage has populated itself before
                                // loading in after boot
                                if app_data.len() >= 4
                                    && app_data.values().filter(|app| app.widget.is_some()).count()
                                        >= 3
                                {
                                    (
                                        server::HttpResponse::new(http::StatusCode::OK),
                                        Some(LazyLoadBlob::new(
                                            Some("text/plain"),
                                            version_from_cargo_toml().as_bytes().to_vec(),
                                        )),
                                    )
                                } else {
                                    (server::HttpResponse::new(http::StatusCode::TOO_EARLY), None)
                                }
                            }
                            "/favorite" => {
                                let Ok(http::Method::POST) = incoming.method() else {
                                    return (
                                        server::HttpResponse::new(
                                            http::StatusCode::METHOD_NOT_ALLOWED,
                                        ),
                                        None,
                                    );
                                };
                                let Some(body) = get_blob() else {
                                    return (
                                        server::HttpResponse::new(http::StatusCode::BAD_REQUEST),
                                        None,
                                    );
                                };
                                let Ok(favorite_toggle) =
                                    serde_json::from_slice::<(String, bool)>(&body.bytes)
                                else {
                                    return (
                                        server::HttpResponse::new(http::StatusCode::BAD_REQUEST),
                                        None,
                                    );
                                };
                                if let Some(app) = app_data.get_mut(&favorite_toggle.0) {
                                    app.favorite = favorite_toggle.1;
                                }
                                (server::HttpResponse::new(http::StatusCode::OK), None)
                            }
                            "/order" => {
                                let Ok(http::Method::POST) = incoming.method() else {
                                    return (
                                        server::HttpResponse::new(
                                            http::StatusCode::METHOD_NOT_ALLOWED,
                                        ),
                                        None,
                                    );
                                };
                                let Some(body) = get_blob() else {
                                    return (
                                        server::HttpResponse::new(http::StatusCode::BAD_REQUEST),
                                        None,
                                    );
                                };
                                let Ok(order_list) =
                                    serde_json::from_slice::<Vec<(String, u32)>>(&body.bytes)
                                else {
                                    return (
                                        server::HttpResponse::new(http::StatusCode::BAD_REQUEST),
                                        None,
                                    );
                                };
                                for (app_id, order) in &order_list {
                                    if let Some(app) = app_data.get_mut(app_id) {
                                        app.order = *order;
                                    }
                                }
                                persisted_app_order = order_list.into_iter().collect();
                                kinode_process_lib::set_state(
                                    &serde_json::to_vec(&persisted_app_order).unwrap(),
                                );
                                (server::HttpResponse::new(http::StatusCode::OK), None)
                            }
                            _ => (server::HttpResponse::new(http::StatusCode::NOT_FOUND), None),
                        }
                    },
                    |_channel_id, _message_type, _message| {
                        // not expecting any websocket messages from FE currently
                    },
                );
            }
        } else {
            // handle messages to get apps, add or remove an app from the homepage.
            // they must have messaging access to us in order to perform this.
            if let Ok(request) = serde_json::from_slice::<homepage::Request>(message.body()) {
                match request {
                    homepage::Request::Add(homepage::AddRequest {
                        label,
                        icon,
                        path,
                        widget,
                    }) => {
                        let id = message.source().process.to_string();
                        app_data.insert(
                            id.clone(),
                            homepage::App {
                                id: id.clone(),
                                process: message.source().process().to_string(),
                                package_name: message.source().package().to_string(),
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
                                order: if let Some(order) = persisted_app_order.get(&id) {
                                    *order
                                } else {
                                    app_data.len() as u32
                                },
                                favorite: DEFAULT_FAVES
                                    .contains(&message.source().process.to_string().as_str()),
                            },
                        );
                    }
                    homepage::Request::Remove => {
                        let id = message.source().process.to_string();
                        app_data.remove(&id);
                        persisted_app_order.remove(&id);
                    }
                    homepage::Request::RemoveOther(id) => {
                        // caps check
                        let required_capability = Capability::new(
                            &our,
                            serde_json::to_string(&homepage::Capability::RemoveOther).unwrap(),
                        );
                        if !message.capabilities().contains(&required_capability) {
                            continue;
                        }
                        // end caps check
                        app_data.remove(&id);
                        persisted_app_order.remove(&id);
                    }
                    homepage::Request::GetApps => {
                        let apps = app_data.values().cloned().collect::<Vec<homepage::App>>();
                        let resp = homepage::Response::GetApps(apps);
                        Response::new()
                            .body(serde_json::to_vec(&resp).unwrap())
                            .send()
                            .unwrap();
                    }
                    homepage::Request::SetStylesheet(new_stylesheet_string) => {
                        // caps check
                        let required_capability = Capability::new(
                            &our,
                            serde_json::to_string(&homepage::Capability::SetStylesheet).unwrap(),
                        );
                        if !message.capabilities().contains(&required_capability) {
                            continue;
                        }
                        // end caps check
                        kinode_process_lib::vfs::File {
                            path: "/homepage:sys/pkg/persisted-kinode.css".to_string(),
                            timeout: 5,
                        }
                        .write(new_stylesheet_string.as_bytes())
                        .expect("failed to write to /persisted-kinode.css");
                        // re-bind
                        http_server
                            .bind_http_static_path(
                                "/kinode.css",
                                false, // kinode.css is not auth'd so that apps on subdomains can use it too!
                                false,
                                Some("text/css".to_string()),
                                new_stylesheet_string.into(),
                            )
                            .expect("failed to bind /kinode.css");
                        println!("updated kinode.css!");
                    }
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

fn make_clock_widget() -> String {
    return format!(
        r#"<html>
    <head>
        <meta name="viewport" content="width=device-width, initial-scale=1">
        <link rel="stylesheet" href="/kinode.css">
        <style>
            .clock {{
                width: 200px;
                height: 200px;
                border: 8px solid var(--text);
                border-radius: 50%;
                position: relative;
                margin: 20px auto;
            }}
            .hand {{
                position: absolute;
                bottom: 50%;
                left: 50%;
                transform-origin: bottom;
                background-color: light-dark(var(--off-black), var(--off-white));
            }}
            .hour {{
                width: 4px;
                height: 60px;
                margin-left: -2px;
            }}
            .minute {{
                width: 3px;
                height: 80px;
                margin-left: -1.5px;
            }}
            .second {{
                width: 2px;
                height: 90px;
                margin-left: -1px;
                background-color: var(--orange);
            }}
            .center {{
                width: 12px;
                height: 12px;
                border-radius: 50%;
                position: absolute;
                top: 50%;
                left: 50%;
                transform: translate(-50%, -50%);
            }}
            .marker {{
                position: absolute;
                width: 2px;
                height: 4px;
                background: light-dark(var(--off-black), var(--off-white));
                left: 50%;
                margin-left: -1px;
                transform-origin: 50% 100px;
            }}
            .marker.primary {{
                width: 3px;
                height: 8px;
                margin-left: -1.5px;
            }}
            .digital-time {{
                font-family: var(--font-family-main);
                margin-top: 1em;
                font-size: 0.7em;
                color: light-dark(var(--off-black), var(--off-white));
                position: absolute;
                width:100%;
                text-align: center;
                bottom: 40px;
            }}
        </style>
    </head>
    <body style="margin: 0;">
        <div class="clock">
            <div class="marker primary" style="transform: rotate(0deg)"></div>
            <div class="marker" style="transform: rotate(30deg)"></div>
            <div class="marker" style="transform: rotate(60deg)"></div>
            <div class="marker primary" style="transform: rotate(90deg)"></div>
            <div class="marker" style="transform: rotate(120deg)"></div>
            <div class="marker" style="transform: rotate(150deg)"></div>
            <div class="marker primary" style="transform: rotate(180deg)"></div>
            <div class="marker" style="transform: rotate(210deg)"></div>
            <div class="marker" style="transform: rotate(240deg)"></div>
            <div class="marker primary" style="transform: rotate(270deg)"></div>
            <div class="marker" style="transform: rotate(300deg)"></div>
            <div class="marker" style="transform: rotate(330deg)"></div>
            <div class="hand hour" id="hour"></div>
            <div class="hand minute" id="minute"></div>
            <div class="hand second" id="second"></div>
            <div class="center"></div>
        </div>
        <div class="digital-time" id="digital"></div>

        <script>
            function updateClock() {{
                const now = new Date();
                const hours = now.getHours() % 12;
                const minutes = now.getMinutes();
                const seconds = now.getSeconds();

                const hourDeg = (hours * 30) + (minutes * 0.5);
                const minuteDeg = minutes * 6;
                const secondDeg = seconds * 6;

                document.getElementById('hour').style.transform = `rotate(${{hourDeg}}deg)`;
                document.getElementById('minute').style.transform = `rotate(${{minuteDeg}}deg)`;
                document.getElementById('second').style.transform = `rotate(${{secondDeg}}deg)`;

                // Update digital display
                const displayHours = hours === 0 ? 12 : hours;
                const displayMinutes = minutes.toString().padStart(2, '0');
                document.getElementById('digital').textContent = `${{displayHours}}:${{displayMinutes}}`;
            }}

            setInterval(updateClock, 1000);
            updateClock();
        </script>
    </body>
</html>"#
    );
}
