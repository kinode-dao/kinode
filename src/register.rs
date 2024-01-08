use aes_gcm::aead::KeyInit;
use ethers::prelude::{abigen, namehash, Address as EthAddress, Provider, U256};
use ethers_providers::Ws;
use hmac::Hmac;
use jwt::SignWithKey;
use ring::rand::SystemRandom;
use ring::signature;
use ring::signature::KeyPair;
use sha2::Sha256;
use static_dir::static_dir;
use std::sync::Arc;
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

// Human readable ABI
abigen!(
    QNSRegistry,
    r"[
    function ws(uint256 node) external view returns (bytes32,uint32,uint16,bytes32[])
]"
);

type RegistrationSender = mpsc::Sender<(Identity, Keyfile, Vec<u8>)>;

pub const _QNS_SEPOLIA_ADDRESS: &str = "0x4C8D8d4A71cE21B4A16dAbf4593cDF30d79728F1";

pub fn _ip_to_number(ip: &str) -> Result<u32, &'static str> {
    let octets: Vec<&str> = ip.split('.').collect();

    if octets.len() != 4 {
        return Err("Invalid IP address");
    }

    let mut ip_num: u32 = 0;
    for &octet in octets.iter() {
        ip_num <<= 8;
        match octet.parse::<u32>() {
            Ok(num) => {
                if num > 255 {
                    return Err("Invalid octet in IP address");
                }
                ip_num += num;
            }
            Err(_) => return Err("Invalid number in IP address"),
        }
    }

    Ok(ip_num)
}

fn _hex_string_to_u8_array(hex_str: &str) -> Result<[u8; 32], &'static str> {
    if !hex_str.starts_with("0x") || hex_str.len() != 66 {
        // "0x" + 64 hex chars
        return Err("Invalid hex format or length");
    }

    let no_prefix = &hex_str[2..];
    let mut bytes = [0_u8; 32];
    for (i, byte) in no_prefix.as_bytes().chunks(2).enumerate() {
        let hex_byte = std::str::from_utf8(byte)
            .map_err(|_| "Invalid UTF-8 sequence")?
            .parse::<u8>()
            .map_err(|_| "Failed to parse hex byte")?;
        bytes[i] = hex_byte;
    }

    Ok(bytes)
}

pub fn generate_jwt(jwt_secret_bytes: &[u8], username: &str) -> Option<String> {
    let jwt_secret: Hmac<Sha256> = match Hmac::new_from_slice(jwt_secret_bytes) {
        Ok(secret) => secret,
        Err(_) => return None,
    };

    let claims = crate::http::types::JwtClaims {
        username: username.to_string(),
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
    keyfile: Option<Vec<u8>>,
) {
    // Networking info is generated and passed to the UI, but not used until confirmed
    let (public_key, serialized_networking_keypair) = keygen::generate_networking_key();
    let net_keypair = Arc::new(serialized_networking_keypair.as_ref().to_vec());
    let tx = Arc::new(tx);

    // TODO: if IP is localhost, don't allow registration as direct
    let ws_port = crate::http::utils::find_open_port(9000).await.unwrap();

    // This is a temporary identity, passed to the UI. If it is confirmed through a /boot or /confirm-change-network-keys, then it will be used to replace the current identity
    let our_temp_id = Arc::new(Identity {
        networking_key: format!("0x{}", public_key),
        name: "".to_string(),
        ws_routing: Some((ip.clone(), ws_port)),
        allowed_routers: vec![
            "nectar-next-router.uq".into(),
            // "nectar-router-1.uq".into(),
            // "nectar-router-2.uq".into(),
            // "nectar-router-3.uq".into(),
        ],
    });

    let keyfile = warp::any().map(move || keyfile.clone());
    let our_temp_id = warp::any().map(move || our_temp_id.clone());
    let net_keypair = warp::any().map(move || net_keypair.clone());
    let tx = warp::any().map(move || tx.clone());
    let ip = warp::any().map(move || ip.clone());
    let rpc_url = warp::any().map(move || rpc_url.clone());

    let static_files = warp::path("static").and(static_dir!("src/register-ui/build/static/"));

    let react_app = warp::path::end()
        .and(warp::get())
        .map(move || warp::reply::html(include_str!("register-ui/build/index.html")))
        .or(warp::path("login")
            .and(warp::get())
            .map(move || warp::reply::html(include_str!("register-ui/build/index.html"))))
        .or(warp::path("register-name")
            .and(warp::get())
            .map(move || warp::reply::html(include_str!("register-ui/build/index.html"))))
        .or(warp::path("claim-invite")
            .and(warp::get())
            .map(move || warp::reply::html(include_str!("register-ui/build/index.html"))))
        .or(warp::path("reset")
            .and(warp::get())
            .map(move || warp::reply::html(include_str!("register-ui/build/index.html"))))
        .or(warp::path("import-keyfile")
            .and(warp::get())
            .map(move || warp::reply::html(include_str!("register-ui/build/index.html"))))
        .or(warp::path("set-password")
            .and(warp::get())
            .map(move || warp::reply::html(include_str!("register-ui/build/index.html"))));

    let api = warp::path("info")
        .and(
            warp::get()
                .and(keyfile.clone())
                .and_then(get_unencrypted_info),
        )
        .or(warp::path("generate-networking-info").and(
            warp::post()
                .and(our_temp_id.clone())
                .and_then(generate_networking_info),
        ))
        .or(warp::path("vet-keyfile").and(
            warp::post()
                .and(warp::body::content_length_limit(1024 * 16))
                .and(warp::body::json())
                .and(keyfile.clone())
                .and_then(handle_keyfile_vet),
        ))
        .or(warp::path("boot").and(
            warp::post()
                .and(warp::body::content_length_limit(1024 * 16))
                .and(warp::body::json())
                .and(tx.clone())
                .and(our_temp_id.clone())
                .and(net_keypair.clone())
                .and_then(handle_boot),
        ))
        .or(warp::path("import-keyfile").and(
            warp::post()
                .and(warp::body::content_length_limit(1024 * 16))
                .and(warp::body::json())
                .and(ip.clone())
                .and(rpc_url.clone())
                .and(tx.clone())
                .and_then(handle_import_keyfile),
        ))
        .or(warp::path("login").and(
            warp::post()
                .and(warp::body::content_length_limit(1024 * 16))
                .and(warp::body::json())
                .and(ip)
                .and(rpc_url)
                .and(tx.clone())
                .and(keyfile.clone())
                .and_then(handle_login),
        ))
        .or(warp::path("confirm-change-network-keys").and(
            warp::post()
                .and(warp::body::content_length_limit(1024 * 16))
                .and(warp::body::json())
                .and(tx)
                .and(our_temp_id)
                .and(net_keypair)
                .and(keyfile)
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

async fn get_unencrypted_info(keyfile: Option<Vec<u8>>) -> Result<impl Reply, Rejection> {
    let (name, allowed_routers) = {
        match keyfile {
            Some(encoded_keyfile) => match keygen::get_username_and_routers(&encoded_keyfile) {
                Ok(k) => k,
                Err(_) => {
                    return Ok(warp::reply::with_status(
                        warp::reply::json(&"Incorrect password"),
                        StatusCode::UNAUTHORIZED,
                    )
                    .into_response())
                }
            },
            None => {
                return Ok(warp::reply::with_status(
                    warp::reply::json(&"Keyfile not present"),
                    StatusCode::NOT_FOUND,
                )
                .into_response())
            }
        }
    };
    Ok(warp::reply::with_status(
        Ok(warp::reply::json(&UnencryptedIdentity {
            name,
            allowed_routers,
        })),
        StatusCode::OK,
    )
    .into_response())
}

async fn generate_networking_info(our_temp_id: Arc<Identity>) -> Result<impl Reply, Rejection> {
    Ok(warp::reply::json(our_temp_id.as_ref()))
}

async fn handle_keyfile_vet(
    payload: KeyfileVet,
    keyfile: Option<Vec<u8>>,
) -> Result<impl Reply, Rejection> {
    let encoded_keyfile = match payload.keyfile.is_empty() {
        true => keyfile.ok_or(warp::reject())?,
        false => base64::decode(payload.keyfile).map_err(|_| warp::reject())?,
    };

    let decoded_keyfile =
        keygen::decode_keyfile(&encoded_keyfile, &payload.password).map_err(|_| warp::reject())?;

    Ok(warp::reply::json(&KeyfileVetted {
        username: decoded_keyfile.username,
        networking_key: format!(
            "0x{}",
            hex::encode(decoded_keyfile.networking_keypair.public_key().as_ref())
        ),
        routers: decoded_keyfile.routers,
    }))
}

async fn handle_boot(
    info: BootInfo,
    sender: Arc<RegistrationSender>,
    our: Arc<Identity>,
    networking_keypair: Arc<Vec<u8>>,
) -> Result<impl Reply, Rejection> {
    let mut our = our.as_ref().clone();
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
        networking_keypair.as_ref(),
        decoded_keyfile.jwt_secret_bytes.clone(),
        decoded_keyfile.file_key.clone(),
    );

    success_response(sender, our, decoded_keyfile, encoded_keyfile).await
}

async fn handle_import_keyfile(
    info: ImportKeyfileInfo,
    ip: String,
    _rpc_url: String,
    sender: Arc<RegistrationSender>,
) -> Result<impl Reply, Rejection> {
    // if keyfile was not present in node and is present from user upload
    let encoded_keyfile = match base64::decode(info.keyfile.clone()) {
        Ok(k) => k,
        Err(_) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&"Keyfile not valid base64"),
                StatusCode::BAD_REQUEST,
            )
            .into_response())
        }
    };

    let Some(ws_port) = crate::http::utils::find_open_port(9000).await else {
        return Ok(warp::reply::with_status(
            warp::reply::json(&"Unable to find free port"),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response());
    };

    let (decoded_keyfile, our) = match keygen::decode_keyfile(&encoded_keyfile, &info.password) {
        Ok(k) => {
            let our = Identity {
                name: k.username.clone(),
                networking_key: format!(
                    "0x{}",
                    hex::encode(k.networking_keypair.public_key().as_ref())
                ),
                ws_routing: if k.routers.is_empty() {
                    Some((ip, ws_port))
                } else {
                    None
                },
                allowed_routers: k.routers.clone(),
            };

            (k, our)
        }
        Err(_) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&"Incorrect Password".to_string()),
                StatusCode::UNAUTHORIZED,
            )
            .into_response())
        }
    };

    // if !networking_info_valid(rpc_url, ip, ws_port, &our).await {
    //     return Ok(warp::reply::with_status(
    //         warp::reply::json(&"Networking info invalid".to_string()),
    //         StatusCode::UNAUTHORIZED,
    //     )
    //     .into_response());
    // }

    success_response(sender, our, decoded_keyfile, encoded_keyfile).await
}

async fn handle_login(
    info: LoginInfo,
    ip: String,
    _rpc_url: String,
    sender: Arc<RegistrationSender>,
    encoded_keyfile: Option<Vec<u8>>,
) -> Result<impl Reply, Rejection> {
    if encoded_keyfile.is_none() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&"Keyfile not present"),
            StatusCode::NOT_FOUND,
        )
        .into_response());
    }
    let encoded_keyfile = encoded_keyfile.unwrap();

    let Some(ws_port) = crate::http::utils::find_open_port(9000).await else {
        return Ok(warp::reply::with_status(
            warp::reply::json(&"Unable to find free port"),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response());
    };

    let (decoded_keyfile, our) = match keygen::decode_keyfile(&encoded_keyfile, &info.password) {
        Ok(k) => {
            let our = Identity {
                name: k.username.clone(),
                networking_key: format!(
                    "0x{}",
                    hex::encode(k.networking_keypair.public_key().as_ref())
                ),
                ws_routing: if k.routers.is_empty() {
                    Some((ip, ws_port))
                } else {
                    None
                },
                allowed_routers: k.routers.clone(),
            };

            (k, our)
        }
        Err(_) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&"Incorrect Password"),
                StatusCode::UNAUTHORIZED,
            )
            .into_response())
        }
    };

    success_response(sender, our, decoded_keyfile, encoded_keyfile).await
}

async fn confirm_change_network_keys(
    info: LoginAndResetInfo,
    sender: Arc<RegistrationSender>,
    our: Arc<Identity>,
    networking_keypair: Arc<Vec<u8>>,
    encoded_keyfile: Option<Vec<u8>>,
) -> Result<impl Reply, Rejection> {
    if encoded_keyfile.is_none() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&"Keyfile not present"),
            StatusCode::NOT_FOUND,
        )
        .into_response());
    }
    let encoded_keyfile = encoded_keyfile.unwrap();
    let mut our = our.as_ref().clone();

    // Get our name from our current keyfile
    let old_decoded_keyfile = match keygen::decode_keyfile(&encoded_keyfile, &info.password) {
        Ok(k) => {
            our.name = k.username.clone();
            k
        }
        Err(_) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&"Invalid password"),
                StatusCode::UNAUTHORIZED,
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

    let decoded_keyfile = Keyfile {
        username: our.name.clone(),
        routers: our.allowed_routers.clone(),
        networking_keypair: signature::Ed25519KeyPair::from_pkcs8(networking_keypair.as_ref())
            .unwrap(),
        jwt_secret_bytes: old_decoded_keyfile.jwt_secret_bytes,
        file_key: old_decoded_keyfile.file_key,
    };

    let encoded_keyfile = keygen::encode_keyfile(
        info.password,
        decoded_keyfile.username.clone(),
        decoded_keyfile.routers.clone(),
        networking_keypair.as_ref(),
        decoded_keyfile.jwt_secret_bytes.clone(),
        decoded_keyfile.file_key.clone(),
    );

    success_response(sender, our.clone(), decoded_keyfile, encoded_keyfile).await
}

async fn success_response(
    sender: Arc<RegistrationSender>,
    our: Identity,
    decoded_keyfile: Keyfile,
    encoded_keyfile: Vec<u8>,
) -> Result<warp::reply::Response, Rejection> {
    let encoded_keyfile_str = base64::encode(&encoded_keyfile);
    let token = match generate_jwt(&decoded_keyfile.jwt_secret_bytes, &our.name) {
        Some(token) => token,
        None => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&"Failed to generate JWT"),
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

    match HeaderValue::from_str(&format!("nectar-auth_{}={};", &our.name, &token)) {
        Ok(v) => {
            headers.append(SET_COOKIE, v);
        }
        Err(_) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&"Failed to generate Auth JWT"),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
    }

    Ok(response)
}

async fn _networking_info_valid(rpc_url: String, ip: String, ws_port: u16, our: &Identity) -> bool {
    // check if Identity for this username has correct networking keys,
    // if not, prompt user to reset them.
    let Ok(ws_rpc) = Provider::<Ws>::connect(rpc_url.clone()).await else {
        return false;
    };
    let Ok(qns_address): Result<EthAddress, _> = _QNS_SEPOLIA_ADDRESS.parse() else {
        return false;
    };
    let contract = QNSRegistry::new(qns_address, ws_rpc.into());
    let node_id: U256 = namehash(&our.name).as_bytes().into();
    let Ok((chain_pubkey, chain_ip, chain_port, chain_routers)) = contract.ws(node_id).call().await
    else {
        return false;
    };

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

    let current_ip = match _ip_to_number(&ip) {
        Ok(ip_num) => ip_num,
        Err(_) => {
            return false;
        }
    };

    let Ok(networking_key_bytes) = _hex_string_to_u8_array(&our.networking_key) else {
        return false;
    };

    let address_match = chain_ip == current_ip && chain_port == ws_port;
    let routers_match = chain_routers == namehashed_routers;

    let routing_match = if chain_ip == 0 {
        routers_match
    } else {
        address_match
    };
    let pubkey_match = chain_pubkey == networking_key_bytes;

    // double check that keys match on-chain information
    if !routing_match || !pubkey_match {
        return false;
    }

    true
}
