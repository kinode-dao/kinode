//! http_api for main:app_store:sys
//! handles http_requests coming in, sending them to relevant processes (main/downloads/chain),
//! and sends back http_responses.
//!
use crate::{
    kinode::process::chain::{ChainRequests, ChainResponses},
    kinode::process::downloads::{
        DownloadRequests, DownloadResponses, LocalDownloadRequest, RemoveFileRequest,
    },
    state::{MirrorCheck, PackageState, State},
};

use kinode_process_lib::{
    http::{self, server, Method, StatusCode},
    println, Address, LazyLoadBlob, PackageId, Request, SendError, SendErrorKind,
};
use serde_json::json;
use std::{collections::HashMap, str::FromStr};

const ICON: &str = include_str!("icon");

/// Bind static and dynamic HTTP paths for the app store,
/// bind to our WS updates path, and add icon and widget to homepage.
pub fn init_frontend(our: &Address, http_server: &mut server::HttpServer) {
    let config = server::HttpBindingConfig::default();

    for path in [
        "/apps",          // all on-chain apps
        "/downloads",     // all downloads
        "/installed",     // all installed apps
        "/ourapps",       // all apps we've published
        "/apps/:id",      // detail about an on-chain app
        "/downloads/:id", // local downloads for an app
        "/installed/:id", // detail about an installed app
        // actions
        "/apps/:id/download",    // download a listed app
        "/apps/:id/install",     // install a downloaded app
        "/downloads/:id/mirror", // start mirroring a version of a downloaded app
        "/downloads/:id/remove", // remove a downloaded app
        "/apps/:id/auto-update", // set auto-updating a version of a downloaded app
        "/mirrorcheck/:node",    // check if a node/mirror is online/offline
    ] {
        http_server
            .bind_http_path(path, config.clone())
            .expect("failed to bind http path");
    }
    http_server
        .serve_ui(
            &our,
            "ui",
            vec!["/", "/app/:id", "/publish", "/download/:id", "my-downloads"],
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
                            a.href = `/main:app_store:sys/app/${app.package_id.package_name}:${app.package_id.publisher_node}`
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
/// - remove a downloaded app: POST /downloads/:id/remove

/// - get online/offline mirrors for a listed app: GET /mirrorcheck/:node
/// - download a listed app: POST /apps/:id/download
/// - install a downloaded app: POST /apps/:id/install
/// - uninstall/delete a downloaded app: DELETE /apps/:id
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
        Err(e) => (
            server::HttpResponse::new(http::StatusCode::INTERNAL_SERVER_ERROR),
            Some(LazyLoadBlob {
                mime: None,
                bytes: serde_json::to_vec(&json!({"error": e.to_string()})).unwrap(),
            }),
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
    // installed package info
    json!({
        "package_id": {
            "package_name": id.package(),
            "publisher_node": id.publisher(),
        },
        "our_version_hash": state.our_version_hash,
        "publisher": id.publisher(),
        "our_version_hash": state.our_version_hash,
        "verified": state.verified,
        "caps_approved": state.caps_approved,
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
            let resp = Request::to(("our", "chain", "app_store", "sys"))
                .body(serde_json::to_vec(&ChainRequests::GetApps)?)
                .send_and_await_response(5)??;
            let msg = serde_json::from_slice::<ChainResponses>(resp.body())?;
            match msg {
                ChainResponses::GetApps(apps) => {
                    Ok((StatusCode::OK, None, serde_json::to_vec(&apps)?))
                }
                _ => Err(anyhow::anyhow!("Invalid response from chain: {:?}", msg)),
            }
        }
        // GET detail about a specific app
        // DELETE uninstall an app
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
                    let package_id =
                        crate::kinode::process::main::PackageId::from_process_lib(package_id);
                    let resp = Request::to(("our", "chain", "app_store", "sys"))
                        .body(serde_json::to_vec(&ChainRequests::GetApp(package_id))?)
                        .send_and_await_response(5)??;
                    let msg = serde_json::from_slice::<ChainResponses>(resp.body())?;
                    match msg {
                        ChainResponses::GetApp(app) => {
                            Ok((StatusCode::OK, None, serde_json::to_vec(&app)?))
                        }
                        _ => Err(anyhow::anyhow!("Invalid response from chain: {:?}", msg)),
                    }
                }
                Method::DELETE => {
                    // uninstall an app
                    crate::utils::uninstall(state, &package_id)?;
                    println!("successfully uninstalled {:?}", package_id);
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
        "/downloads" => {
            // get all local downloads!
            let resp = Request::to(("our", "downloads", "app_store", "sys"))
                .body(serde_json::to_vec(&DownloadRequests::GetFiles(None))?)
                .send_and_await_response(5)??;

            let msg = serde_json::from_slice::<DownloadResponses>(resp.body())?;
            match msg {
                DownloadResponses::GetFiles(files) => {
                    Ok((StatusCode::OK, None, serde_json::to_vec(&files)?))
                }
                DownloadResponses::Err(e) => Err(anyhow::anyhow!("Error from downloads: {:?}", e)),
                _ => Err(anyhow::anyhow!(
                    "Invalid response from downloads: {:?}",
                    msg
                )),
            }
        }
        "/downloads/:id" => {
            // get all local downloads!
            let Ok(package_id) = get_package_id(url_params) else {
                return Ok((
                    StatusCode::BAD_REQUEST,
                    None,
                    format!("Missing id").into_bytes(),
                ));
            };
            let package_id = crate::kinode::process::main::PackageId::from_process_lib(package_id);
            let resp = Request::to(("our", "downloads", "app_store", "sys"))
                .body(serde_json::to_vec(&DownloadRequests::GetFiles(Some(
                    package_id,
                )))?)
                .send_and_await_response(5)??;

            let msg = serde_json::from_slice::<DownloadResponses>(resp.body())?;
            match msg {
                DownloadResponses::GetFiles(files) => {
                    Ok((StatusCode::OK, None, serde_json::to_vec(&files)?))
                }
                DownloadResponses::Err(e) => Err(anyhow::anyhow!("Error from downloads: {:?}", e)),
                _ => Err(anyhow::anyhow!(
                    "Invalid response from downloads: {:?}",
                    msg
                )),
            }
        }
        "/installed" => {
            let all: Vec<serde_json::Value> = state
                .packages
                .iter()
                .map(|(package_id, listing)| gen_package_info(package_id, listing))
                .collect();
            return Ok((StatusCode::OK, None, serde_json::to_vec(&all)?));
        }
        "/installed/:id" => {
            let Ok(package_id) = get_package_id(url_params) else {
                return Ok((
                    StatusCode::BAD_REQUEST,
                    None,
                    format!("Missing id").into_bytes(),
                ));
            };
            let specific_package_info = state
                .packages
                .get(&package_id)
                .map(|listing| gen_package_info(&package_id, listing))
                .ok_or_else(|| {
                    anyhow::Error::new(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("Package with id {} not found", package_id),
                    ))
                })?;
            return Ok((
                StatusCode::OK,
                None,
                serde_json::to_vec(&specific_package_info)?,
            ));
        }
        "/ourapps" => {
            let resp = Request::to(("our", "chain", "app_store", "sys"))
                .body(serde_json::to_vec(&ChainRequests::GetOurApps)?)
                .send_and_await_response(5)??;
            let msg = serde_json::from_slice::<ChainResponses>(resp.body())?;
            match msg {
                ChainResponses::GetOurApps(apps) => {
                    Ok((StatusCode::OK, None, serde_json::to_vec(&apps)?))
                }
                _ => Err(anyhow::anyhow!("Invalid response from chain: {:?}", msg)),
            }
        }
        // POST /apps/:id/download
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

            let download_request = DownloadRequests::LocalDownload(LocalDownloadRequest {
                package_id: crate::kinode::process::main::PackageId::from_process_lib(package_id),
                download_from: download_from.clone(),
                desired_version_hash: version_hash,
            });

            Request::to(("our", "downloads", "app_store", "sys"))
                .body(serde_json::to_vec(&download_request)?)
                .send()?;
            Ok((
                StatusCode::OK,
                None,
                serde_json::to_vec(&DownloadResponses::Success)?,
            ))
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

            let body = crate::get_blob()
                .ok_or(anyhow::anyhow!("missing blob"))?
                .bytes;
            let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();

            let version_hash = body_json
                .get("version_hash")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("No version_hash specified!"))?;

            let process_package_id =
                crate::kinode::process::main::PackageId::from_process_lib(package_id);

            match crate::utils::install(
                &process_package_id,
                None,
                &version_hash,
                state,
                &our.node().to_string(),
            ) {
                Ok(_) => {
                    println!("successfully installed package: {:?}", process_package_id);
                    Ok((StatusCode::CREATED, None, vec![]))
                }
                Err(e) => Ok((
                    StatusCode::SERVICE_UNAVAILABLE,
                    None,
                    e.to_string().into_bytes(),
                )),
            }
        }
        // start mirroring a downloaded app: PUT
        // stop mirroring a downloaded app: DELETE
        "/downloads/:id/mirror" => {
            let Ok(package_id) = get_package_id(url_params) else {
                return Ok((
                    StatusCode::BAD_REQUEST,
                    None,
                    format!("Missing id").into_bytes(),
                ));
            };
            let downloads = Address::from_str("our@downloads:app_store:sys")?;

            match method {
                // start mirroring an app
                Method::PUT => {
                    let resp = Request::new()
                        .target(downloads)
                        .body(serde_json::to_vec(&DownloadRequests::StartMirroring(
                            crate::kinode::process::main::PackageId::from_process_lib(package_id),
                        ))?)
                        .send_and_await_response(5)??;
                    let msg = serde_json::from_slice::<DownloadResponses>(resp.body())?;
                    match msg {
                        DownloadResponses::Success => Ok((StatusCode::OK, None, vec![])),
                        DownloadResponses::Err(e) => {
                            Err(anyhow::anyhow!("Error starting mirroring: {:?}", e))
                        }
                        _ => Err(anyhow::anyhow!(
                            "Invalid response from downloads: {:?}",
                            msg
                        )),
                    }
                }
                // stop mirroring an app
                Method::DELETE => {
                    let resp = Request::new()
                        .target(downloads)
                        .body(serde_json::to_vec(&DownloadRequests::StopMirroring(
                            crate::kinode::process::main::PackageId::from_process_lib(package_id),
                        ))?)
                        .send_and_await_response(5)??;
                    let msg = serde_json::from_slice::<DownloadResponses>(resp.body())?;
                    match msg {
                        DownloadResponses::Success => Ok((StatusCode::OK, None, vec![])),
                        DownloadResponses::Err(e) => {
                            Err(anyhow::anyhow!("Error stopping mirroring: {:?}", e))
                        }
                        _ => Err(anyhow::anyhow!(
                            "Invalid response from downloads: {:?}",
                            msg
                        )),
                    }
                }
                _ => Ok((
                    StatusCode::METHOD_NOT_ALLOWED,
                    None,
                    format!("Invalid method {method} for {bound_path}").into_bytes(),
                )),
            }
        }
        // remove a downloaded app: POST
        "/downloads/:id/remove" => {
            let Ok(package_id) = get_package_id(url_params) else {
                return Ok((
                    StatusCode::BAD_REQUEST,
                    None,
                    format!("Missing id").into_bytes(),
                ));
            };
            let body = crate::get_blob()
                .ok_or(anyhow::anyhow!("missing blob"))?
                .bytes;
            let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let version_hash = body_json
                .get("version_hash")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("No version_hash specified!"))?;
            let download_request = DownloadRequests::RemoveFile(RemoveFileRequest {
                package_id: crate::kinode::process::main::PackageId::from_process_lib(package_id),
                version_hash: version_hash,
            });

            let resp = Request::to(("our", "downloads", "app_store", "sys"))
                .body(serde_json::to_vec(&download_request)?)
                .send_and_await_response(5)??;
            let msg = serde_json::from_slice::<DownloadResponses>(resp.body())?;
            match msg {
                DownloadResponses::Success => Ok((StatusCode::OK, None, vec![])),
                DownloadResponses::Err(e) => Err(anyhow::anyhow!("Error removing file: {:?}", e)),
                _ => Err(anyhow::anyhow!(
                    "Invalid response from downloads: {:?}",
                    msg
                )),
            }
        }
        // start auto-updating a downloaded app: PUT
        // stop auto-updating a downloaded app: DELETE
        "/apps/:id/auto-update" => {
            let package_id = get_package_id(url_params)?;

            let chain_request = match method {
                Method::PUT => ChainRequests::StartAutoUpdate(
                    crate::kinode::process::main::PackageId::from_process_lib(package_id),
                ),
                Method::DELETE => ChainRequests::StopAutoUpdate(
                    crate::kinode::process::main::PackageId::from_process_lib(package_id),
                ),
                _ => {
                    return Ok((
                        StatusCode::METHOD_NOT_ALLOWED,
                        None,
                        format!("Invalid method {method} for {bound_path}").into_bytes(),
                    ))
                }
            };

            let resp = Request::to(("our", "chain", "app_store", "sys"))
                .body(serde_json::to_vec(&chain_request)?)
                .send_and_await_response(5)??;

            let msg = serde_json::from_slice::<ChainResponses>(resp.body())?;
            match msg {
                ChainResponses::AutoUpdateStarted
                | ChainResponses::AutoUpdateStopped
                | ChainResponses::Err(_) => Ok((StatusCode::OK, None, serde_json::to_vec(&msg)?)),
                _ => Ok((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    None,
                    format!("Invalid response from chain: {:?}", msg).into_bytes(),
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
