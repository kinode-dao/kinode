#![feature(let_chains)]
use kinode_process_lib::{
    await_message, call_init,
    http::{
        bind_http_path, bind_http_static_path, send_response, HttpServerError, HttpServerRequest,
        StatusCode,
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

const HOME_PAGE: &str = include_str!("index.html");

const APP_TEMPLATE: &str = r#"
<a class="app-link" id="${package_name}" href="/${path}">
  <img
    src="${base64_icon}" />
  <h6>${label}</h6>
</a>"#;

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

// // Copied in from process_lib serve_ui. see https://github.com/kinode-dao/process_lib/blob/main/src/http.rs
// fn static_serve_dir(
//     our: &Address,
//     directory: &str,
//     authenticated: bool,
//     local_only: bool,
//     paths: Vec<&str>,
// ) -> anyhow::Result<()> {
//     serve_index_html(our, directory, authenticated, local_only, paths)?;

// let initial_path = format!("{}/pkg/{}", our.package_id(), directory);
// println!("initial path: {}", initial_path);

// let mut queue = VecDeque::new();
// queue.push_back(initial_path.clone());

// while let Some(path) = queue.pop_front() {
//     let Ok(directory_response) = KiRequest::to(("our", "vfs", "distro", "sys"))
//         .body(serde_json::to_vec(&VfsRequest {
//             path,
//             action: VfsAction::ReadDir,
//         })?)
//         .send_and_await_response(5)?
//     else {
//         return Err(anyhow::anyhow!(
//             "serve_ui: no response for path: {}",
//             initial_path
//         ));
//     };

//     let directory_body = serde_json::from_slice::<VfsResponse>(directory_response.body())?;

//     // Determine if it's a file or a directory and handle appropriately
//     match directory_body {
//         VfsResponse::ReadDir(directory_info) => {
//             for entry in directory_info {
//                 match entry.file_type {
//                     // If it's a file, serve it statically
//                     FileType::File => {
//                         KiRequest::to(("our", "vfs", "distro", "sys"))
//                             .body(serde_json::to_vec(&VfsRequest {
//                                 path: entry.path.clone(),
//                                 action: VfsAction::Read,
//                             })?)
//                             .send_and_await_response(5)??;

//                         let Some(blob) = get_blob() else {
//                             return Err(anyhow::anyhow!(
//                                 "serve_ui: no blob for {}",
//                                 entry.path
//                             ));
//                         };

//                         let content_type = get_mime_type(&entry.path);

//                         println!("binding {}", entry.path.replace(&initial_path, ""));

//                         bind_http_static_path(
//                             entry.path.replace(&initial_path, ""),
//                             authenticated, // Must be authenticated
//                             local_only,    // Is not local-only
//                             Some(content_type),
//                             blob.bytes,
//                         )?;
//                     }
//                     FileType::Directory => {
//                         // Push the directory onto the queue
//                         queue.push_back(entry.path);
//                     }
//                     _ => {}
//                 }
//             }
//         }
//         _ => {
//             return Err(anyhow::anyhow!(
//                 "serve_ui: unexpected response for path: {:?}",
//                 directory_body
//             ))
//         }
//     };
// }

//     Ok(())
// }

call_init!(init);
fn init(our: Address) {
    let mut apps: HashMap<ProcessId, String> = HashMap::new();
    let mut app_data: HashMap<ProcessId, HomepageApp> = HashMap::new();

    // static_serve_dir(&our, "index.html", true, false, vec!["/"]);
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
                        app_data.insert(
                            message.source().process.clone(),
                            HomepageApp {
                                package_name: message.source().clone().package().to_string(),
                                path: path.clone(),
                                label: label.clone(),
                                base64_icon: icon.clone(),
                            },
                        );
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
                                app_data
                                    .values()
                                    .map(|app| serde_json::to_string(app).unwrap())
                                    .collect::<Vec<String>>()
                                    .join("\n")
                                    .as_bytes()
                                    .to_vec(),
                            );
                        }
                        send_response(
                            StatusCode::OK,
                            Some(HashMap::new()),
                            "hello".as_bytes().to_vec(),
                        );
                    }
                    _ => {}
                }
            }
        }
    }
}
