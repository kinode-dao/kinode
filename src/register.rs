use aes_gcm::aead::KeyInit;
use hmac::Hmac;
use jwt::SignWithKey;
use ring::pkcs8::Document;
use ring::rand::SystemRandom;
use ring::signature;
use sha2::Sha256;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};
use warp::{
    http::header::{HeaderValue, SET_COOKIE},
    Filter, Rejection, Reply,
};

use crate::http_server;
use crate::keygen;
use crate::types::*;

type RegistrationSender = mpsc::Sender<(Identity, String, Document, Vec<u8>)>;

pub fn generate_jwt(jwt_secret_bytes: &[u8], username: String) -> Option<String> {
    let jwt_secret: Hmac<Sha256> = match Hmac::new_from_slice(&jwt_secret_bytes) {
        Ok(secret) => secret,
        Err(_) => return None,
    };

    let claims = JwtClaims {
        username: username.clone(),
        expiration: 0,
    };

    match claims.sign_with_key(&jwt_secret) {
        Ok(token) => Some(token),
        Err(_) => None,
    }
}

/// Serve the registration page and receive POSTs and PUTs from it
pub async fn register(
    tx: RegistrationSender,
    kill_rx: oneshot::Receiver<bool>,
    ip: String,
    port: u16,
    redir_port: u16,
) {
    let our = Arc::new(Mutex::new(None));
    let networking_keypair = Arc::new(Mutex::new(None));

    let our_get = our.clone();
    let networking_keypair_post = networking_keypair.clone();

    let static_files = warp::path("static").and(warp::fs::dir("./src/register/build/static/"));
    let react_app = warp::path::end()
        .and(warp::get())
        .and(warp::fs::file("./src/register/build/index.html"));

    let api = warp::path("get-ws-info").and(
        // 1. Get uqname (already on chain) and return networking information
        warp::get()
            .and(warp::any().map(move || ip.clone()))
            .and(warp::any().map(move || our_get.clone()))
            .and(warp::any().map(move || networking_keypair_post.clone()))
            .and_then(handle_get)
            // 2. trigger for finalizing registration once on-chain actions are done
            .or(warp::post()
                .and(warp::body::content_length_limit(1024 * 16))
                .and(warp::body::json())
                .and(warp::any().map(move || tx.clone()))
                .and(warp::any().map(move || our.lock().unwrap().take().unwrap()))
                .and(warp::any().map(move || networking_keypair.lock().unwrap().take().unwrap()))
                .and(warp::any().map(move || redir_port))
                .and_then(handle_post)),
    );

    let routes = static_files.or(react_app).or(api);

    let _ = open::that(format!("http://localhost:{}/", port));
    warp::serve(routes)
        .bind_with_graceful_shutdown(([0, 0, 0, 0], port), async {
            kill_rx.await.ok();
        })
        .1
        .await;
}

async fn handle_get(
    ip: String,
    our_get: Arc<Mutex<Option<Identity>>>,
    networking_keypair_post: Arc<Mutex<Option<Document>>>,
) -> Result<impl Reply, Rejection> {
    // 1. Generate networking keys
    let (public_key, serialized_networking_keypair) = keygen::generate_networking_key();
    *networking_keypair_post.lock().unwrap() = Some(serialized_networking_keypair);

    // 2. set our...
    // TODO: if IP is localhost, assign a router...
    let ws_port = http_server::find_open_port(9000).await.unwrap();

    let our = Identity {
        networking_key: public_key,
        name: String::new(),
        ws_routing: Some((ip.clone(), ws_port)),
        allowed_routers: vec![
            "uqbar-router-1.uq".into(), // "0x8d9e54427c50660c6d4802f63edca86a9ca5fd6a78070c4635950e9d149ed441".into(),
            "uqbar-router-2.uq".into(), // "0x06d331ed65843ecf0860c73292005d8103af20820546b2f8f9007d01f60595b1".into(),
            "uqbar-router-3.uq".into(), // "0xe6ab611eb62e8aee0460295667f8179cda4315982717db4b0b3da6022deecac1".into(),
        ],
    };

    *our_get.lock().unwrap() = Some(our.clone());

    // return response containing networking information
    Ok(warp::reply::json(&our))
}

async fn handle_post(
    info: Registration,
    sender: RegistrationSender,
    mut our: Identity,
    networking_keypair: Document,
    _redir_port: u16,
) -> Result<impl Reply, Rejection> {

    if info.direct {
        our.allowed_routers = vec![];
    } else {
        our.ws_routing = None;
    }

    our.name = info.username;

    let seed = SystemRandom::new();
    let mut jwt_secret = [0u8, 32];
    ring::rand::SecureRandom::fill(&seed, &mut jwt_secret).unwrap();

    let token = match generate_jwt(&jwt_secret, our.name.clone()) {
        Some(token) => token,
        None => return Err(warp::reject()),
    };
    let cookie_value = format!("uqbar-auth_{}={};", &our.name, &token);
    let ws_cookie_value = format!("uqbar-ws-auth_{}={};", &our.name, &token);

    sender
        .send((our, info.password, networking_keypair, jwt_secret.to_vec()))
        .await
        .unwrap();

    let mut response = warp::reply::html("Success".to_string()).into_response();

    let headers = response.headers_mut();
    headers.append(SET_COOKIE, HeaderValue::from_str(&cookie_value).unwrap());
    headers.append(SET_COOKIE, HeaderValue::from_str(&ws_cookie_value).unwrap());

    Ok(response)
}

/// Serve the login page, just get a password
pub async fn login(
    tx: mpsc::Sender<(
        String,
        Vec<String>,
        signature::Ed25519KeyPair,
        Vec<u8>,
        Vec<u8>,
    )>,
    kill_rx: oneshot::Receiver<bool>,
    keyfile: Vec<u8>,
    port: u16,
) {
    let login_page = include_str!("login.html");
    let redirect_to_login =
        warp::path::end().map(|| warp::redirect(warp::http::Uri::from_static("/login")));
    let routes = warp::path("login")
        .and(
            // 1. serve login.html right here
            warp::get()
                .map(move || warp::reply::html(login_page))
                // 2. await a single POST
                //    - password
                .or(warp::post()
                    .and(warp::body::content_length_limit(1024 * 16))
                    .and(warp::body::json())
                    .and(warp::any().map(move || keyfile.clone()))
                    .and(warp::any().map(move || tx.clone()))
                    .and_then(handle_password)),
        )
        .or(redirect_to_login);

    let _ = open::that(format!("http://localhost:{}/login", port));
    warp::serve(routes)
        .bind_with_graceful_shutdown(([0, 0, 0, 0], port), async {
            kill_rx.await.ok();
        })
        .1
        .await;
}

async fn handle_password(
    password: serde_json::Value,
    keyfile: Vec<u8>,
    tx: mpsc::Sender<(
        String,
        Vec<String>,
        signature::Ed25519KeyPair,
        Vec<u8>,
        Vec<u8>,
    )>,
) -> Result<impl Reply, Rejection> {
    let password = match password["password"].as_str() {
        Some(p) => p,
        None => return Err(warp::reject()),
    };
    // use password to decrypt networking keys
    let (username, routers, networking_keypair, jwt_secret_bytes, file_key) =
        keygen::decode_keyfile(keyfile, password);

    let token = match generate_jwt(&jwt_secret_bytes, username.clone()) {
        Some(token) => token,
        None => return Err(warp::reject()),
    };
    let cookie_value = format!("uqbar-auth_{}={};", &username, &token);
    let ws_cookie_value = format!("uqbar-ws-auth_{}={};", &username, &token);

    let mut response = warp::reply::html("Success".to_string()).into_response();

    let headers = response.headers_mut();
    headers.append(SET_COOKIE, HeaderValue::from_str(&cookie_value).unwrap());
    headers.append(SET_COOKIE, HeaderValue::from_str(&ws_cookie_value).unwrap());

    tx.send((
        username,
        routers,
        networking_keypair,
        jwt_secret_bytes.to_vec(),
        file_key.to_vec(),
    ))
    .await
    .unwrap();
    // TODO unhappy paths where key has changed / can't be decrypted
    Ok(response)
}
