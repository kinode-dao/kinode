use aes_gcm::aead::KeyInit;
use alloy_primitives::{Address, Bytes, FixedBytes, U256};
use alloy_providers::provider::{Provider, TempProvider};
use alloy_pubsub::PubSubFrontend;
use alloy_rpc_client::ClientBuilder;
use alloy_rpc_types::request::{TransactionInput, TransactionRequest};
use alloy_signer::Signature;
use alloy_sol_macro::sol;
use alloy_sol_types::{SolCall, SolValue};
use alloy_transport_ws::WsConnect;
use hmac::Hmac;
use jwt::SignWithKey;
use ring::rand::SystemRandom;
use ring::signature;
use ring::signature::KeyPair;
use sha2::Sha256;

use static_dir::static_dir;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, oneshot};
use warp::{
    http::{
        header::{HeaderMap, HeaderValue, SET_COOKIE},
        StatusCode,
    },
    Filter, Rejection, Reply,
};

use crate::keygen;
use lib::types::core::*;

type RegistrationSender = mpsc::Sender<(Identity, Keyfile, Vec<u8>)>;

pub const KNS_SEPOLIA_ADDRESS: Address = Address::new([
    0x38, 0x07, 0xFB, 0xD6, 0x92, 0xAa, 0x5c, 0x96, 0xF1, 0xD8, 0xD7, 0xc5, 0x9a, 0x13, 0x46, 0xa8,
    0x85, 0xF4, 0x0B, 0x1C,
]);

pub const KNS_OPTIMISM_ADDRESS: Address = Address::new([
    0xca, 0x5b, 0x58, 0x11, 0xc0, 0xC4, 0x0a, 0xAB, 0x32, 0x95, 0xf9, 0x32, 0xb1, 0xB5, 0x11, 0x2E,
    0xb7, 0xbb, 0x4b, 0xD6,
]);

sol! {
    function auth(
        bytes32 _node,
        address _sender
    ) public view virtual returns (bool authed);

    function nodes(bytes32) external view returns (address, uint96);
}

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

    let claims = crate::http::server_types::JwtClaims {
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
    keyfile: Option<Vec<u8>>,
    testnet: bool,
) {
    // Networking info is generated and passed to the UI, but not used until confirmed
    let (public_key, serialized_networking_keypair) = keygen::generate_networking_key();
    let net_keypair = Arc::new(serialized_networking_keypair.as_ref().to_vec());
    let tx = Arc::new(tx);

    // TODO: if IP is localhost, don't allow registration as direct
    let ws_port = crate::http::utils::find_open_port(9000, 65535)
        .await
        .expect(
            "Unable to find free port between 9000 and 65535 for a new websocket, are you kidding?",
        );

    // This is a temporary identity, passed to the UI. If it is confirmed through a /boot or /confirm-change-network-keys, then it will be used to replace the current identity
    let our_temp_id = Arc::new(Identity {
        networking_key: format!("0x{}", public_key),
        name: "".to_string(),
        ws_routing: Some((ip.clone(), ws_port)),
        allowed_routers: vec![
            // "next-release-router.os".into(),
            "default-router-1.os".into(),
            "default-router-2.os".into(),
            "default-router-3.os".into(),
        ],
    });

    // KnsRegistrar contract address
    let kns_address = if testnet {
        KNS_SEPOLIA_ADDRESS
    } else {
        KNS_OPTIMISM_ADDRESS
    };

    // This ETH provider uses public rpc endpoints to verify registration signatures.
    let url = if testnet {
        "wss://ethereum-sepolia-rpc.publicnode.com".to_string()
    } else {
        "wss://optimism-rpc.publicnode.com".to_string()
    };
    let connector = WsConnect { url, auth: None };
    let client = ClientBuilder::default().ws(connector).await.unwrap();

    let provider = Arc::new(Provider::new_with_client(client));

    let keyfile = warp::any().map(move || keyfile.clone());
    let our_temp_id = warp::any().map(move || our_temp_id.clone());
    let net_keypair = warp::any().map(move || net_keypair.clone());
    let tx = warp::any().map(move || tx.clone());
    let ip = warp::any().map(move || ip.clone());

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
            .map(move || warp::reply::html(include_str!("register-ui/build/index.html"))))
        .or(warp::path("current-chain").and(warp::get()).map(move || {
            if testnet {
                warp::reply::json(&"0xaa36a7")
            } else {
                warp::reply::json(&"0xa")
            }
        }));

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
                .and_then(move |boot_info, tx, our_temp_id, net_keypair| {
                    let provider = provider.clone();
                    handle_boot(
                        boot_info,
                        tx,
                        our_temp_id,
                        net_keypair,
                        testnet,
                        kns_address,
                        provider,
                    )
                }),
        ))
        .or(warp::path("import-keyfile").and(
            warp::post()
                .and(warp::body::content_length_limit(1024 * 16))
                .and(warp::body::json())
                .and(ip.clone())
                .and(tx.clone())
                .and_then(handle_import_keyfile),
        ))
        .or(warp::path("login").and(
            warp::post()
                .and(warp::body::content_length_limit(1024 * 16))
                .and(warp::body::json())
                .and(ip)
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
                        warp::reply::json(&"keyfile deserialization went wrong"),
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
    return Ok(warp::reply::with_status(
        warp::reply::json(&UnencryptedIdentity {
            name,
            allowed_routers,
        }),
        StatusCode::OK,
    )
    .into_response());
}

async fn generate_networking_info(our_temp_id: Arc<Identity>) -> Result<impl Reply, Rejection> {
    Ok(warp::reply::json(our_temp_id.as_ref()))
}

async fn handle_keyfile_vet(
    payload: KeyfileVet,
    keyfile: Option<Vec<u8>>,
) -> Result<impl Reply, Rejection> {
    // additional checks?
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
    testnet: bool,
    kns_address: Address,
    provider: Arc<Provider<PubSubFrontend>>,
) -> Result<impl Reply, Rejection> {
    let mut our = our.as_ref().clone();

    our.name = info.username;
    if info.direct {
        our.allowed_routers = vec![];
    } else {
        our.ws_routing = None;
    }
    let jwt_seed = SystemRandom::new();
    let mut jwt_secret = [0u8, 32];
    ring::rand::SecureRandom::fill(&jwt_seed, &mut jwt_secret).unwrap();

    // verifying owner + signature, get registrar contract, call auth()
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs();

    if info.timestamp < now + 120 {
        return Ok(warp::reply::with_status(
            warp::reply::json(&"Timestamp is outdated."),
            StatusCode::UNAUTHORIZED,
        )
        .into_response());
    }

    let namehash = FixedBytes::<32>::from_slice(&keygen::namehash(&our.name));
    let tld_call = nodesCall { _0: namehash }.abi_encode();
    let tx_input = TransactionInput::new(Bytes::from(tld_call));
    let tx = TransactionRequest {
        to: Some(kns_address),
        input: tx_input,
        ..Default::default()
    };

    let Ok(tld) = provider.call(tx, None).await else {
        return Ok(warp::reply::with_status(
            warp::reply::json(&"Failed to fetch TLD contract for username"),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response());
    };

    let Ok((tld_address, _)) = <(Address, U256)>::abi_decode(&tld, false) else {
        return Ok(warp::reply::with_status(
            warp::reply::json(&"Failed to decode TLD contract from return bytes"),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response());
    };
    let owner = Address::from_str(&info.owner).map_err(|_| warp::reject())?;

    let auth_call = authCall {
        _node: namehash,
        _sender: owner,
    }
    .abi_encode();
    let tx_input = TransactionInput::new(Bytes::from(auth_call));
    let tx = TransactionRequest {
        to: Some(tld_address),
        input: tx_input,
        ..Default::default()
    };

    let Ok(authed) = provider.call(tx, None).await else {
        return Ok(warp::reply::with_status(
            warp::reply::json(&"Failed to fetch TLD contract for username"),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response());
    };

    let is_ok = bool::abi_decode(&authed, false).map_err(|_| warp::reject())?;

    if !is_ok {
        return Ok(warp::reply::with_status(
            warp::reply::json(&"Address is not authorized for username!"),
            StatusCode::UNAUTHORIZED,
        )
        .into_response());
    };

    // manual json creation to preserve order..
    let sig_data_json = format!(
        r#"{{"username":"{}","password":"{}","timestamp":{}}}"#,
        our.name, info.password, info.timestamp
    );
    let sig_data = sig_data_json.as_bytes();

    let sig = Signature::from_str(&info.signature).map_err(|_| warp::reject())?;

    let recovered_address = sig
        .recover_address_from_msg(sig_data)
        .map_err(|_| warp::reject())?;

    if recovered_address != owner {
        return Ok(warp::reply::with_status(
            warp::reply::json(&"Recovered address does not match owner"),
            StatusCode::UNAUTHORIZED,
        )
        .into_response());
    }

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
        &networking_keypair,
        &decoded_keyfile.jwt_secret_bytes,
        &decoded_keyfile.file_key,
    );

    success_response(sender, our, decoded_keyfile, encoded_keyfile).await
}

async fn handle_import_keyfile(
    info: ImportKeyfileInfo,
    ip: String,
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

    let Some(ws_port) = crate::http::utils::find_open_port(9000, 9999).await else {
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

    let Some(ws_port) = crate::http::utils::find_open_port(9000, 65535).await else {
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
        &networking_keypair,
        &decoded_keyfile.jwt_secret_bytes,
        &decoded_keyfile.file_key,
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

    match HeaderValue::from_str(&format!("kinode-auth_{}={};", &our.name, &token)) {
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
