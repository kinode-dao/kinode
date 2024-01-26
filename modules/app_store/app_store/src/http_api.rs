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
                                    subtitle: Some("A test app".to_string()),
                                    description: Some("A very long description".to_string()),
                                    image: Some("https://www.freeiconspng.com/thumbs/chess-icon/chess-icon-28.png".to_string()),
                                    version: None,
                                    license: None,
                                    website: Some("https://example.com".to_string()),
                                    screenshots: vec![],
                                    mirrors: vec!["https://fakemirror1.com".to_string(), "https://fakemirror2.com".to_string()],
                                    versions: vec!["0.1.0".to_string(), "0.2.0".to_string()],
                                }),
                            },
                            PackageListing {
                                owner: EthAddress::zero(),
                                name: "file_transfer".to_string(),
                                publisher: our.node.clone(),
                                metadata_hash: "0x1".to_string(),
                                metadata: Some(OnchainPackageMetadata {
                                    name: Some("Kino Files".to_string()),
                                    subtitle: Some("A test app".to_string()),
                                    description: Some("A very long description".to_string()),
                                    image: Some("https://cdn-icons-png.flaticon.com/512/1037/1037316.png".to_string()),
                                    version: None,
                                    license: None,
                                    website: Some("https://example.com".to_string()),
                                    screenshots: vec![],
                                    mirrors: vec!["https://fakemirror1.com".to_string(), "https://fakemirror2.com".to_string()],
                                    versions: vec!["0.1.0".to_string(), "0.2.0".to_string()],
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
                                metadata_hash: "0x2".to_string(),
                                metadata: Some(OnchainPackageMetadata {
                                    name: Some("Remote".to_string()),
                                    subtitle: Some("A test app".to_string()),
                                    description: Some("A very long description".to_string()),
                                    image: Some("data:image/jpeg;base64,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".to_string()),
                                    version: None,
                                    license: None,
                                    website: Some("https://example.com".to_string()),
                                    screenshots: vec![],
                                    mirrors: vec!["https://fakemirror1.com".to_string(), "https://fakemirror2.com".to_string()],
                                    versions: vec!["0.1.0".to_string(), "0.2.0".to_string()],
                                }),
                            },
                            PackageListing {
                                owner: EthAddress::zero(),
                                name: "happy_path".to_string(),
                                publisher: our.node.clone(),
                                metadata_hash: "0x3".to_string(),
                                metadata: Some(OnchainPackageMetadata {
                                    name: Some("Happy Path".to_string()),
                                    subtitle: Some("A test app".to_string()),
                                    description: Some("A very long description".to_string()),
                                    image: Some("https://png.pngtree.com/element_our/20200610/ourmid/pngtree-tranquil-lake-image_2235547.jpg".to_string()),
                                    version: None,
                                    license: None,
                                    website: Some("https://example.com".to_string()),
                                    screenshots: vec![],
                                    mirrors: vec!["https://fakemirror1.com".to_string(), "https://fakemirror2.com".to_string()],
                                    versions: vec!["0.1.0".to_string(), "0.2.0".to_string()],
                                }),
                            },
                            PackageListing {
                                owner: EthAddress::zero(),
                                name: "meme_deck".to_string(),
                                publisher: our.node.clone(),
                                metadata_hash: "0x4".to_string(),
                                metadata: Some(OnchainPackageMetadata {
                                    name: Some("Meme Deck".to_string()),
                                    subtitle: Some("A test app".to_string()),
                                    description: Some("A very long description".to_string()),
                                    image: Some("https://play-lh.googleusercontent.com/EsrYxR7S7ozFYA9Wn9tcmWkdrsjRIl1OZsmK1UhAvBanc6CozQ7X9sAc8gPttcGys8GC".to_string()),
                                    version: None,
                                    license: None,
                                    website: Some("https://example.com".to_string()),
                                    screenshots: vec![],
                                    mirrors: vec!["https://fakemirror1.com".to_string(), "https://fakemirror2.com".to_string()],
                                    versions: vec!["0.1.0".to_string(), "0.2.0".to_string()],
                                }),
                            },
                            PackageListing {
                                owner: EthAddress::zero(),
                                name: "sheep_simulator".to_string(),
                                publisher: our.node.clone(),
                                metadata_hash: "0x5".to_string(),
                                metadata: Some(OnchainPackageMetadata {
                                    name: Some("Sheep Simulator".to_string()),
                                    subtitle: Some("A test app".to_string()),
                                    description: Some("A very long description".to_string()),
                                    image: Some("https://www.freeiconspng.com/thumbs/sheep-png/sheep-png-29.png".to_string()),
                                    version: None,
                                    license: None,
                                    website: Some("https://example.com".to_string()),
                                    screenshots: vec![],
                                    mirrors: vec!["https://fakemirror1.com".to_string(), "https://fakemirror2.com".to_string()],
                                    versions: vec!["0.1.0".to_string(), "0.2.0".to_string()],
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
                                metadata_hash: "0x6".to_string(),
                                metadata: Some(OnchainPackageMetadata {
                                    name: Some("Winch".to_string()),
                                    subtitle: Some("A test app".to_string()),
                                    description: Some("A very long description".to_string()),
                                    image: Some("data:image/jpeg;base64,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".to_string()),
                                    version: None,
                                    license: None,
                                    website: Some("https://example.com".to_string()),
                                    screenshots: vec![],
                                    mirrors: vec!["https://fakemirror1.com".to_string(), "https://fakemirror2.com".to_string()],
                                    versions: vec!["0.1.0".to_string(), "0.2.0".to_string()],
                                }),
                            },
                            PackageListing {
                                owner: EthAddress::zero(),
                                name: "bucket".to_string(),
                                publisher: our.node.clone(),
                                metadata_hash: "0x7".to_string(),
                                metadata: Some(OnchainPackageMetadata {
                                    name: Some("Bucket".to_string()),
                                    subtitle: Some("A test app".to_string()),
                                    description: Some("A very long description".to_string()),
                                    image: Some("https://banner2.cleanpng.com/20171218/edc/iron-bucket-png-image-5a376a125f3243.4048600615135810743899.jpg".to_string()),
                                    version: None,
                                    license: None,
                                    website: Some("https://example.com".to_string()),
                                    screenshots: vec![],
                                    mirrors: vec!["https://fakemirror1.com".to_string(), "https://fakemirror2.com".to_string()],
                                    versions: vec!["0.1.0".to_string(), "0.2.0".to_string()],
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
