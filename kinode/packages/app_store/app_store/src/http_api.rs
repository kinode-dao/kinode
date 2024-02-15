use crate::{DownloadResponse, PackageListing, PackageState, RequestedPackage, State};
use kinode_process_lib::{
    http::{send_response, IncomingHttpRequest, Method, StatusCode},
    print_to_terminal, Address, NodeId, PackageId,
};
use serde_json::json;
use std::collections::HashMap;

/// Actions supported over HTTP:
/// - get all downloaded apps: GET /apps
/// - get all listed apps: GET /apps/listed
/// - get some subset of listed apps, via search or filter: ?
/// - get detail about a specific downloaded app: GET /apps/:id
/// - get capabilities for a specific downloaded app: GET /apps/:id/caps
/// - get detail about a specific listed app: GET /apps/listed/:id
///
/// - download a listed app: POST /apps/listed/:id
/// - install a downloaded app: POST /apps/:id
/// - uninstall/delete a downloaded app: DELETE /apps/:id
/// - update a downloaded app: PUT /apps/:id
/// - approve capabilities for a downloaded app: POST /apps/:id/caps
/// - start mirroring a downloaded app: PUT /apps/:id/mirror
/// - stop mirroring a downloaded app: DELETE /apps/:id/mirror
/// - start auto-updating a downloaded app: PUT /apps/:id/auto-update
/// - stop auto-updating a downloaded app: DELETE /apps/:id/auto-update
pub fn handle_http_request(
    our: &Address,
    state: &mut State,
    requested_packages: &mut HashMap<PackageId, RequestedPackage>,
    req: &IncomingHttpRequest,
) -> anyhow::Result<()> {
    match serve_paths(our, state, requested_packages, req) {
        Ok((status_code, _headers, body)) => send_response(
            status_code,
            Some(HashMap::from([(
                String::from("Content-Type"),
                String::from("application/json"),
            )])),
            body,
        ),
        Err(e) => {
            print_to_terminal(1, &format!("http error: {:?}", e));
            send_response(StatusCode::INTERNAL_SERVER_ERROR, None, vec![])
        }
    }

    Ok(())
}

fn gen_package_info(
    id: &PackageId,
    listing: Option<&PackageListing>,
    state: Option<&PackageState>,
) -> serde_json::Value {
    json!({
        "owner": match &listing {
            Some(listing) => Some(&listing.owner),
            None => None,
        },
        "package": id.package().to_string(),
        "publisher": id.publisher(),
        "installed": match &state {
            Some(state) => state.installed,
            None => false,
        },
        "metadata_hash": match &listing {
            Some(listing) => Some(&listing.metadata_hash),
            None => None,
        },
        "metadata": match &listing {
            Some(listing) => Some(&listing.metadata),
            None => match state {
                Some(state) => Some(&state.metadata),
                None => None,
            },
        },
        "state": match &state {
            Some(state) => json!({
                "mirrored_from": state.mirrored_from,
                "our_version": state.our_version,
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
    our: &Address,
    state: &mut State,
    requested_packages: &mut HashMap<PackageId, RequestedPackage>,
    req: &IncomingHttpRequest,
) -> anyhow::Result<(StatusCode, Option<HashMap<String, String>>, Vec<u8>)> {
    let path = req.path()?;
    let method = req.method()?;

    // TODO get rid of this workaround when we change `IncomingHttpRequest`
    let bound_path: &str = if path.ends_with("auto-update") {
        "/apps/:id/auto-update"
    } else if path.ends_with("mirror") {
        "/apps/:id/mirror"
    } else if path.ends_with("caps") {
        "/apps/:id/caps"
    } else if path.starts_with("/apps/listed/") {
        "/apps/listed/:id"
    } else if &path == "/apps/listed" || &path == "/apps" {
        &path
    } else {
        "/apps/:id"
    };

    // print_to_terminal(0, &format!("HTTP {method} {path} {bound_path}"));

    match bound_path {
        // GET all downloaded apps
        "/apps" => {
            if method != Method::GET {
                return Ok((
                    StatusCode::METHOD_NOT_ALLOWED,
                    None,
                    format!("Invalid method {method} for {path}").into_bytes(),
                ));
            }
            let all: Vec<serde_json::Value> = state
                .downloaded_packages
                .iter()
                .map(|(package_id, package_state)| {
                    let listing = state.get_listing(package_id);
                    gen_package_info(package_id, listing, Some(package_state))
                })
                .collect();
            return Ok((StatusCode::OK, None, serde_json::to_vec(&all)?));
        }
        // GET all listed apps
        "/apps/listed" => {
            if method != Method::GET {
                return Ok((
                    StatusCode::METHOD_NOT_ALLOWED,
                    None,
                    format!("Invalid method {method} for {path}").into_bytes(),
                ));
            }
            let all: Vec<serde_json::Value> = state
                .listed_packages
                .iter()
                .map(|(_hash, listing)| {
                    let package_id = PackageId::new(&listing.name, &listing.publisher);
                    let state = state.downloaded_packages.get(&package_id);
                    gen_package_info(&package_id, Some(listing), state)
                })
                .collect();
            return Ok((StatusCode::OK, None, serde_json::to_vec(&all)?));
        }
        // GET detail about a specific downloaded app
        // install a downloaded app: POST
        // update a downloaded app: PUT
        // uninstall/delete a downloaded app: DELETE
        "/apps/:id" => {
            let package_id = path
                .split("/")
                .last()
                .unwrap_or_default()
                .parse::<PackageId>()?;
            match method {
                Method::GET => {
                    let Some(pkg) = state.downloaded_packages.get(&package_id) else {
                        return Ok((
                            StatusCode::NOT_FOUND,
                            None,
                            format!("App not found: {package_id}").into_bytes(),
                        ));
                    };
                    let listing = state.get_listing(&package_id);
                    Ok((
                        StatusCode::OK,
                        None,
                        gen_package_info(&package_id, listing, Some(pkg))
                            .to_string()
                            .into_bytes(),
                    ))
                }
                Method::POST => {
                    // install an app
                    crate::handle_install(our, state, &package_id)?;
                    Ok((StatusCode::CREATED, None, format!("Installed").into_bytes()))
                }
                Method::PUT => {
                    // update an app
                    // TODO
                    Ok((StatusCode::NO_CONTENT, None, format!("TODO").into_bytes()))
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
                    format!("Invalid method {method} for {path}").into_bytes(),
                )),
            }
        }
        // GET detail about a specific listed app
        // download a listed app: POST
        "/apps/listed/:id" => {
            let package_id = path
                .split("/")
                .last()
                .unwrap_or_default()
                .parse::<PackageId>()?;
            match method {
                Method::GET => {
                    let Some(listing) = state.get_listing(&package_id) else {
                        return Ok((
                            StatusCode::NOT_FOUND,
                            None,
                            format!("App not found: {package_id}").into_bytes(),
                        ));
                    };
                    let downloaded = state.downloaded_packages.get(&package_id);
                    Ok((
                        StatusCode::OK,
                        None,
                        gen_package_info(&package_id, Some(listing), downloaded)
                            .to_string()
                            .into_bytes(),
                    ))
                }
                Method::POST => {
                    // download an app
                    // TODO get fields from POST body
                    let pkg_listing: &PackageListing = state
                        .get_listing(&package_id)
                        .ok_or(anyhow::anyhow!("No package"))?;
                    let mirrors: &Vec<NodeId> = pkg_listing
                        .metadata
                        .as_ref()
                        .expect("Package does not have metadata")
                        .properties
                        .mirrors
                        .as_ref();
                    // TODO select on FE
                    let download_from = mirrors
                        .first()
                        .ok_or(anyhow::anyhow!("No mirrors for package {package_id}"))?;
                    // TODO select on FE
                    let mirror = false;
                    let auto_update = false;
                    let desired_version_hash = None;
                    match crate::start_download(
                        our,
                        requested_packages,
                        &package_id,
                        download_from,
                        mirror,
                        auto_update,
                        &desired_version_hash,
                    ) {
                        DownloadResponse::Started => Ok((
                            StatusCode::CREATED,
                            None,
                            format!("Downloading").into_bytes(),
                        )),
                        DownloadResponse::Failure => Ok((
                            StatusCode::SERVICE_UNAVAILABLE,
                            None,
                            format!("Failed to download").into_bytes(),
                        )),
                    }
                }
                _ => Ok((
                    StatusCode::METHOD_NOT_ALLOWED,
                    None,
                    format!("Invalid method {method} for {path}").into_bytes(),
                )),
            }
        }
        // GET caps for a specific downloaded app
        // approve capabilities for a downloaded app: POST
        "/apps/:id/caps" => {
            let package_id = path
                .split("/")
                .nth(2)
                .unwrap_or_default()
                .parse::<PackageId>()?;
            match method {
                // return the capabilities for that app
                Method::GET => Ok(match crate::fetch_package_manifest(&package_id) {
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
                    format!("Invalid method {method} for {path}").into_bytes(),
                )),
            }
        }
        // start mirroring a downloaded app: PUT
        // stop mirroring a downloaded app: DELETE
        "/apps/:id/mirror" => {
            let package_id = path
                .split("/")
                .nth(2)
                .unwrap_or_default()
                .parse::<PackageId>()?;
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
                    format!("Invalid method {method} for {path}").into_bytes(),
                )),
            }
        }
        // start auto-updating a downloaded app: PUT
        // stop auto-updating a downloaded app: DELETE
        "/apps/:id/auto-update" => {
            let package_id = path
                .split("/")
                .nth(2)
                .unwrap_or_default()
                .parse::<PackageId>()?;
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
                    format!("Invalid method {method} for {path}").into_bytes(),
                )),
            }
        }
        _ => Ok((
            StatusCode::NOT_FOUND,
            None,
            format!("Path not found: {}", path).into_bytes(),
        )),
    }
}
