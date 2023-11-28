use aes_gcm::aead::KeyInit;

use ethers::prelude::{abigen, namehash, Address as EthAddress, Provider, U256};
use ethers_providers::Ws;
use hmac::Hmac;
use jwt::SignWithKey;
use ring::pkcs8::Document;
use ring::rand::SystemRandom;
use ring::signature;
use ring::signature::KeyPair;
use sha2::Sha256;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};
use warp::{
    http::{
        header::{HeaderMap, HeaderValue, SET_COOKIE},
        StatusCode,
    },
    Filter, Rejection, Reply,
};

use crate::keygen;
use crate::types::*;

type RegistrationSender = mpsc::Sender<(Identity, Keyfile, Vec<u8>)>;

pub const QNS_SEPOLIA_ADDRESS: &str = "0x9e5ed0e7873E0d7f10eEb6dE72E87fE087A12776";

pub fn generate_jwt(jwt_secret_bytes: &[u8], username: String) -> Option<String> {
    let jwt_secret: Hmac<Sha256> = match Hmac::new_from_slice(jwt_secret_bytes) {
        Ok(secret) => secret,
        Err(_) => return None,
    };

    let claims = crate::http::types::JwtClaims {
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
    rpc_url: String,
    keyfile: Vec<u8>,
) {
    let our_temp_arc = Arc::new(Mutex::new(None)); // Networking info is generated and passed to the UI, but not used until confirmed
    let our_ws_info = our_temp_arc.clone();

    let net_keypair_arc = Arc::new(Mutex::new(None));
    let net_keypair_ws_info = net_keypair_arc.clone();

    let keyfile_arc = Arc::new(Mutex::new(Some(keyfile)));
    let keyfile_vet = keyfile_arc.clone();

    let static_files = warp::path("static").and(warp::fs::dir("./src/register-ui/build/static/"));

    let react_app = warp::path::end()
        .and(warp::get())
        .and(warp::fs::file("./src/register-ui/build/index.html"));

    let keyfile_info_copy = keyfile_arc.clone();
    let boot_tx = tx.clone();
    let boot_our_arc = our_temp_arc.clone();
    let boot_net_keypair_arc = net_keypair_arc.clone();
    let import_tx = tx.clone();
    let import_our_arc = our_temp_arc.clone();
    let login_tx = tx.clone();
    let login_keyfile_arc = keyfile_arc.clone();
    let generate_keys_ip = ip.clone();

    let api = warp::path("info")
        .and(
            warp::get()
                .and(warp::any().map(move || keyfile_info_copy.clone()))
                .and_then(get_unencrypted_info),
        )
        .or(warp::path("generate-networking-info").and(
            warp::post()
                .and(warp::any().map(move || generate_keys_ip.clone()))
                .and(warp::any().map(move || our_ws_info.clone()))
                .and(warp::any().map(move || net_keypair_ws_info.clone()))
                .and_then(generate_networking_info),
        ))
        .or(warp::path("vet-keyfile").and(
            warp::post()
                .and(warp::body::content_length_limit(1024 * 16))
                .and(warp::body::json())
                .and(warp::any().map(move || keyfile_vet.clone()))
                .and_then(handle_keyfile_vet),
        ))
        .or(warp::path("boot").and(
            warp::post()
                .and(warp::body::content_length_limit(1024 * 16))
                .and(warp::body::json())
                .and(warp::any().map(move || boot_tx.clone()))
                .and(warp::any().map(move || boot_our_arc.lock().unwrap().take().unwrap()))
                .and(warp::any().map(move || boot_net_keypair_arc.lock().unwrap().take().unwrap()))
                .and_then(handle_boot),
        ))
        .or(warp::path("import-keyfile").and(
            warp::post()
                .and(warp::body::content_length_limit(1024 * 16))
                .and(warp::body::json())
                .and(warp::any().map(move || import_tx.clone()))
                .and(warp::any().map(move || import_our_arc.lock().unwrap().take().unwrap()))
                .and_then(handle_import_keyfile),
        ))
        .or(warp::path("login").and(
            warp::post()
                .and(warp::body::content_length_limit(1024 * 16))
                .and(warp::body::json())
                .and(warp::any().map(move || ip.clone()))
                .and(warp::any().map(move || rpc_url.clone()))
                .and(warp::any().map(move || login_tx.clone()))
                .and(warp::any().map(move || login_keyfile_arc.lock().unwrap().take().unwrap()))
                .and_then(handle_login),
        ))
        .or(warp::path("confirm-change-network-keys").and(
            warp::post()
                .and(warp::body::content_length_limit(1024 * 16))
                .and(warp::body::json())
                .and(warp::any().map(move || tx.clone()))
                .and(warp::any().map(move || our_temp_arc.lock().unwrap().take().unwrap()))
                .and(warp::any().map(move || net_keypair_arc.lock().unwrap().take().unwrap()))
                .and(warp::any().map(move || keyfile_arc.lock().unwrap().take().unwrap()))
                .and_then(confirm_change_network_keys),
        ));

    let mut headers = HeaderMap::new();
    headers.insert(
        "Cache-Control",
        HeaderValue::from_static("no-store, no-cache, must-revalidate, proxy-revalidate"),
    );

    let routes = static_files
        .or(react_app)
        .or(api)
        .with(warp::reply::with::headers(headers));

    let _ = open::that(format!("http://localhost:{}/", port));
    warp::serve(routes)
        .bind_with_graceful_shutdown(([0, 0, 0, 0], port), async {
            kill_rx.await.ok();
        })
        .1
        .await;
}

async fn get_unencrypted_info(
    keyfile_arc: Arc<Mutex<Option<Vec<u8>>>>,
) -> Result<impl Reply, Rejection> {
    let (name, allowed_routers) = {
        match keyfile_arc.lock().unwrap().clone() {
            Some(encoded_keyfile) => match keygen::get_username_and_routers(encoded_keyfile) {
                Ok(k) => k,
                Err(_) => {
                    return Ok(warp::reply::with_status(
                        warp::reply::json(&"Failed to decode keyfile".to_string()),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    )
                    .into_response())
                }
            },
            None => {
                return Ok(warp::reply::with_status(
                    warp::reply::json(&"Keyfile not present".to_string()),
                    StatusCode::NOT_FOUND,
                )
                .into_response())
            }
        }
    };

    let our = UnencryptedIdentity {
        name,
        allowed_routers,
    };

    Ok(warp::reply::with_status(Ok(warp::reply::json(&our)), StatusCode::OK).into_response())
}

async fn generate_networking_info(
    ip: String,
    our_temp_arc: Arc<Mutex<Option<Identity>>>,
    networking_keypair_arc: Arc<Mutex<Option<Document>>>,
) -> Result<impl Reply, Rejection> {
    let (public_key, serialized_networking_keypair) = keygen::generate_networking_key();
    *networking_keypair_arc.lock().unwrap() = Some(serialized_networking_keypair);

    // TODO: if IP is localhost, don't allow registration as direct
    let ws_port = crate::http::utils::find_open_port(9000).await.unwrap();

    // This is a temporary identity, passed to the UI. If it is confirmed through a /boot or /confirm-change-network-keys, then it will be used to replace the current identity
    let our_temp = Identity {
        networking_key: format!("0x{}", public_key),
        name: "".to_string(),
        ws_routing: Some((ip, ws_port)),
        allowed_routers: vec![
            "uqbar-router-1.uq".into(), // "0x8d9e54427c50660c6d4802f63edca86a9ca5fd6a78070c4635950e9d149ed441".into(),
            "uqbar-router-2.uq".into(), // "0x06d331ed65843ecf0860c73292005d8103af20820546b2f8f9007d01f60595b1".into(),
            "uqbar-router-3.uq".into(), // "0xe6ab611eb62e8aee0460295667f8179cda4315982717db4b0b3da6022deecac1".into(),
        ],
    };

    *our_temp_arc.lock().unwrap() = Some(our_temp.clone());

    Ok(warp::reply::json(&our_temp))
}

async fn handle_keyfile_vet(
    payload: KeyfileVet,
    keyfile_arc: Arc<Mutex<Option<Vec<u8>>>>,
) -> Result<impl Reply, Rejection> {
    let encoded_keyfile = match payload.keyfile.is_empty() {
        true => keyfile_arc.lock().unwrap().clone().unwrap(),
        false => base64::decode(payload.keyfile).unwrap(),
    };

    let decoded_keyfile = match keygen::decode_keyfile(encoded_keyfile, &payload.password) {
        Ok(k) => k,
        Err(_) => return Err(warp::reject()),
    };

    let keyfile_vetted = KeyfileVetted {
        username: decoded_keyfile.username,
        networking_key: format!(
            "0x{}",
            hex::encode(decoded_keyfile.networking_keypair.public_key().as_ref())
        ),
        routers: decoded_keyfile.routers,
    };

    Ok(warp::reply::json(&keyfile_vetted))
}

async fn handle_boot(
    info: BootInfo,
    sender: RegistrationSender,
    mut our: Identity,
    networking_keypair: Document,
) -> Result<impl Reply, Rejection> {
    our.name = info.username;

    if info.direct {
        our.allowed_routers = vec![];
    } else {
        our.ws_routing = None;
    }

    let seed = SystemRandom::new();
    let mut jwt_secret = [0u8, 32];
    ring::rand::SecureRandom::fill(&seed, &mut jwt_secret).unwrap();

    let decoded_keyfile = Keyfile {
        username: our.name.clone(),
        routers: our.allowed_routers.clone(),
        networking_keypair: signature::Ed25519KeyPair::from_pkcs8(networking_keypair.as_ref())
            .unwrap(),
        jwt_secret_bytes: jwt_secret.to_vec(),
        file_key: keygen::generate_file_key(),
    };

    let encoded_keyfile = keygen::encode_keyfile(
        info.password,
        decoded_keyfile.username.clone(),
        decoded_keyfile.routers.clone(),
        networking_keypair,
        decoded_keyfile.jwt_secret_bytes.clone(),
        decoded_keyfile.file_key.clone(),
    );

    let encoded_keyfile_str = base64::encode(encoded_keyfile.clone());

    success_response(
        sender,
        our,
        decoded_keyfile,
        encoded_keyfile,
        encoded_keyfile_str,
    )
    .await
}

async fn handle_import_keyfile(
    info: ImportKeyfileInfo,
    sender: RegistrationSender,
    mut our: Identity,
) -> Result<impl Reply, Rejection> {
    // if keyfile was not present in node and is present from user upload
    let encoded_keyfile = match base64::decode(info.keyfile.clone()) {
        Ok(k) => k,
        Err(_) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&"Keyfile not valid base64".to_string()),
                StatusCode::BAD_REQUEST,
            )
            .into_response())
        }
    };

    let decoded_keyfile = match keygen::decode_keyfile(encoded_keyfile.clone(), &info.password) {
        Ok(k) => {
            our.name = k.username.clone();
            our.allowed_routers = k.routers.clone();
            if !our.allowed_routers.is_empty() {
                our.ws_routing = None;
            }
            our.networking_key = format!(
                "0x{}",
                hex::encode(k.networking_keypair.public_key().as_ref())
            );
            k
        }
        Err(_) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&"Failed to decode keyfile".to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
    };

    let encoded_keyfile_str = info.keyfile.clone();

    success_response(
        sender,
        our,
        decoded_keyfile,
        encoded_keyfile,
        encoded_keyfile_str,
    )
    .await
}

async fn handle_login(
    info: LoginInfo,
    ip: String,
    rpc_url: String,
    sender: RegistrationSender,
    encoded_keyfile: Vec<u8>,
) -> Result<impl Reply, Rejection> {
    if encoded_keyfile.is_empty() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&"Keyfile not present".to_string()),
            StatusCode::NOT_FOUND,
        )
        .into_response());
    }

    let (decoded_keyfile, our) =
        match keygen::decode_keyfile(encoded_keyfile.clone(), &info.password) {
            Ok(k) => {
                let mut our = Identity {
                    name: k.username.clone(),
                    networking_key: format!(
                        "0x{}",
                        hex::encode(k.networking_keypair.public_key().as_ref())
                    ),
                    ws_routing: None, // TODO: look on line 362 for this
                    allowed_routers: k.routers.clone(),
                };

                if !our.allowed_routers.is_empty() {
                    our.ws_routing = None;
                }

                (k, our)
            }
            Err(_) => {
                return Ok(warp::reply::with_status(
                    warp::reply::json(&"Failed to decode keyfile".to_string()),
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
                .into_response())
            }
        };

    // check if Identity for this username has correct networking keys,
    // if not, prompt user to reset them.
    let Ok(ws_rpc) = Provider::<Ws>::connect(rpc_url.clone()).await else {
        return Ok(warp::reply::with_status(
            warp::reply::json(&"Could not connect to RPC endpoint".to_string()),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response());
    };
    let qns_address: EthAddress = QNS_SEPOLIA_ADDRESS.parse().unwrap();
    let contract = QNSRegistry::new(qns_address, ws_rpc.into());
    let node_id: U256 = namehash(&our.name).as_bytes().into();
    let onchain_id = contract.ws(node_id).call().await.unwrap(); // TODO unwrap

    // The open port should match the registry
    let ws_port = crate::http::utils::find_open_port(9000).await.unwrap();

    // double check that routers match on-chain information
    let namehashed_routers: Vec<[u8; 32]> = our
        .allowed_routers
        .clone()
        .into_iter()
        .map(|name| {
            let hash = namehash(&name);
            let mut result = [0u8; 32];
            result.copy_from_slice(hash.as_bytes());
            result
        })
        .collect();

    let current_ip_and_port = combineIpAndPort(ip, ws_port);

    // double check that keys match on-chain information
    if onchain_id.routers != namehashed_routers
        || onchain_id.public_key != our.networking_key
        || (onchain_id.ip_and_port > 0 && onchain_id.ip_and_port != current_ip_and_port)
    {
        return Ok(warp::reply::with_status(
            warp::reply::json(&format!("Your onchain routing info of {}, does not match your current IP and port of {}", onchain_id.ip_and_port, current_ip_and_port).to_string()),
            StatusCode::CONFLICT,
        )
        .into_response());
    }

    let encoded_keyfile_str = base64::encode(encoded_keyfile.clone());

    success_response(
        sender,
        our,
        decoded_keyfile,
        encoded_keyfile,
        encoded_keyfile_str,
    )
    .await
}

async fn confirm_change_network_keys(
    info: LoginAndResetInfo,
    sender: RegistrationSender,
    mut our: Identity, // the arc of our temporary identity
    networking_keypair: Document,
    encoded_keyfile: Vec<u8>,
) -> Result<impl Reply, Rejection> {
    if encoded_keyfile.is_empty() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&"Keyfile not present".to_string()),
            StatusCode::BAD_REQUEST,
        )
        .into_response());
    }

    // Get our name from our current keyfile
    match keygen::decode_keyfile(encoded_keyfile.clone(), &info.password) {
        Ok(k) => {
            our.name = k.username.clone();
        }
        Err(_) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&"Failed to decode keyfile".to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response());
        }
    };

    // Determine if direct node or not
    if info.direct {
        our.allowed_routers = vec![];
    } else {
        our.ws_routing = None;
    }

    let seed = SystemRandom::new();
    let mut jwt_secret = [0u8, 32];
    ring::rand::SecureRandom::fill(&seed, &mut jwt_secret).unwrap();

    let decoded_keyfile = Keyfile {
        username: our.name.clone(),
        routers: our.allowed_routers.clone(),
        networking_keypair: signature::Ed25519KeyPair::from_pkcs8(networking_keypair.as_ref())
            .unwrap(),
        jwt_secret_bytes: jwt_secret.to_vec(),
        file_key: keygen::generate_file_key(),
    };

    let encoded_keyfile = keygen::encode_keyfile(
        info.password,
        decoded_keyfile.username.clone(),
        decoded_keyfile.routers.clone(),
        networking_keypair,
        decoded_keyfile.jwt_secret_bytes.clone(),
        decoded_keyfile.file_key.clone(),
    );

    let encoded_keyfile_str = base64::encode(encoded_keyfile.clone());

    success_response(
        sender,
        our,
        decoded_keyfile,
        encoded_keyfile,
        encoded_keyfile_str,
    )
    .await
}

async fn success_response(
    sender: RegistrationSender,
    our: Identity,
    decoded_keyfile: Keyfile,
    encoded_keyfile: Vec<u8>,
    encoded_keyfile_str: String,
) -> Result<warp::reply::Response, Rejection> {
    let token = match generate_jwt(&decoded_keyfile.jwt_secret_bytes, our.name.clone()) {
        Some(token) => token,
        None => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&"Failed to generate JWT".to_string()),
                StatusCode::SERVICE_UNAVAILABLE,
            )
            .into_response())
        }
    };

    sender
        .send((our.clone(), decoded_keyfile, encoded_keyfile))
        .await
        .unwrap();

    let mut response =
        warp::reply::with_status(warp::reply::json(&encoded_keyfile_str), StatusCode::FOUND)
            .into_response();

    let headers = response.headers_mut();

    match HeaderValue::from_str(&format!("uqbar-auth_{}={};", &our.name, &token)) {
        Ok(v) => {
            headers.append(SET_COOKIE, v);
        }
        Err(_) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&"Failed to generate Auth JWT".to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
    }

    // match HeaderValue::from_str(&format!("uqbar-ws-auth_{}={};", &our.name, &token)) {
    //     Ok(v) => {
    //         headers.append(SET_COOKIE, v);
    //     },
    //     Err(_) => {
    //         return Ok(warp::reply::with_status(
    //             warp::reply::json(&"Failed to generate WS JWT".to_string()),
    //             StatusCode::INTERNAL_SERVER_ERROR,
    //         )
    //         .into_response())
    //     }
    // }

    Ok(response)
}
