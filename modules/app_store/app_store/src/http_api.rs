use crate::{OnchainPackageMetadata, PackageListing, State};
use kinode_process_lib::{
    eth::EthAddress,
    http::{send_response, IncomingHttpRequest, StatusCode},
    Address,
};

pub fn handle_http_request(
    our: &Address,
    state: &mut State,
    req: &IncomingHttpRequest,
) -> anyhow::Result<()> {
    let path = req.path()?;
    let method = req.method()?;

    let (status_code, headers, body) = match path.as_str() {
        "/apps" => {
            match method.as_str() {
                "GET" => {
                    // TODO: Return a list of the user's apps
                    (
                        StatusCode::OK,
                        None,
                        serde_json::to_vec(&vec![
                            PackageListing {
                                owner: EthAddress::zero(),
                                name: "chess".to_string(),
                                publisher: our.node.clone(),
                                metadata_hash: "0x0".to_string(),
                                metadata: Some(OnchainPackageMetadata {
                                    name: Some("Chess".to_string()),
                                    description: Some("A test app".to_string()),
                                    image: None,
                                    version: None,
                                    license: None,
                                    website: Some("https://example.com".to_string()),
                                    mirrors: vec![],
                                    versions: vec![],
                                }),
                            },
                            PackageListing {
                                owner: EthAddress::zero(),
                                name: "file_transfer".to_string(),
                                publisher: our.node.clone(),
                                metadata_hash: "0x0".to_string(),
                                metadata: Some(OnchainPackageMetadata {
                                    name: Some("Kino Files".to_string()),
                                    description: Some("A test app".to_string()),
                                    image: None,
                                    version: None,
                                    license: None,
                                    website: Some("https://example.com".to_string()),
                                    mirrors: vec![],
                                    versions: vec![],
                                }),
                            },
                        ])?,
                    )
                }
                "POST" => {
                    // Add an app
                    (StatusCode::CREATED, None, format!("Installed").into_bytes())
                }
                _ => (
                    StatusCode::METHOD_NOT_ALLOWED,
                    None,
                    format!("Invalid method {} for {}", method, path).into_bytes(),
                ),
            }
        }
        "/apps/:id" => {
            let Some(app_id) = path.split("/").last() else {
                return Err(anyhow::anyhow!("No app ID"));
            };

            match method.as_str() {
                "PUT" => {
                    // Update an app
                    (
                        StatusCode::NO_CONTENT,
                        None,
                        format!("Updated").into_bytes(),
                    )
                }
                "DELETE" => {
                    // Uninstall an app
                    (
                        StatusCode::NO_CONTENT,
                        None,
                        format!("Uninstalled").into_bytes(),
                    )
                }
                _ => (
                    StatusCode::METHOD_NOT_ALLOWED,
                    None,
                    format!("Invalid method {} for {}", method, path).into_bytes(),
                ),
            }
        }
        "/apps/latest" => {
            match method.as_str() {
                "GET" => {
                    // Return a list of latest apps
                    // The first 2 will show up in "featured"
                    (
                        StatusCode::OK,
                        None,
                        serde_json::to_vec(&vec![
                            PackageListing {
                                owner: EthAddress::zero(),
                                name: "remote".to_string(),
                                publisher: our.node.clone(),
                                metadata_hash: "0x0".to_string(),
                                metadata: Some(OnchainPackageMetadata {
                                    name: Some("Remote".to_string()),
                                    description: Some("A test app".to_string()),
                                    image: None,
                                    version: None,
                                    license: None,
                                    website: Some("https://example.com".to_string()),
                                    mirrors: vec![],
                                    versions: vec![],
                                }),
                            },
                            PackageListing {
                                owner: EthAddress::zero(),
                                name: "happy_path".to_string(),
                                publisher: our.node.clone(),
                                metadata_hash: "0x0".to_string(),
                                metadata: Some(OnchainPackageMetadata {
                                    name: Some("Happy Path".to_string()),
                                    description: Some("A test app".to_string()),
                                    image: None,
                                    version: None,
                                    license: None,
                                    website: Some("https://example.com".to_string()),
                                    mirrors: vec![],
                                    versions: vec![],
                                }),
                            },
                            PackageListing {
                                owner: EthAddress::zero(),
                                name: "meme_deck".to_string(),
                                publisher: our.node.clone(),
                                metadata_hash: "0x0".to_string(),
                                metadata: Some(OnchainPackageMetadata {
                                    name: Some("Meme Deck".to_string()),
                                    description: Some("A test app".to_string()),
                                    image: None,
                                    version: None,
                                    license: None,
                                    website: Some("https://example.com".to_string()),
                                    mirrors: vec![],
                                    versions: vec![],
                                }),
                            },
                            PackageListing {
                                owner: EthAddress::zero(),
                                name: "sheep_simulator".to_string(),
                                publisher: our.node.clone(),
                                metadata_hash: "0x0".to_string(),
                                metadata: Some(OnchainPackageMetadata {
                                    name: Some("Sheep Simulator".to_string()),
                                    description: Some("A test app".to_string()),
                                    image: None,
                                    version: None,
                                    license: None,
                                    website: Some("https://example.com".to_string()),
                                    mirrors: vec![],
                                    versions: vec![],
                                }),
                            },
                        ])?,
                    )
                }
                _ => (
                    StatusCode::METHOD_NOT_ALLOWED,
                    None,
                    format!("Invalid method {} for {}", method, path).into_bytes(),
                ),
            }
        }
        "/apps/search/:query" => {
            match method.as_str() {
                "GET" => {
                    let Some(encoded_query) = path.split("/").last() else {
                        return Err(anyhow::anyhow!("No query"));
                    };
                    let query = urlencoding::decode(encoded_query).expect("UTF-8");

                    // Return a list of apps matching the query
                    // Query by name, publisher, package_name, description, website
                    (
                        StatusCode::OK,
                        None,
                        serde_json::to_vec(&vec![
                            PackageListing {
                                owner: EthAddress::zero(),
                                name: "winch".to_string(),
                                publisher: our.node.clone(),
                                metadata_hash: "0x0".to_string(),
                                metadata: Some(OnchainPackageMetadata {
                                    name: Some("Winch".to_string()),
                                    description: Some("A test app".to_string()),
                                    image: None,
                                    version: None,
                                    license: None,
                                    website: Some("https://example.com".to_string()),
                                    mirrors: vec![],
                                    versions: vec![],
                                }),
                            },
                            PackageListing {
                                owner: EthAddress::zero(),
                                name: "bucket".to_string(),
                                publisher: our.node.clone(),
                                metadata_hash: "0x0".to_string(),
                                metadata: Some(OnchainPackageMetadata {
                                    name: Some("Bucket".to_string()),
                                    description: Some("A test app".to_string()),
                                    image: None,
                                    version: None,
                                    license: None,
                                    website: Some("https://example.com".to_string()),
                                    mirrors: vec![],
                                    versions: vec![],
                                }),
                            },
                        ])?,
                    )
                }
                _ => (
                    StatusCode::METHOD_NOT_ALLOWED,
                    None,
                    format!("Invalid method {} for {}", method, path).into_bytes(),
                ),
            }
        }
        "/apps/publish" => {
            match method.as_str() {
                "POST" => {
                    // Publish an app
                    (StatusCode::OK, None, format!("Success").into_bytes())
                }
                _ => (
                    StatusCode::METHOD_NOT_ALLOWED,
                    None,
                    format!("Invalid method {} for {}", method, path).into_bytes(),
                ),
            }
        }
        _ => (
            StatusCode::NOT_FOUND,
            None,
            format!("Path not found: {}", path).into_bytes(),
        ),
    };

    send_response(status_code, headers, body)
}
