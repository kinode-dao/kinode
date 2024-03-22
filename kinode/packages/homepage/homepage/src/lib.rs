#![feature(let_chains)]
use kinode_process_lib::{
    await_message, call_init, get_blob,
    http::{
        bind_http_path, bind_http_static_path, get_mime_type, serve_index_html, serve_ui,
        HttpServerError,
    },
    println,
    vfs::{FileType, VfsAction, VfsRequest, VfsResponse},
    Address, LazyLoadBlob as KiBlob, Message, ProcessId, Request as KiRequest,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

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

const HOME_PAGE: &str = include_str!("../../pkg/ui/index.html");

const APP_TEMPLATE: &str = r#"
<a class="app-link" id="${package_name}" href="/${path}">
  <img
    src="${base64_icon}" />
  <h6>${label}</h6>
</a>"#;

call_init!(init);

// Copied in from process_lib serve_ui. see https://github.com/kinode-dao/process_lib/blob/main/src/http.rs
fn static_serve_dir(
    our: &Address,
    directory: &str,
    authenticated: bool,
    local_only: bool,
    paths: Vec<&str>,
) -> anyhow::Result<()> {
    serve_index_html(our, directory, authenticated, local_only, paths)?;

    let initial_path = format!("{}/pkg/{}", our.package_id(), directory);
    println!("initial path: {}", initial_path);

    let mut queue = VecDeque::new();
    queue.push_back(initial_path.clone());

    while let Some(path) = queue.pop_front() {
        let Ok(directory_response) = KiRequest::to(("our", "vfs", "distro", "sys"))
            .body(serde_json::to_vec(&VfsRequest {
                path,
                action: VfsAction::ReadDir,
            })?)
            .send_and_await_response(5)?
        else {
            return Err(anyhow::anyhow!(
                "serve_ui: no response for path: {}",
                initial_path
            ));
        };

        let directory_body = serde_json::from_slice::<VfsResponse>(directory_response.body())?;

        // Determine if it's a file or a directory and handle appropriately
        match directory_body {
            VfsResponse::ReadDir(directory_info) => {
                for entry in directory_info {
                    match entry.file_type {
                        // If it's a file, serve it statically
                        FileType::File => {
                            KiRequest::to(("our", "vfs", "distro", "sys"))
                                .body(serde_json::to_vec(&VfsRequest {
                                    path: entry.path.clone(),
                                    action: VfsAction::Read,
                                })?)
                                .send_and_await_response(5)??;

                            let Some(blob) = get_blob() else {
                                return Err(anyhow::anyhow!(
                                    "serve_ui: no blob for {}",
                                    entry.path
                                ));
                            };

                            let content_type = get_mime_type(&entry.path);

                            println!("binding {}", entry.path.replace(&initial_path, ""));

                            bind_http_static_path(
                                entry.path.replace(&initial_path, ""),
                                authenticated, // Must be authenticated
                                local_only,    // Is not local-only
                                Some(content_type),
                                blob.bytes,
                            )?;
                        }
                        FileType::Directory => {
                            // Push the directory onto the queue
                            queue.push_back(entry.path);
                        }
                        _ => {}
                    }
                }
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "serve_ui: unexpected response for path: {:?}",
                    directory_body
                ))
            }
        };
    }

    Ok(())
}

fn init(our: Address) {
    let mut apps: HashMap<ProcessId, String> = HashMap::new();

    static_serve_dir(&our, "ui", true, false, vec!["/", "/login"]);

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
                        // bind_index(&our.node, &apps);
                    }
                    HomepageRequest::Remove => {
                        apps.remove(&message.source().process);
                        // bind_index(&our.node, &apps);
                    }
                }
            }
        }
    }
}
