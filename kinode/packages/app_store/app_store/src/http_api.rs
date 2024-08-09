use crate::{
    kinode::process::downloads::{DownloadRequest, DownloadResponse},
    state::{MirrorCheck, PackageState, State},
};

use kinode_process_lib::{
    http::server,
    http::{self, Method, StatusCode},
    Address, LazyLoadBlob, NodeId, PackageId, Request,
};
use kinode_process_lib::{SendError, SendErrorKind};
use serde_json::json;
use std::{collections::HashMap, str::FromStr};

const ICON: &str = include_str!("icon");

/// Bind static and dynamic HTTP paths for the app store,
/// bind to our WS updates path, and add icon and widget to homepage.
pub fn init_frontend(our: &Address, http_server: &mut server::HttpServer) {
    let config = server::HttpBindingConfig::default();

    for path in [
        "/apps",              // all apps, combination of downloads, installed
        "/downloads",         // all downloads
        "/installed",         // all installed apps
        "/ourapps",           // all apps we've published
        "/apps/:id",          // detail about a specific app
        "/apps/:id/download", // download a listed app
        "/apps/:id/install",  // install a downloaded app
        "/apps/:id/update",
        "/apps/:id/caps",
        "/apps/:id/mirror",
        "/apps/:id/auto-update",
        "/apps/rebuild-index",
        "/mirrorcheck/:node",
    ] {
        http_server
            .bind_http_path(path, config.clone())
            .expect("failed to bind http path");
    }
    http_server
        .serve_ui(
            &our,
            "ui",
            vec!["/", "/app/:id", "/publish", "/downloads/:id"],
            config.clone(),
        )
        .expect("failed to serve static UI");

    http_server
        .bind_ws_path("/", server::WsBindingConfig::default())
        .expect("failed to bind ws path");

    // add ourselves to the homepage
    kinode_process_lib::homepage::add_to_homepage(
        "App Store",
        Some(ICON),
        Some("/"),
        Some(&make_widget()),
    );
}

fn make_widget() -> String {
    return r#"<html>
<head>
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <link rel="stylesheet" href="/kinode.css">
    <style>
        * {
            box-sizing: border-box;
            margin: 0;
            padding: 0;
            font-family: 'Kode Mono', monospace;
        }

        body {
            overflow: hidden;
            background: transparent;
        }

        #latest-apps {
            display: flex;
            flex-wrap: wrap;
            padding: 0.5rem;
            gap: 0.5rem;
            align-items: center;
            border-radius: 1rem;
            box-shadow: 0 10px 15px -3px rgba(0, 0, 0, 0.1), 0 4px 6px -2px rgba(0, 0, 0, 0.05);
            height: 100vh;
            width: 100vw;
            overflow-y: auto;
        }

        .app {
            padding: 0.5rem;
            display: flex;
            flex-grow: 1;
            align-items: stretch;
            border-radius: 0.5rem;
            box-shadow: 0 1px 2px 0 rgba(0, 0, 0, 0.05);
            cursor: pointer;
            font-family: sans-serif;
            width: 100%;
        }

        .app-image {
            border-radius: 0.75rem;
            margin-right: 0.5rem;
            flex-grow: 1;
            background-size: contain;
            background-repeat: no-repeat;
            background-position: center;
            height: 92px;
            width: 92px;
            max-width: 33%;
        }

        .app-info {
            display: flex;
            flex-direction: column;
            flex-grow: 1;
            max-width: 67%;
        }

        .app-info h2 {
            font-weight: bold;
            font-size: medium;
        }

        @media screen and (min-width: 500px) {
            .app {
                width: 49%;
            }
        }
    </style>
</head>
<body>
    <div id="latest-apps"></div>
    <script>
        document.addEventListener('DOMContentLoaded', function() {
            fetch('/main:app_store:sys/apps', { credentials: 'include' })
                .then(response => response.json())
                .then(data => {
                    const container = document.getElementById('latest-apps');
                    data.forEach(app => {
                        if (app.metadata) {
                            const a = document.createElement('a');
                            a.className = 'app';
                            a.href = `/main:app_store:sys/app/${app.package}:${app.publisher}`
                            a.target = '_blank';
                            a.rel = 'noopener noreferrer';
                            const iconLetter = app.metadata_hash.replace('0x', '')[0].toUpperCase();
                            a.innerHTML = `<div
                                class="app-image"
                                style="
                                    background-image: url('${app.metadata.image || `/bird-orange.svg`}');
                                "
                            ></div>
                            <div class="app-info">
                                <h2>${app.metadata.name}</h2>
                                <p>${app.metadata.description}</p>
                            </div>`;
                                container.appendChild(a);
                        }
                    });
                })
                .catch(error => console.error('Error fetching apps:', error));
        });
    </script>
</body>
</html>"#
        .to_string();
}

/// Actions supported over HTTP:
/// - get all apps: GET /apps
/// - get all downloaded apps: GET /downloads
/// - get all installed apps: GET /installed
/// - get all apps we've published: GET /ourapps
/// - get detail about a specific app: GET /apps/:id
/// - get detail about a specific apps downloads: GET /downloads/:id
/// - get capabilities for a specific app: GET /apps/:id/caps
///
/// - get online/offline mirrors for a listed app: GET /mirrorcheck/:node
/// - download a listed app: POST /apps/:id/download
/// - install a downloaded app: POST /apps/:id/install
/// - uninstall/delete a downloaded app: DELETE /apps/:id
/// - update a downloaded app: POST /apps/:id/update
/// - approve capabilities for a downloaded app: POST /apps/:id/caps
/// - start mirroring a downloaded app: PUT /apps/:id/mirror
/// - stop mirroring a downloaded app: DELETE /apps/:id/mirror
/// - start auto-updating a downloaded app: PUT /apps/:id/auto-update
/// - stop auto-updating a downloaded app: DELETE /apps/:id/auto-update
///
/// - RebuildIndex: POST /apps/rebuild-index // TODO, this could be just terminal I think?
pub fn handle_http_request(
    our: &Address,
    state: &mut State,
    req: &server::IncomingHttpRequest,
) -> (server::HttpResponse, Option<LazyLoadBlob>) {
    match serve_paths(our, state, req) {
        Ok((status_code, _headers, body)) => (
            server::HttpResponse::new(status_code).header("Content-Type", "application/json"),
            Some(LazyLoadBlob {
                mime: None,
                bytes: body,
            }),
        ),
        Err(_e) => (
            server::HttpResponse::new(http::StatusCode::INTERNAL_SERVER_ERROR),
            None,
        ),
    }
}

fn get_package_id(url_params: &HashMap<String, String>) -> anyhow::Result<PackageId> {
    let Some(package_id) = url_params.get("id") else {
        return Err(anyhow::anyhow!("Missing id"));
    };

    let id = package_id.parse::<PackageId>()?;
    Ok(id)
}

fn gen_package_info(id: &PackageId, state: &PackageState) -> serde_json::Value {
    // installed package.
    // on-chain info combined somewhere else.

    json!({
        "package": id.package().to_string(),
        "publisher": id.publisher(),
        "our_version_hash": state.our_version_hash,
        "verified": state.verified,
        "caps_approved": state.caps_approved,
        "manifest_hash": state.manifest_hash,
    })
}

fn serve_paths(
    our: &Address,
    state: &mut State,
    req: &server::IncomingHttpRequest,
) -> anyhow::Result<(http::StatusCode, Option<HashMap<String, String>>, Vec<u8>)> {
    let method = req.method()?;

    let bound_path: &str = req.bound_path(Some(&our.process.to_string()));
    let url_params = req.url_params();

    match bound_path {
        // GET all apps
        "/apps" => {
            // TODO COMBINE DOWNLOADED AND INSTALLED
            if method != Method::GET {
                return Ok((
                    StatusCode::METHOD_NOT_ALLOWED,
                    None,
                    format!("Invalid method {method} for {bound_path}").into_bytes(),
                ));
            }
            let all: Vec<serde_json::Value> = state
                .packages
                .iter()
                .map(|(package_id, listing)| gen_package_info(package_id, listing))
                .collect();
            return Ok((StatusCode::OK, None, serde_json::to_vec(&all)?));
        }
        "/downloads" => {
            // TODO
            Ok((StatusCode::OK, None, vec![]))
        }
        "/installed" => {
            // TODO
            Ok((StatusCode::OK, None, vec![]))
        }
        "/ourapps" => {
            // TODO
            Ok((StatusCode::OK, None, vec![]))
        }
        "/downloads/:id" => {
            // TODO
            Ok((StatusCode::OK, None, vec![]))
        }
        // GET detail about a specific app
        // update a downloaded app: PUT
        "/apps/:id" => {
            let Ok(package_id) = get_package_id(url_params) else {
                return Ok((
                    StatusCode::BAD_REQUEST,
                    None,
                    format!("Missing id").into_bytes(),
                ));
            };

            match method {
                Method::GET => {
                    let Some(listing) = state.packages.get(&package_id) else {
                        return Ok((
                            StatusCode::NOT_FOUND,
                            None,
                            format!("App not found: {package_id}").into_bytes(),
                        ));
                    };
                    Ok((
                        StatusCode::OK,
                        None,
                        gen_package_info(&package_id, listing)
                            .to_string()
                            .into_bytes(),
                    ))
                }
                Method::DELETE => {
                    // uninstall an app
                    state.uninstall(&package_id)?;
                    Ok((
                        StatusCode::NO_CONTENT,
                        None,
                        format!("Uninstalled").into_bytes(),
                    ))
                }
                _ => Ok((
                    StatusCode::METHOD_NOT_ALLOWED,
                    None,
                    format!("Invalid method {method} for {bound_path}").into_bytes(),
                )),
            }
        }
        // PUT /apps/:id/download
        // download a listed app from a mirror
        "/apps/:id/download" => {
            let Ok(package_id) = get_package_id(url_params) else {
                return Ok((
                    StatusCode::BAD_REQUEST,
                    None,
                    format!("Missing id").into_bytes(),
                ));
            };
            // from POST body, look for download_from field and use that as the mirror
            let body = crate::get_blob()
                .ok_or(anyhow::anyhow!("missing blob"))?
                .bytes;
            let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let download_from = body_json
                .get("download_from")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("No download_from specified!"))?;
            let version_hash = body_json
                .get("version_hash")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("No version_hash specified!"))?;

            // TODO: handle HTTP urls here I think, with different context...

            let download_request = DownloadRequest {
                package_id: crate::kinode::process::main::PackageId::from_process_lib(package_id),
                download_from: Some(download_from),
                desired_version_hash: version_hash,
            };
            // TODO make these constants somewhere or something. this is so bad
            let downloads_process =
                Address::from_str(&format!("{:?}@downloads:app_store:sys", our.node)).unwrap();

            // send and await response to downloads process
            let response = Request::new()
                .target(downloads_process)
                .body(serde_json::to_vec(&download_request).unwrap())
                .send_and_await_response(5)??;

            let response: DownloadResponse = serde_json::from_slice(&response.body())?;
            Ok((StatusCode::OK, None, serde_json::to_vec(&response)?))
        }
        // POST /apps/:id/update
        // update a downloaded app
        "/apps/:id/update" => {
            let Ok(package_id) = get_package_id(url_params) else {
                return Ok((
                    StatusCode::BAD_REQUEST,
                    None,
                    format!("Missing id").into_bytes(),
                ));
            };

            // TODO send to downloads:
            // // actually just install something, we go get it from vfs or ask for it from downloads?

            match method {
                _ => Ok((
                    StatusCode::METHOD_NOT_ALLOWED,
                    None,
                    format!("Invalid method {method} for {bound_path}").into_bytes(),
                )),
            }
        }
        // POST /apps/:id/install
        // install a downloaded app
        "/apps/:id/install" => {
            let Ok(package_id) = get_package_id(url_params) else {
                return Ok((
                    StatusCode::BAD_REQUEST,
                    None,
                    format!("Missing id").into_bytes(),
                ));
            };

            match crate::utils::install(&package_id, &our.to_string()) {
                Ok(_) => Ok((StatusCode::CREATED, None, vec![])),
                Err(e) => Ok((
                    StatusCode::SERVICE_UNAVAILABLE,
                    None,
                    e.to_string().into_bytes(),
                )),
            }
        }
        // GET caps for a specific downloaded app
        // approve capabilities for a downloaded app: POST
        "/apps/:id/caps" => {
            let Ok(package_id) = get_package_id(url_params) else {
                return Ok((
                    StatusCode::BAD_REQUEST,
                    None,
                    format!("Missing id").into_bytes(),
                ));
            };
            match method {
                // return the capabilities for that app
                Method::GET => Ok(match crate::utils::fetch_package_manifest(&package_id) {
                    Ok(manifest) => (StatusCode::OK, None, serde_json::to_vec(&manifest)?),
                    Err(_) => (
                        StatusCode::NOT_FOUND,
                        None,
                        format!("App manifest not found: {package_id}").into_bytes(),
                    ),
                }),
                // approve the capabilities for that app
                // Method::POST => Ok(
                //     match state.update_downloaded_package(&package_id, |pkg| {
                //         pkg.caps_approved = true;
                //     }) {
                //         true => (StatusCode::OK, None, vec![]),
                //         false => (
                //             StatusCode::NOT_FOUND,
                //             None,
                //             format!("App not found: {package_id}").into_bytes(),
                //         ),
                //     },
                // ),
                _ => Ok((
                    StatusCode::METHOD_NOT_ALLOWED,
                    None,
                    format!("Invalid method {method} for {bound_path}").into_bytes(),
                )),
            }
        }
        // start mirroring a downloaded app: PUT
        // stop mirroring a downloaded app: DELETE
        "/apps/:id/mirror" => {
            let Ok(package_id) = get_package_id(url_params) else {
                return Ok((
                    StatusCode::BAD_REQUEST,
                    None,
                    format!("Missing id").into_bytes(),
                ));
            };

            // TODO move to downloads.

            match method {
                // start mirroring an app
                // Method::PUT => {
                //     state.start_mirroring(&package_id);
                //     Ok((StatusCode::OK, None, vec![]))
                // }
                // // stop mirroring an app
                // Method::DELETE => {
                //     state.stop_mirroring(&package_id);
                //     Ok((StatusCode::OK, None, vec![]))
                // }
                _ => Ok((
                    StatusCode::METHOD_NOT_ALLOWED,
                    None,
                    format!("Invalid method {method} for {bound_path}").into_bytes(),
                )),
            }
        }
        // start auto-updating a downloaded app: PUT
        // stop auto-updating a downloaded app: DELETE
        "/apps/:id/auto-update" => {
            let Ok(package_id) = get_package_id(url_params) else {
                return Ok((
                    StatusCode::BAD_REQUEST,
                    None,
                    format!("Missing id").into_bytes(),
                ));
            };

            match method {
                // start auto-updating an app
                // Method::PUT => {
                //     state.start_auto_update(&package_id);
                //     Ok((StatusCode::OK, None, vec![]))
                // }
                // // stop auto-updating an app
                // Method::DELETE => {
                //     state.stop_auto_update(&package_id);
                //     Ok((StatusCode::OK, None, vec![]))
                // }
                _ => Ok((
                    StatusCode::METHOD_NOT_ALLOWED,
                    None,
                    format!("Invalid method {method} for {bound_path}").into_bytes(),
                )),
            }
        }
        // GET online/offline mirrors for a listed app
        "/mirrorcheck/:node" => {
            if method != Method::GET {
                return Ok((
                    StatusCode::METHOD_NOT_ALLOWED,
                    None,
                    format!("Invalid method {method} for {bound_path}").into_bytes(),
                ));
            }
            let Some(node) = url_params.get("node") else {
                return Ok((
                    StatusCode::BAD_REQUEST,
                    None,
                    format!("Missing node").into_bytes(),
                ));
            };
            if let Err(SendError { kind, .. }) = Request::to((node, "net", "distro", "sys"))
                .body(b"checking your mirror status...")
                .send_and_await_response(3)
                .unwrap()
            {
                match kind {
                    SendErrorKind::Timeout => {
                        let check_reponse = MirrorCheck {
                            node: node.to_string(),
                            is_online: false,
                            error: Some(format!("node {} timed out", node).to_string()),
                        };
                        return Ok((StatusCode::OK, None, serde_json::to_vec(&check_reponse)?));
                    }
                    SendErrorKind::Offline => {
                        let check_reponse = MirrorCheck {
                            node: node.to_string(),
                            is_online: false,
                            error: Some(format!("node {} is offline", node).to_string()),
                        };
                        return Ok((StatusCode::OK, None, serde_json::to_vec(&check_reponse)?));
                    }
                }
            } else {
                let check_reponse = MirrorCheck {
                    node: node.to_string(),
                    is_online: true,
                    error: None,
                };
                return Ok((StatusCode::OK, None, serde_json::to_vec(&check_reponse)?));
            }
        }
        _ => Ok((
            StatusCode::NOT_FOUND,
            None,
            format!("Path not found: {bound_path}").into_bytes(),
        )),
    }
}
