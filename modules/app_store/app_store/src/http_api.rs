use crate::{OnchainPackageMetadata, PackageListing, PackageState, State};
use kinode_process_lib::{
    eth::EthAddress,
    http::{send_response, IncomingHttpRequest, Method, StatusCode},
    Address, PackageId,
};
use sha3::digest::generic_array::arr::Inc;
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
    req: &IncomingHttpRequest,
) -> anyhow::Result<()> {
    match serve_paths(state, req) {
        Ok((status_code, headers, body)) => send_response(
            status_code,
            Some(HashMap::from([(
                String::from("Content-Type"),
                String::from("application/json"),
            )])),
            body,
        ),
        Err(e) => {
            crate::print_to_terminal(1, &format!("http error: {:?}", e));
            send_response(StatusCode::INTERNAL_SERVER_ERROR, None, vec![])
        }
    }
}

fn serve_paths(
    state: &mut State,
    req: &IncomingHttpRequest,
) -> anyhow::Result<(StatusCode, Option<HashMap<String, String>>, Vec<u8>)> {
    let path = req.path()?;
    let method = req.method()?;
    match path.as_str() {
        // GET all downloaded apps
        "/apps" => {
            if method != Method::GET {
                return Ok((
                    StatusCode::METHOD_NOT_ALLOWED,
                    None,
                    format!("Invalid method {method} for {path}").into_bytes(),
                ));
            }
            return Ok((
                StatusCode::OK,
                None,
                serde_json::to_vec(
                    &state.get_downloaded_packages_info()
                )?,
            ));
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
            return Ok((
                StatusCode::OK,
                None,
                serde_json::to_vec(&state.get_listed_packages_info())?,
            ));
        }
        // GET detail about a specific downloaded app
        // install a downloaded app: POST
        // update a downloaded app: PUT
        // uninstall/delete a downloaded app: DELETE
        "/apps/:id" => {
            let Ok(package_id) = path.split("/").last().unwrap_or_default().parse::<PackageId>() else {
                return Err(anyhow::anyhow!("No app ID"));
            };
            match method {
                Method::GET => Ok(match state.get_package_info(&package_id) {
                    Some(pkg) => (StatusCode::OK, None, serde_json::to_vec(&pkg)?),
                    None => (
                        StatusCode::NOT_FOUND,
                        None,
                        format!("App not found: {package_id}").into_bytes(),
                    ),
                }),
                Method::POST => {
                    // install an app
                    Ok((
                        StatusCode::NO_CONTENT,
                        None,
                        format!("Installed").into_bytes(),
                    ))
                }
                Method::PUT => {
                    // update an app
                    Ok((
                        StatusCode::NO_CONTENT,
                        None,
                        format!("Updated").into_bytes(),
                    ))
                }
                Method::DELETE => {
                    // uninstall an app
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
            let Ok(package_id) = path.split("/").last().unwrap_or_default().parse::<PackageId>() else {
                return Err(anyhow::anyhow!("No app ID"));
            };
            match method {
                Method::GET => Ok(match state.get_package_info(&package_id) {
                    Some(pkg) => (StatusCode::OK, None, serde_json::to_vec(&pkg)?),
                    None => (
                        StatusCode::NOT_FOUND,
                        None,
                        format!("App not found: {package_id}").into_bytes(),
                    ),
                }),
                Method::POST => {
                    // download an app
                    // TODO
                    Ok((
                        StatusCode::NO_CONTENT,
                        None,
                        format!("Downloaded").into_bytes(),
                    ))
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
            let Ok(package_id) = path.split("/").nth(1).unwrap_or_default().parse::<PackageId>() else {
                return Err(anyhow::anyhow!("No app ID"));
            };
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
            let Ok(package_id) = path.split("/").nth(1).unwrap_or_default().parse::<PackageId>() else {
                return Err(anyhow::anyhow!("No app ID"));
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
                    format!("Invalid method {method} for {path}").into_bytes(),
                )),
            }
        }
        // start auto-updating a downloaded app: PUT
        // stop auto-updating a downloaded app: DELETE
        "/apps/:id/auto-update" => {
            let Ok(package_id) = path.split("/").nth(1).unwrap_or_default().parse::<PackageId>() else {
                return Err(anyhow::anyhow!("No app ID"));
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
