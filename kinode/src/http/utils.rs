use hmac::{Hmac, Mac};
use jwt::VerifyWithKey;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashMap;
use tokio::net::TcpListener;
use warp::http::{header::HeaderName, header::HeaderValue, HeaderMap};

use lib::types::http_server::*;

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

/// Ingest an auth token given from client and return the node name or an error.
pub fn _verify_auth_token(auth_token: &str, jwt_secret: &[u8]) -> Result<String, jwt::Error> {
    let Ok(secret) = Hmac::<Sha256>::new_from_slice(jwt_secret) else {
        return Err(jwt::Error::Format);
    };

    println!("hello\r");

    let claims: Result<JwtClaims, jwt::Error> = auth_token.verify_with_key(&secret);

    match claims {
        Ok(data) => Ok(data.username),
        Err(err) => Err(err),
    }
}

pub fn auth_cookie_valid(our_node: &str, cookie: &str, jwt_secret: &[u8]) -> bool {
    let cookie_parts: Vec<&str> = cookie.split("; ").collect();
    let mut auth_token = None;

    for cookie_part in cookie_parts {
        let cookie_part_parts: Vec<&str> = cookie_part.split('=').collect();
        if cookie_part_parts.len() == 2
            && cookie_part_parts[0] == format!("kinode-auth_{}", our_node)
        {
            auth_token = Some(cookie_part_parts[1].to_string());
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

    let claims: Result<JwtClaims, _> = auth_token.verify_with_key(&secret);

    match claims {
        Ok(data) => data.username == our_node,
        Err(_) => false,
    }
}

pub fn normalize_path(path: &str) -> String {
    match path.strip_suffix('/') {
        Some(new) => new.to_string(),
        None => path.to_string(),
    }
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

pub async fn find_open_port(start_at: u16, end_at: u16) -> Option<u16> {
    for port in start_at..end_at {
        let bind_addr = format!("0.0.0.0:{}", port);
        if is_port_available(&bind_addr).await {
            return Some(port);
        }
    }
    None
}

pub async fn is_port_available(bind_addr: &str) -> bool {
    TcpListener::bind(bind_addr).await.is_ok()
}

pub fn _binary_encoded_string_to_bytes(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u8).collect()
}
