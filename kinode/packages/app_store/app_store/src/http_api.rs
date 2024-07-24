use crate::state::{PackageListing, State};
use crate::DownloadResponse;
use kinode_process_lib::{
    http::{
        bind_http_path, bind_ws_path, send_response, serve_ui, IncomingHttpRequest, Method,
        StatusCode,
    },
    Address, NodeId, PackageId, Request,
};
use serde_json::json;
use std::collections::HashMap;

const ICON: &str = include_str!("icon");

/// Bind static and dynamic HTTP paths for the app store,
/// bind to our WS updates path, and add icon and widget to homepage.
pub fn init_frontend(our: &Address) {
    for path in [
        "/apps",
        "/apps/:id",
        "/apps/:id/caps",
        "/apps/:id/mirror",
        "/apps/:id/auto-update",
        "/apps/rebuild-index",
    ] {
        bind_http_path(path, true, false).expect("failed to bind http path");
    }
    serve_ui(
        &our,
        "ui",
        true,
        false,
        vec!["/", "/my-apps", "/app-details/:id", "/publish"],
    )
    .expect("failed to serve static UI");

    bind_ws_path("/", true, true).expect("failed to bind ws path");

    // add ourselves to the homepage
    Request::to(("our", "homepage", "homepage", "sys"))
        .body(
            serde_json::json!({
                "Add": {
                    "label": "App Store",
                    "icon": ICON,
                    "path": "/",
                    "widget": make_widget()
                }
            })
            .to_string()
            .as_bytes()
            .to_vec(),
        )
        .send()
        .unwrap();
}

fn make_widget() -> String {
    return r#"<html>
<head>
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <style>
        * {
            box-sizing: border-box;
            margin: 0;
            padding: 0;
        }

        a {
            text-decoration: none;
            color: inherit;
        }

        body {
            color: white;
            overflow: hidden;
        }

        #latest-apps {
            display: flex;
            flex-wrap: wrap;
            padding: 0.5rem;
            gap: 0.5rem;
            align-items: center;
            backdrop-filter: saturate(1.25);
            border-radius: 1rem;
            box-shadow: 0 10px 15px -3px rgba(0, 0, 0, 0.1), 0 4px 6px -2px rgba(0, 0, 0, 0.05);
            height: 100vh;
            width: 100vw;
            overflow-y: auto;
            scrollbar-color: transparent transparent;
            scrollbar-width: none;
        }

        .app {
            padding: 0.5rem;
            display: flex;
            flex-grow: 1;
            align-items: stretch;
            border-radius: 0.5rem;
            box-shadow: 0 1px 2px 0 rgba(0, 0, 0, 0.05);
            background-color: rgba(255, 255, 255, 0.1);
            cursor: pointer;
            font-family: sans-serif;
            width: 100%;
        }

        .app:hover {
            background-color: rgba(255, 255, 255, 0.2);
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
<body class="text-white overflow-hidden">
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
                            a.href = `/main:app_store:sys/app-details/${app.package}:${app.publisher}`
                            a.target = '_blank';
                            a.rel = 'noopener noreferrer';
                            const iconLetter = app.metadata_hash.replace('0x', '')[0].toUpperCase();
                            a.innerHTML = `<div
                                class="app-image"
                                style="
                                    background-image: url('${app.metadata.image || `/icons/${iconLetter}`}');
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
/// - get some subset of listed apps, via search or filter: ?
/// - get detail about a specific app: GET /apps/:id
/// - get capabilities for a specific downloaded app: GET /apps/:id/caps
///
/// - install a downloaded app, download a listed app: POST /apps/:id
/// - uninstall/delete a downloaded app: DELETE /apps/:id
/// - update a downloaded app: PUT /apps/:id
/// - approve capabilities for a downloaded app: POST /apps/:id/caps
/// - start mirroring a downloaded app: PUT /apps/:id/mirror
/// - stop mirroring a downloaded app: DELETE /apps/:id/mirror
/// - start auto-updating a downloaded app: PUT /apps/:id/auto-update
/// - stop auto-updating a downloaded app: DELETE /apps/:id/auto-update
///
/// - RebuildIndex: POST /apps/rebuild-index
pub fn handle_http_request(state: &mut State, req: &IncomingHttpRequest) -> anyhow::Result<()> {
    match serve_paths(state, req) {
        Ok((status_code, _headers, body)) => send_response(
            status_code,
            Some(HashMap::from([(
                String::from("Content-Type"),
                String::from("application/json"),
            )])),
            body,
        ),
        Err(_e) => send_response(StatusCode::INTERNAL_SERVER_ERROR, None, vec![]),
    }

    Ok(())
}

fn get_package_id(url_params: &HashMap<String, String>) -> anyhow::Result<PackageId> {
    let Some(package_id) = url_params.get("id") else {
        return Err(anyhow::anyhow!("Missing id"));
    };

    let id = package_id.parse::<PackageId>()?;
    Ok(id)
}

fn gen_package_info(id: &PackageId, listing: &PackageListing) -> serde_json::Value {
    json!({
        "tba": listing.tba,
        "package": id.package().to_string(),
        "publisher": id.publisher(),
        "installed": match &listing.state {
            Some(state) => state.installed,
            None => false,
        },
        "metadata_hash": listing.metadata_hash,
        "metadata": listing.metadata,
        "state": match &listing.state {
            Some(state) => json!({
                "mirrored_from": state.mirrored_from,
                "our_version": state.our_version_hash,
                "caps_approved": state.caps_approved,
                "mirroring": state.mirroring,
                "auto_update": state.auto_update,
                "verified": state.verified,
            }),
            None => json!(null),
        },
    })
}

fn serve_paths(
    state: &mut State,
    req: &IncomingHttpRequest,
) -> anyhow::Result<(StatusCode, Option<HashMap<String, String>>, Vec<u8>)> {
    let method = req.method()?;

    let bound_path: &str = req.bound_path(Some(&state.our.process.to_string()));
    let url_params = req.url_params();

    match bound_path {
        // GET all apps
        "/apps" => {
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
        // GET detail about a specific app
        // install an app: POST
        // update a downloaded app: PUT
        // uninstall an app: DELETE
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
                Method::POST => {
                    let Some(listing) = state.packages.get(&package_id) else {
                        return Ok((
                            StatusCode::NOT_FOUND,
                            None,
                            format!("App not found: {package_id}").into_bytes(),
                        ));
                    };
                    if listing.state.is_some() {
                        // install a downloaded app
                        crate::handle_install(state, &package_id)?;
                        Ok((StatusCode::CREATED, None, format!("Installed").into_bytes()))
                    } else {
                        // download a listed app
                        let pkg_listing: &PackageListing = state
                            .packages
                            .get(&package_id)
                            .ok_or(anyhow::anyhow!("No package"))?;
                        // from POST body, look for download_from field and use that as the mirror
                        let body = crate::get_blob()
                            .ok_or(anyhow::anyhow!("missing blob"))?
                            .bytes;
                        let body_json: serde_json::Value =
                            serde_json::from_slice(&body).unwrap_or_default();
                        let mirrors: &Vec<NodeId> = pkg_listing
                            .metadata
                            .as_ref()
                            .expect("Package does not have metadata")
                            .properties
                            .mirrors
                            .as_ref();
                        let download_from = body_json
                            .get("download_from")
                            .unwrap_or(&json!(mirrors
                                .first()
                                .ok_or(anyhow::anyhow!("No mirrors for package {package_id}"))?))
                            .as_str()
                            .ok_or(anyhow::anyhow!("download_from not a string"))?
                            .to_string();
                        // TODO select on FE? or after download but before install?
                        let mirror = false;
                        let auto_update = false;
                        let desired_version_hash = None;
                        match crate::start_download(
                            state,
                            package_id,
                            download_from,
                            mirror,
                            auto_update,
                            desired_version_hash,
                        ) {
                            DownloadResponse::Started => Ok((
                                StatusCode::CREATED,
                                None,
                                format!("Downloading").into_bytes(),
                            )),
                            other => Ok((
                                StatusCode::SERVICE_UNAVAILABLE,
                                None,
                                format!("Failed to download: {other:?}").into_bytes(),
                            )),
                        }
                    }
                }
                Method::PUT => {
                    // update a downloaded app
                    let listing: &PackageListing = state
                        .packages
                        .get(&package_id)
                        .ok_or(anyhow::anyhow!("No package listing"))?;
                    let Some(ref pkg_state) = listing.state else {
                        return Err(anyhow::anyhow!("No package state"));
                    };
                    let download_from = pkg_state
                        .mirrored_from
                        .as_ref()
                        .ok_or(anyhow::anyhow!("No mirror for package {package_id}"))?
                        .to_string();
                    match crate::start_download(
                        state,
                        package_id,
                        download_from,
                        pkg_state.mirroring,
                        pkg_state.auto_update,
                        None,
                    ) {
                        DownloadResponse::Started => Ok((
                            StatusCode::CREATED,
                            None,
                            format!("Downloading").into_bytes(),
                        )),
                        _ => Ok((
                            StatusCode::SERVICE_UNAVAILABLE,
                            None,
                            format!("Failed to download").into_bytes(),
                        )),
                    }
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
                Method::POST => Ok(
                    match state.update_downloaded_package(&package_id, |pkg| {
                        pkg.caps_approved = true;
                    }) {
                        true => (StatusCode::OK, None, vec![]),
                        false => (
                            StatusCode::NOT_FOUND,
                            None,
                            format!("App not found: {package_id}").into_bytes(),
                        ),
                    },
                ),
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

            match method {
                // start mirroring an app
                Method::PUT => {
                    state.start_mirroring(&package_id);
                    Ok((StatusCode::OK, None, vec![]))
                }
                // stop mirroring an app
                Method::DELETE => {
                    state.stop_mirroring(&package_id);
                    Ok((StatusCode::OK, None, vec![]))
                }
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
                Method::PUT => {
                    state.start_auto_update(&package_id);
                    Ok((StatusCode::OK, None, vec![]))
                }
                // stop auto-updating an app
                Method::DELETE => {
                    state.stop_auto_update(&package_id);
                    Ok((StatusCode::OK, None, vec![]))
                }
                _ => Ok((
                    StatusCode::METHOD_NOT_ALLOWED,
                    None,
                    format!("Invalid method {method} for {bound_path}").into_bytes(),
                )),
            }
        }
        // RebuildIndex: POST
        "/apps/rebuild-index" => {
            if method != Method::POST {
                return Ok((
                    StatusCode::METHOD_NOT_ALLOWED,
                    None,
                    format!("Invalid method {method} for {bound_path}").into_bytes(),
                ));
            }
            crate::rebuild_index(state);
            Ok((StatusCode::OK, None, vec![]))
        }
        _ => Ok((
            StatusCode::NOT_FOUND,
            None,
            format!("Path not found: {bound_path}").into_bytes(),
        )),
    }
}
