use aes_gcm::aead::KeyInit;
use base64;
use hmac::Hmac;
use jwt::SignWithKey;
use ring::pkcs8::Document;
use ring::rand::SystemRandom;
use ring::signature;
use sha2::Sha256;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};
use warp::{
    http::{ StatusCode, header::{HeaderValue, SET_COOKIE}, },
    Filter, Rejection, Reply, 
};

use crate::http_server;
use crate::keygen;
use crate::types::*;

type RegistrationSender = mpsc::Sender<(Identity, Keyfile, Vec<u8>)>;

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
    keyfile: Vec<u8>
) {

    let our_arc = Arc::new(Mutex::new(None));
    let our_ws_info = our_arc.clone();

    let net_keypair_arc = Arc::new(Mutex::new(None));
    let net_keypair_ws_info = net_keypair_arc.clone();

    let keyfile_arc = Arc::new(Mutex::new(Some(keyfile)));
    let keyfile_has = keyfile_arc.clone();

    let static_files = warp::path("static").and(warp::fs::dir("./src/register/build/static/"));
    let react_app = warp::path::end()
        .and(warp::get())
        .and(warp::fs::file("./src/register/build/index.html"));

    let api = warp::path("has-keyfile")
            .and(warp::get()
                .and(warp::any().map(move || keyfile_has.clone()))
                .and_then(handle_has_keyfile))
        .or(warp::path("info")
            .and(warp::get()
                .and(warp::any().map(move || ip.clone()))
                .and(warp::any().map(move || our_ws_info.clone()))
                .and(warp::any().map(move || net_keypair_ws_info.clone()))
                .and_then(handle_info)))
        .or(warp::path("vet-keyfile")
            .and(warp::post()
                .and(warp::body::content_length_limit(1024 * 16)) 
                .and(warp::body::json())
                .and_then(handle_keyfile_check)))
        .or(warp::path("boot")
            .and(warp::put()
                .and(warp::body::content_length_limit(1024 * 16)) 
                .and(warp::body::json())
                .and(warp::any().map(move || tx.clone()))
                .and(warp::any().map(move || our_arc.lock().unwrap().take().unwrap()))
                .and(warp::any().map(move || net_keypair_arc.lock().unwrap().take().unwrap()))
                .and(warp::any().map(move || keyfile_arc.lock().unwrap().take().unwrap()))
                .and_then(handle_boot)));

    let routes = static_files.or(react_app).or(api);

    let _ = open::that(format!("http://localhost:{}/", port));
    warp::serve(routes)
        .bind_with_graceful_shutdown(([0, 0, 0, 0], port), async {
            kill_rx.await.ok();
        })
        .1
        .await;
}

async fn handle_has_keyfile(
    keyfile: Arc<Mutex<Option<Vec<u8>>>>,
) -> Result<impl Reply, Rejection> {

    Ok(warp::reply::json(&keyfile.lock().unwrap().is_some()))

}

async fn handle_keyfile_check(
    payload: KeyfileCheck
) -> Result<impl Reply, Rejection> {

    let keyfile = base64::decode(payload.keyfile).unwrap();

    match keygen::decode_keyfile(keyfile, &payload.password) {
        Ok(_) => Ok(warp::reply::with_status(warp::reply(), StatusCode::OK)),
        Err(_) => Err(warp::reject()),
    }

}

async fn handle_keyfile_gen(
    payload: Registration,
    our: Arc<Mutex<Option<Identity>>>,
    networking_keypair: Arc<Mutex<Option<Document>>>,
    jwt_secret: Arc<Mutex<Option<Vec<u8>>>>,
) -> Result<impl Reply, Rejection> {

    Ok(warp::reply::with_status(warp::reply(), StatusCode::OK))

}

async fn handle_boot(
    info: BootInfo,
    sender: RegistrationSender,
    mut our: Identity,
    networking_keypair: Document,
    mut encoded_keyfile: Vec<u8>,
) -> Result<impl Reply, Rejection> {

    println!("hello");

    if info.direct {
        our.allowed_routers = vec![];
    } else {
        our.ws_routing = None;
    }

    println!("~~~~~");

    if encoded_keyfile.is_empty() && !info.keyfile.is_empty() {
        match base64::decode(info.keyfile) {
            Ok(k) => encoded_keyfile = k,
            Err(_) => return Err(warp::reject()),
        }
    }

    println!("_____");

    let decoded_keyfile = if !encoded_keyfile.is_empty() {
        match keygen::decode_keyfile(encoded_keyfile.clone(), &info.password) {
            Ok(k) => k,
            Err(_) => return Err(warp::reject()),
        }
    } else {
        let seed = SystemRandom::new();
        let mut jwt_secret = [0u8, 32];
        ring::rand::SecureRandom::fill(&seed, &mut jwt_secret).unwrap();

        let networking_pair = signature::Ed25519KeyPair::from_pkcs8(networking_keypair.as_ref()).unwrap();

        Keyfile {
            username: our.name.clone(),
            routers: our.allowed_routers.clone(),
            networking_keypair: signature::Ed25519KeyPair
                ::from_pkcs8(networking_keypair.as_ref()).unwrap(),
            jwt_secret_bytes: jwt_secret.to_vec(),
            file_key: keygen::generate_file_key(),
        }
    };

    println!(">>>>>");

    if encoded_keyfile.is_empty() {
        encoded_keyfile = keygen::encode_keyfile(
            info.password,
            decoded_keyfile.username.clone(),
            decoded_keyfile.routers.clone(),
            networking_keypair,
            decoded_keyfile.jwt_secret_bytes.clone(),
            decoded_keyfile.file_key.clone(),
        );
    }

    println!("<<<<<");

    let token = match generate_jwt(&decoded_keyfile.jwt_secret_bytes, our.name.clone()) {
        Some(token) => token,
        None => return Err(warp::reject()),
    };

    sender.send((our.clone(), decoded_keyfile, encoded_keyfile.clone())).await.unwrap();

    let mut response = warp::reply::html("Success".to_string()).into_response();

    println!("ioioioio");

    let headers = response.headers_mut();
    headers.append(SET_COOKIE, HeaderValue::from_str(
        &format!("uqbar-auth_{}={};", &our.name, &token)).unwrap());
    headers.append(SET_COOKIE, HeaderValue::from_str(
        &format!("uqbar-ws-auth_{}={};", &our.name, &token)).unwrap());

    Ok(warp::reply::with_status(warp::reply(), StatusCode::OK))
}

async fn handle_info(
    ip: String,
    our_arc: Arc<Mutex<Option<Identity>>>,
    networking_keypair_arc: Arc<Mutex<Option<Document>>>,
) -> Result<impl Reply, Rejection> {
    // 1. Generate networking keys
    let (public_key, serialized_networking_keypair) = keygen::generate_networking_key();
    *networking_keypair_arc.lock().unwrap() = Some(serialized_networking_keypair);

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

    *our_arc.lock().unwrap() = Some(our.clone());

    Ok(warp::reply::json(&our))

}

async fn handle_post(
    info: Registration,
    sender: RegistrationSender,
    mut our: Identity,
    networking_keypair: Document,
    keyfile: Vec<u8>,
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

    // sender
    //     .send((our, info.password, networking_keypair, jwt_secret.to_vec(), keyfile))
    //     .await
    //     .unwrap();

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
    let routes = warp::path("login").and(
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
    );

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
    let decoded = match keygen::decode_keyfile(keyfile, password) {
        Ok(decoded) => decoded,
        Err(_) => return Err(warp::reject()),
    };

    let token = match generate_jwt(&decoded.jwt_secret_bytes, decoded.username.clone()) {
        Some(token) => token,
        None => return Err(warp::reject()),
    };
    let cookie_value = format!("uqbar-auth_{}={};", &decoded.username, &token);
    let ws_cookie_value = format!("uqbar-ws-auth_{}={};", &decoded.username, &token);

    let mut response = warp::reply::html("Success".to_string()).into_response();

    let headers = response.headers_mut();
    headers.append(SET_COOKIE, HeaderValue::from_str(&cookie_value).unwrap());
    headers.append(SET_COOKIE, HeaderValue::from_str(&ws_cookie_value).unwrap());

    tx.send((
        decoded.username,
        decoded.routers,
        decoded.networking_keypair,
        decoded.jwt_secret_bytes.to_vec(),
        decoded.file_key.to_vec(),
    ))
    .await
    .unwrap();
    // TODO unhappy paths where key has changed / can't be decrypted
    Ok(response)
}
