use hmac::{Hmac, Mac};
use jwt::VerifyWithKey;
use lib::{core::ProcessId, types::http_server};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashMap;
use tokio::net::TcpListener;
use warp::http::{header::HeaderName, header::HeaderValue, HeaderMap};

#[derive(Serialize, Deserialize)]
pub struct RpcMessage {
    pub node: Option<String>,
    pub process: String,
    pub inherit: Option<bool>,
    pub expects_response: Option<u64>,
    pub body: Option<String>,
    pub metadata: Option<String>,
    pub context: Option<String>,
    pub mime: Option<String>,
    pub data: Option<String>,
}

pub fn auth_token_valid(
    our_node: &str,
    subdomain: Option<&ProcessId>,
    auth_token: &str,
    jwt_secret: &[u8],
) -> bool {
    let token: Vec<&str> = auth_token.split("; ").collect();

    let token_label = match subdomain {
        None => format!("hyperware-auth_{our_node}"),
        Some(subdomain) => format!("hyperware-auth_{our_node}@{subdomain}"),
    };

    let mut auth_token = None;
    for entry in token {
        let token_parts: Vec<&str> = entry.split('=').collect();
        if token_parts.len() == 2 && token_parts[0] == token_label {
            auth_token = Some(token_parts[1].to_string());
            break;
        }
    }

    let auth_token = match auth_token {
        Some(token) if !token.is_empty() => token,
        _ => return false,
    };

    let Ok(secret) = Hmac::<Sha256>::new_from_slice(jwt_secret) else {
        return false;
    };

    // Verify JWT structure (header.payload.signature) before attempting to decode
    let jwt_format =
        regex::Regex::new(r"^[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$").unwrap();
    if !jwt_format.is_match(&auth_token) {
        return false;
    }

    let claims: Result<http_server::JwtClaims, _> = auth_token.verify_with_key(&secret);

    match claims {
        Ok(data) => {
            data.username == our_node
                && data.subdomain == subdomain.map(|s| s.to_string())
                && data.expiration > chrono::Utc::now().timestamp() as u64
        }
        Err(_) => false,
    }
}

pub fn normalize_path(path: &str) -> &str {
    match path.strip_suffix('/') {
        Some(new) => new,
        None => path,
    }
}

pub fn format_path_with_process(process: &ProcessId, path: &str) -> String {
    let process = process.to_string();
    if process != "homepage:homepage:sys" {
        if path.starts_with('/') {
            format!("/{}{}", process, normalize_path(path))
        } else {
            format!("/{}/{}", process, normalize_path(path))
        }
    } else {
        normalize_path(path).to_string()
    }
}

/// first strip the process name leaving just package ID, then
/// convert all non-alphanumeric characters in the process ID to `-`
pub fn generate_secure_subdomain(process: &ProcessId) -> String {
    [process.package(), process.publisher()]
        .join("-")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

pub fn serialize_headers(headers: &HeaderMap) -> HashMap<String, String> {
    let mut hashmap = HashMap::new();
    for (key, value) in headers.iter() {
        let key_str = key.to_string();
        let value_str = value.to_str().unwrap_or("").to_string();
        hashmap.insert(key_str, value_str);
    }
    hashmap
}

pub fn deserialize_headers(hashmap: HashMap<String, String>) -> HeaderMap {
    let mut header_map = HeaderMap::new();
    for (key, value) in hashmap {
        let key_bytes = key.as_bytes();
        let Ok(key_name) = HeaderName::from_bytes(key_bytes) else {
            continue;
        };
        let Ok(value_header) = HeaderValue::from_str(&value) else {
            continue;
        };
        header_map.insert(key_name, value_header);
    }
    header_map
}

pub async fn find_open_port(start_at: u16, end_at: u16) -> Option<TcpListener> {
    for port in start_at..end_at {
        let bind_addr = format!("0.0.0.0:{}", port);
        if let Some(bound) = is_port_available(&bind_addr).await {
            return Some(bound);
        }
    }
    None
}

pub async fn is_port_available(bind_addr: &str) -> Option<TcpListener> {
    TcpListener::bind(bind_addr).await.ok()
}

pub fn _binary_encoded_string_to_bytes(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u8).collect()
}
