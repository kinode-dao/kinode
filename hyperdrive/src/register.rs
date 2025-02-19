use crate::{keygen, sol::*};
use crate::{HYPERMAP_ADDRESS, MULTICALL_ADDRESS};
use alloy::providers::{Provider, ProviderBuilder, RootProvider};
use alloy::pubsub::PubSubFrontend;
use alloy::rpc::client::WsConnect;
use alloy::rpc::types::eth::{TransactionInput, TransactionRequest};
use alloy::signers::Signature;
use alloy_primitives::{Address as EthAddress, Bytes, FixedBytes, U256};
use alloy_sol_types::{eip712_domain, SolCall, SolStruct};
use base64::{engine::general_purpose::STANDARD as base64_standard, Engine};
use lib::types::core::{
    BootInfo, Identity, ImportKeyfileInfo, Keyfile, LoginInfo, NodeRouting, UnencryptedIdentity,
};
use ring::{rand::SystemRandom, signature, signature::KeyPair};
use std::{
    str::FromStr,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::{mpsc, oneshot};
use warp::{
    http::{
        header::{HeaderMap, HeaderValue, SET_COOKIE},
        StatusCode,
    },
    Filter, Rejection, Reply,
};
#[cfg(feature = "simulation-mode")]
use {alloy_sol_macro::sol, alloy_sol_types::SolValue};

type RegistrationSender = mpsc::Sender<(Identity, Keyfile, Vec<u8>)>;

/// Serve the registration page and receive POSTs and PUTs from it
pub async fn register(
    tx: RegistrationSender,
    kill_rx: oneshot::Receiver<bool>,
    ip: String,
    ws_networking: (Option<&tokio::net::TcpListener>, bool),
    tcp_networking: (Option<&tokio::net::TcpListener>, bool),
    http_port: u16,
    keyfile: Option<Vec<u8>>,
    maybe_rpc: Option<String>,
    detached: bool,
) {
    // Networking info is generated and passed to the UI, but not used until confirmed
    let (public_key, serialized_networking_keypair) = keygen::generate_networking_key();
    let net_keypair = Arc::new(serialized_networking_keypair.as_ref().to_vec());
    let tx = Arc::new(tx);

    let ws_port = match ws_networking.0 {
        Some(listener) => listener.local_addr().unwrap().port(),
        None => 0,
    };
    let ws_flag_used = ws_networking.1;
    let tcp_port = match tcp_networking.0 {
        Some(listener) => listener.local_addr().unwrap().port(),
        None => 0,
    };
    let tcp_flag_used = tcp_networking.1;

    let mut ports_map = std::collections::BTreeMap::new();
    if ws_port != 0 {
        ports_map.insert("ws".to_string(), ws_port);
    }
    if tcp_port != 0 {
        ports_map.insert("tcp".to_string(), tcp_port);
    }

    // This is a **temporary** identity, passed to the UI.
    // If it is confirmed through a /boot, then it will be used to replace the current identity.
    let our_temp_id = Arc::new(Identity {
        networking_key: format!("0x{}", public_key),
        name: "".to_string(),
        routing: NodeRouting::Both {
            ip: ip.clone(),
            ports: ports_map,
            routers: {
                // select 3 random routers from this list
                use rand::prelude::SliceRandom;
                let routers = (1..=12)
                    .map(|i| format!("default-router-{}.kino", i))
                    .collect::<Vec<_>>()
                    .choose_multiple(&mut rand::thread_rng(), 3)
                    .cloned()
                    .collect::<Vec<_>>();
                routers
            },
        },
    });

    let provider = Arc::new(connect_to_provider(maybe_rpc).await);

    let keyfile = warp::any().map(move || keyfile.clone());
    let our_temp_id = warp::any().map(move || our_temp_id.clone());
    let net_keypair = warp::any().map(move || net_keypair.clone());
    let tx = warp::any().map(move || tx.clone());
    let ip = warp::any().map(move || ip.clone());
    let ws_port = warp::any().map(move || (ws_port, ws_flag_used));
    let tcp_port = warp::any().map(move || (tcp_port, tcp_flag_used));

    #[cfg(unix)]
    let static_files =
        warp::path("assets").and(static_dir::static_dir!("src/register-ui/build/assets/"));
    #[cfg(target_os = "windows")]
    let static_files =
        warp::path("assets").and(static_dir::static_dir!("src\\register-ui\\build\\assets\\"));

    #[cfg(unix)]
    let react_app = warp::path::end()
        .or(warp::path("login"))
        .or(warp::path("commit-os-name"))
        .or(warp::path("mint-os-name"))
        .or(warp::path("claim-invite"))
        .or(warp::path("reset"))
        .or(warp::path("import-keyfile"))
        .or(warp::path("set-password"))
        .or(warp::path("custom-register"))
        .and(warp::get())
        .map(move |_| warp::reply::html(include_str!("register-ui/build/index.html")));
    #[cfg(target_os = "windows")]
    let react_app = warp::path::end()
        .or(warp::path("login"))
        .or(warp::path("commit-os-name"))
        .or(warp::path("mint-os-name"))
        .or(warp::path("claim-invite"))
        .or(warp::path("reset"))
        .or(warp::path("import-keyfile"))
        .or(warp::path("set-password"))
        .or(warp::path("custom-register"))
        .and(warp::get())
        .map(move |_| warp::reply::html(include_str!("register-ui\\build\\index.html")));

    let boot_provider = provider.clone();
    let login_provider = provider.clone();
    let import_provider = provider.clone();

    let api = warp::path("info")
        .and(
            warp::get()
                .and(keyfile.clone())
                .and_then(get_unencrypted_info),
        )
        .or(warp::path("current-chain")
            .and(warp::get())
            .map(move || warp::reply::json(&"0xa")))
        .or(warp::path("our").and(warp::get()).and(keyfile.clone()).map(
            move |keyfile: Option<Vec<u8>>| {
                if let Some(keyfile) = keyfile {
                    if let Ok((username, _, _, _, _, _)) = serde_json::from_slice::<(
                        String,
                        Vec<String>,
                        Vec<u8>,
                        Vec<u8>,
                        Vec<u8>,
                        Vec<u8>,
                    )>(&keyfile)
                    .or_else(|_| {
                        bincode::deserialize::<(
                            String,
                            Vec<String>,
                            Vec<u8>,
                            Vec<u8>,
                            Vec<u8>,
                            Vec<u8>,
                        )>(&keyfile)
                    }) {
                        return warp::reply::html(username);
                    }
                }
                warp::reply::html(String::new())
            },
        ))
        .or(warp::path("generate-networking-info").and(
            warp::post()
                .and(our_temp_id.clone())
                .and_then(generate_networking_info),
        ))
        .or(warp::path("boot").and(
            warp::post()
                .and(warp::body::content_length_limit(1024 * 16))
                .and(warp::body::json())
                .and(tx.clone())
                .and(our_temp_id.clone())
                .and(net_keypair.clone())
                .and_then(move |boot_info, tx, our_temp_id, net_keypair| {
                    let boot_provider = boot_provider.clone();
                    handle_boot(boot_info, tx, our_temp_id, net_keypair, boot_provider)
                }),
        ))
        .or(warp::path("import-keyfile").and(
            warp::post()
                .and(warp::body::content_length_limit(1024 * 16))
                .and(warp::body::json())
                .and(ip.clone())
                .and(ws_port.clone())
                .and(tcp_port.clone())
                .and(tx.clone())
                .and_then(move |boot_info, ip, ws_port, tcp_port, tx| {
                    let import_provider = import_provider.clone();
                    handle_import_keyfile(boot_info, ip, ws_port, tcp_port, tx, import_provider)
                }),
        ))
        .or(warp::path("login").and(
            warp::post()
                .and(warp::body::content_length_limit(1024 * 16))
                .and(warp::body::json())
                .and(ip)
                .and(ws_port.clone())
                .and(tcp_port.clone())
                .and(tx.clone())
                .and(keyfile.clone())
                .and_then(move |boot_info, ip, ws_port, tcp_port, tx, keyfile| {
                    let login_provider = login_provider.clone();
                    handle_login(
                        boot_info,
                        ip,
                        ws_port,
                        tcp_port,
                        tx,
                        keyfile,
                        login_provider,
                    )
                }),
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

    if !detached {
        let _ = open::that(format!("http://localhost:{}/", http_port));
    }
    warp::serve(routes)
        .bind_with_graceful_shutdown(([0, 0, 0, 0], http_port), async {
            kill_rx.await.ok();
        })
        .1
        .await;
}

pub async fn connect_to_provider(maybe_rpc: Option<String>) -> RootProvider<PubSubFrontend> {
    let url = if let Some(rpc_url) = maybe_rpc {
        rpc_url
    } else {
        "wss://base-rpc.publicnode.com".to_string()
    };

    let client = match ProviderBuilder::new().on_ws(WsConnect::new(url)).await {
        Ok(client) => client,
        Err(e) => {
            panic!(
                "Error: runtime could not connect to ETH RPC: {e}\n\
                This is necessary in order to verify node identity onchain.\n\
                Please make sure you are using a valid WebSockets URL if using \
                the --rpc flag, and you are connected to the internet."
            );
        }
    };

    client
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

async fn handle_boot(
    info: BootInfo,
    sender: Arc<RegistrationSender>,
    our: Arc<Identity>,
    networking_keypair: Arc<Vec<u8>>,
    provider: Arc<RootProvider<PubSubFrontend>>,
) -> Result<impl Reply, Rejection> {
    let hypermap = EthAddress::from_str(HYPERMAP_ADDRESS).unwrap();
    let mut our = our.as_ref().clone();

    our.name = info.username;
    if info.direct {
        our.both_to_direct();
    } else {
        our.both_to_routers();
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
    let Ok(password_hash) = FixedBytes::<32>::from_str(&info.password_hash) else {
        return Ok(warp::reply::with_status(
            warp::reply::json(&"Invalid password hash"),
            StatusCode::UNAUTHORIZED,
        )
        .into_response());
    };

    let namehash = FixedBytes::<32>::from_slice(&keygen::namehash(&our.name));

    let get_call = getCall { namehash }.abi_encode();
    let tx_input = TransactionInput::new(Bytes::from(get_call));

    let tx = TransactionRequest::default().to(hypermap).input(tx_input);

    // this call can fail if the indexer has not caught up to the transaction
    // that just got confirmed on our frontend. for this reason, we retry
    // the call a few times before giving up.
    let mut attempts = 0;
    while attempts < 5 {
        match provider.call(&tx).await {
            Ok(get) => {
                let Ok(node_info) = getCall::abi_decode_returns(&get, false) else {
                    return Ok(warp::reply::with_status(
                        warp::reply::json(&"Failed to decode hypermap entry from return bytes"),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    )
                    .into_response());
                };
                let owner = node_info.owner;

                let chain_id: u64 = 8453; // base

                let domain = eip712_domain! {
                    name: "Hypermap",
                    version: "1",
                    chain_id: chain_id,
                    verifying_contract: hypermap,
                };

                let boot = Boot {
                    username: our.name.clone(),
                    password_hash,
                    timestamp: U256::from(info.timestamp),
                    direct: info.direct,
                    reset: info.reset,
                    chain_id: U256::from(chain_id),
                };

                let hash = boot.eip712_signing_hash(&domain);
                let sig = Signature::from_str(&info.signature).map_err(|_| warp::reject())?;

                let recovered_address = sig
                    .recover_address_from_prehash(&hash)
                    .map_err(|_| warp::reject())?;

                if recovered_address != owner {
                    println!("recovered_address: {}\r", recovered_address);
                    println!("owner: {}\r", owner);
                    attempts += 1;
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    continue;
                }

                let decoded_keyfile = Keyfile {
                    username: our.name.clone(),
                    routers: our.routers().unwrap_or(&vec![]).clone(),
                    networking_keypair: signature::Ed25519KeyPair::from_pkcs8(
                        networking_keypair.as_ref(),
                    )
                    .unwrap(),
                    jwt_secret_bytes: jwt_secret.to_vec(),
                    file_key: keygen::generate_file_key(),
                };

                let encoded_keyfile = keygen::encode_keyfile(
                    info.password_hash,
                    decoded_keyfile.username.clone(),
                    decoded_keyfile.routers.clone(),
                    &networking_keypair,
                    &decoded_keyfile.jwt_secret_bytes,
                    &decoded_keyfile.file_key,
                );

                return success_response(sender, our, decoded_keyfile, encoded_keyfile).await;
            }
            Err(_) => {
                attempts += 1;
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            }
        }
    }

    return Ok(warp::reply::with_status(
        warp::reply::json(&"Recovered address does not match owner"),
        StatusCode::UNAUTHORIZED,
    )
    .into_response());
}

async fn handle_import_keyfile(
    info: ImportKeyfileInfo,
    ip: String,
    ws_networking_port: (u16, bool),
    tcp_networking_port: (u16, bool),
    sender: Arc<RegistrationSender>,
    provider: Arc<RootProvider<PubSubFrontend>>,
) -> Result<impl Reply, Rejection> {
    println!("received base64 keyfile: {}\r", info.keyfile);
    // if keyfile was not present in node and is present from user upload
    let encoded_keyfile = match base64_standard.decode(info.keyfile) {
        Ok(k) => k,
        Err(_) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&"Keyfile not valid base64"),
                StatusCode::BAD_REQUEST,
            )
            .into_response())
        }
    };

    println!(
        "received keyfile: {}\r",
        String::from_utf8_lossy(&encoded_keyfile)
    );
    let (decoded_keyfile, mut our) =
        match keygen::decode_keyfile(&encoded_keyfile, &info.password_hash) {
            Ok(k) => {
                let our = Identity {
                    name: k.username.clone(),
                    networking_key: format!(
                        "0x{}",
                        hex::encode(k.networking_keypair.public_key().as_ref())
                    ),
                    routing: if k.routers.is_empty() {
                        NodeRouting::Direct {
                            ip,
                            ports: std::collections::BTreeMap::new(),
                        }
                    } else {
                        NodeRouting::Routers(k.routers.clone())
                    },
                };

                (k, our)
            }
            Err(_) => {
                return Ok(warp::reply::with_status(
                    warp::reply::json(&"Incorrect password!".to_string()),
                    StatusCode::UNAUTHORIZED,
                )
                .into_response())
            }
        };

    if let Err(e) =
        assign_routing(&mut our, provider, ws_networking_port, tcp_networking_port).await
    {
        return Ok(warp::reply::with_status(
            warp::reply::json(&e.to_string()),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response());
    }
    success_response(sender, our, decoded_keyfile, encoded_keyfile).await
}

async fn handle_login(
    info: LoginInfo,
    ip: String,
    ws_networking_port: (u16, bool),
    tcp_networking_port: (u16, bool),
    sender: Arc<RegistrationSender>,
    encoded_keyfile: Option<Vec<u8>>,
    provider: Arc<RootProvider<PubSubFrontend>>,
) -> Result<impl Reply, Rejection> {
    if encoded_keyfile.is_none() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&"Keyfile not present"),
            StatusCode::NOT_FOUND,
        )
        .into_response());
    }
    let encoded_keyfile = encoded_keyfile.unwrap();

    let (decoded_keyfile, mut our) =
        match keygen::decode_keyfile(&encoded_keyfile, &info.password_hash) {
            Ok(k) => {
                let our = Identity {
                    name: k.username.clone(),
                    networking_key: format!(
                        "0x{}",
                        hex::encode(k.networking_keypair.public_key().as_ref())
                    ),
                    routing: if k.routers.is_empty() {
                        NodeRouting::Direct {
                            ip,
                            ports: std::collections::BTreeMap::new(),
                        }
                    } else {
                        NodeRouting::Routers(k.routers.clone())
                    },
                };

                (k, our)
            }
            Err(_) => {
                return Ok(warp::reply::with_status(
                    warp::reply::json(&"Incorrect password!"),
                    StatusCode::UNAUTHORIZED,
                )
                .into_response())
            }
        };

    if let Err(e) =
        assign_routing(&mut our, provider, ws_networking_port, tcp_networking_port).await
    {
        return Ok(warp::reply::with_status(
            warp::reply::json(&e.to_string()),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response());
    }
    success_response(sender, our, decoded_keyfile, encoded_keyfile).await
}

pub async fn assign_routing(
    our: &mut Identity,
    provider: Arc<RootProvider<PubSubFrontend>>,
    ws_networking_port: (u16, bool),
    tcp_networking_port: (u16, bool),
) -> anyhow::Result<()> {
    let multicall = EthAddress::from_str(MULTICALL_ADDRESS)?;
    let hypermap = EthAddress::from_str(HYPERMAP_ADDRESS)?;

    let netkey_hash =
        FixedBytes::<32>::from_slice(&keygen::namehash(&format!("~net-key.{}", our.name)));
    let ws_hash =
        FixedBytes::<32>::from_slice(&keygen::namehash(&format!("~ws-port.{}", our.name)));
    let tcp_hash =
        FixedBytes::<32>::from_slice(&keygen::namehash(&format!("~tcp-port.{}", our.name)));
    let ip_hash = FixedBytes::<32>::from_slice(&keygen::namehash(&format!("~ip.{}", our.name)));

    let multicalls = vec![
        Call {
            target: hypermap,
            callData: Bytes::from(
                getCall {
                    namehash: netkey_hash,
                }
                .abi_encode(),
            ),
        },
        Call {
            target: hypermap,
            callData: Bytes::from(getCall { namehash: ws_hash }.abi_encode()),
        },
        Call {
            target: hypermap,
            callData: Bytes::from(getCall { namehash: tcp_hash }.abi_encode()),
        },
        Call {
            target: hypermap,
            callData: Bytes::from(getCall { namehash: ip_hash }.abi_encode()),
        },
    ];

    let multicall_call = aggregateCall { calls: multicalls }.abi_encode();
    let tx_input = TransactionInput::new(Bytes::from(multicall_call));
    let tx = TransactionRequest::default().to(multicall).input(tx_input);

    let Ok(multicall_return) = provider.call(&tx).await else {
        return Err(anyhow::anyhow!("Failed to fetch node IP data from hypermap"));
    };

    let Ok(results) = aggregateCall::abi_decode_returns(&multicall_return, false) else {
        return Err(anyhow::anyhow!("Failed to decode hypermap multicall data"));
    };

    let netkey = getCall::abi_decode_returns(&results.returnData[0], false)?;
    let ws = getCall::abi_decode_returns(&results.returnData[1], false)?;
    let tcp = getCall::abi_decode_returns(&results.returnData[2], false)?;
    let ip = getCall::abi_decode_returns(&results.returnData[3], false)?;

    let ip = keygen::bytes_to_ip(&ip.data);
    let ws = keygen::bytes_to_port(&ws.data);
    let tcp = keygen::bytes_to_port(&tcp.data);

    if netkey.data.to_string() != our.networking_key {
        return Err(anyhow::anyhow!(
            "Networking key from PKI ({}) does not match our saved networking key ({})",
            netkey.data.to_string(),
            our.networking_key
        ));
    }

    if !our.is_direct() {
        // indirect node
        return Ok(());
    }

    if ip.is_ok() && (ws.is_ok() || tcp.is_ok()) {
        // direct node
        let mut ports = std::collections::BTreeMap::new();
        if let Ok(ws) = ws {
            if ws_networking_port.1 && ws != ws_networking_port.0 {
                return Err(anyhow::anyhow!(
                    "Binary used --ws-port flag to set port to {}, but node is using port {} onchain.",
                    ws_networking_port.0,
                    ws
                ));
            }
            ports.insert("ws".to_string(), ws);
        }
        if let Ok(tcp) = tcp {
            if tcp_networking_port.1 && tcp != tcp_networking_port.0 {
                return Err(anyhow::anyhow!(
                    "Binary used --tcp-port flag to set port to {}, but node is using port {} onchain.",
                    tcp_networking_port.0,
                    tcp
                ));
            }
            ports.insert("tcp".to_string(), tcp);
        }
        our.routing = NodeRouting::Direct {
            ip: ip.unwrap().to_string(),
            ports,
        };
    } else {
        // indirect node
    }
    Ok(())
}

async fn success_response(
    sender: Arc<RegistrationSender>,
    our: Identity,
    decoded_keyfile: Keyfile,
    encoded_keyfile: Vec<u8>,
) -> Result<warp::reply::Response, Rejection> {
    let encoded_keyfile_str = base64_standard.encode(&encoded_keyfile);
    let token = match keygen::generate_jwt(&decoded_keyfile.jwt_secret_bytes, &our.name, &None) {
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

    match HeaderValue::from_str(&format!("hyperware-auth_{}={token};", our.name)) {
        Ok(v) => {
            response.headers_mut().append(SET_COOKIE, v);
            response
                .headers_mut()
                .append("HttpOnly", HeaderValue::from_static("true"));
            response
                .headers_mut()
                .append("Secure", HeaderValue::from_static("true"));
            response
                .headers_mut()
                .append("SameSite", HeaderValue::from_static("Strict"));
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
