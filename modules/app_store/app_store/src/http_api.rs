use std::collections::HashMap;

use kinode_process_lib::{http::{send_response, IncomingHttpRequest, StatusCode}, Address};

use crate::{PackageListing, State};

pub fn handle_http_request(
  our: &Address,
  state: &mut State,
  req: IncomingHttpRequest,
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
                              owner: our.node.clone(),
                              publisher: our.node.clone(),
                              name: "Chess".to_string(),
                              icon: "".to_string(),
                              package_name: "chess".to_string(),
                              description: Some("A test app".to_string()),
                              website: Some("https://example.com".to_string()),
                              rating: 3.0,
                              versions: HashMap::new(),
                              mirrors: vec![],
                          },
                          PackageListing {
                              owner: our.node.clone(),
                              publisher: our.node.clone(),
                              name: "File Transfer".to_string(),
                              icon: "".to_string(),
                              package_name: "file_transfer".to_string(),
                              description: Some("A test app".to_string()),
                              website: Some("https://example.com".to_string()),
                              rating: 3.0,
                              versions: HashMap::new(),
                              mirrors: vec![],
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
                              owner: our.node.clone(),
                              publisher: our.node.clone(),
                              name: "Remote".to_string(),
                              icon: "".to_string(),
                              package_name: "remote".to_string(),
                              description: Some("A test app".to_string()),
                              website: Some("https://example.com".to_string()),
                              rating: 3.0,
                              versions: HashMap::new(),
                              mirrors: vec![],
                          },
                          PackageListing {
                              owner: our.node.clone(),
                              publisher: our.node.clone(),
                              name: "Happy Path".to_string(),
                              icon: "".to_string(),
                              package_name: "happy_path".to_string(),
                              description: Some("A test app".to_string()),
                              website: Some("https://example.com".to_string()),
                              rating: 3.0,
                              versions: HashMap::new(),
                              mirrors: vec![],
                          },
                          PackageListing {
                              owner: our.node.clone(),
                              publisher: our.node.clone(),
                              name: "Meme Deck".to_string(),
                              icon: "".to_string(),
                              package_name: "meme_deck".to_string(),
                              description: Some("A test app".to_string()),
                              website: Some("https://example.com".to_string()),
                              rating: 3.0,
                              versions: HashMap::new(),
                              mirrors: vec![],
                          },
                          PackageListing {
                              owner: our.node.clone(),
                              publisher: our.node.clone(),
                              name: "Sheep Simulator".to_string(),
                              icon: "".to_string(),
                              package_name: "sheep_simulator".to_string(),
                              description: Some("A test app".to_string()),
                              website: Some("https://example.com".to_string()),
                              rating: 3.0,
                              versions: HashMap::new(),
                              mirrors: vec![],
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
                              owner: our.node.clone(),
                              publisher: our.node.clone(),
                              name: "Winch".to_string(),
                              icon: "".to_string(),
                              package_name: "winch".to_string(),
                              description: Some("A test app".to_string()),
                              website: Some("https://example.com".to_string()),
                              rating: 3.0,
                              versions: HashMap::new(),
                              mirrors: vec![],
                          },
                          PackageListing {
                              owner: our.node.clone(),
                              publisher: our.node.clone(),
                              name: "Bucket".to_string(),
                              icon: "".to_string(),
                              package_name: "bucket".to_string(),
                              description: Some("A test app".to_string()),
                              website: Some("https://example.com".to_string()),
                              rating: 3.0,
                              versions: HashMap::new(),
                              mirrors: vec![],
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

  send_response(status_code, headers, body)?;

  Ok(())
}
