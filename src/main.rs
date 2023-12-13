#![feature(btree_extract_if)]

use crate::types::*;
use anyhow::Result;
use clap::{arg, value_parser, Command};
use std::env;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio::{fs, time::timeout};

#[cfg(feature = "simulation-mode")]
use ring::{rand::SystemRandom, signature, signature::KeyPair};

mod eth_rpc;
mod filesystem;
mod http;
mod kernel;
mod keygen;
mod net;
mod register;
mod terminal;
mod timer;
mod types;
mod vfs;

const EVENT_LOOP_CHANNEL_CAPACITY: usize = 10_000;
const EVENT_LOOP_DEBUG_CHANNEL_CAPACITY: usize = 50;
const TERMINAL_CHANNEL_CAPACITY: usize = 32;
const WEBSOCKET_SENDER_CHANNEL_CAPACITY: usize = 32;
const FILESYSTEM_CHANNEL_CAPACITY: usize = 32;
const HTTP_CHANNEL_CAPACITY: usize = 32;
const HTTP_CLIENT_CHANNEL_CAPACITY: usize = 32;
const ETH_RPC_CHANNEL_CAPACITY: usize = 32;
const VFS_CHANNEL_CAPACITY: usize = 1_000;
const CAP_CHANNEL_CAPACITY: usize = 1_000;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// This can and should be an environment variable / setting. It configures networking
/// such that indirect nodes always use routers, even when target is a direct node,
/// such that only their routers can ever see their physical networking details.
const REVEAL_IP: bool = true;

async fn serve_register_fe(
    home_directory_path: &str,
    our_ip: String,
    http_server_port: u16,
    rpc_url: String,
) -> (Identity, Vec<u8>, Keyfile) {
    // check if we have keys saved on disk, encrypted
    // if so, prompt user for "password" to decrypt with

    // once password is received, use to decrypt local keys file,
    // and pass the keys into boot process as is done in registration.

    // NOTE: when we log in, we MUST check the PKI to make sure our
    // information matches what we think it should be. this includes
    // username, networking key, and routing info.
    // if any do not match, we should prompt user to create a "transaction"
    // that updates their PKI info on-chain.
    let (kill_tx, kill_rx) = oneshot::channel::<bool>();

    let disk_keyfile: Option<Vec<u8>> = fs::read(format!("{}/.keys", home_directory_path))
        .await
        .ok();

    let (tx, mut rx) = mpsc::channel::<(Identity, Keyfile, Vec<u8>)>(1);
    let (our, decoded_keyfile, encoded_keyfile) = tokio::select! {
        _ = register::register(tx, kill_rx, our_ip, http_server_port, rpc_url, disk_keyfile) => {
            panic!("registration failed")
        }
        Some((our, decoded_keyfile, encoded_keyfile)) = rx.recv() => {
            (our, decoded_keyfile, encoded_keyfile)
        }
    };

    fs::write(
        format!("{}/.keys", home_directory_path),
        encoded_keyfile.clone(),
    )
    .await
    .unwrap();

    let _ = kill_tx.send(true);

    (our, encoded_keyfile, decoded_keyfile)
}

#[tokio::main]
async fn main() {
    let app = Command::new("Uqbar")
        .version(VERSION)
        .author("Uqbar DAO: https://github.com/uqbar-dao")
        .about("A General Purpose Sovereign Cloud Computing Platform")
        .arg(arg!([home] "Path to home directory").required(true))
        .arg(
            arg!(--port <PORT> "First port to try binding")
                .default_value("8080")
                .value_parser(value_parser!(u16)),
        );

    #[cfg(not(feature = "simulation-mode"))]
    let app = app.arg(arg!(--rpc <WS_URL> "Ethereum RPC endpoint (must be wss://)").required(true));

    #[cfg(feature = "simulation-mode")]
    let app = app
        .arg(arg!(--rpc <WS_URL> "Ethereum RPC endpoint (must be wss://)"))
        .arg(arg!(--password <PASSWORD> "Networking password"))
        .arg(arg!(--"fake-node-name" <NAME> "Name of fake node to boot"))
        .arg(
            arg!(--"network-router-port" <PORT> "Network router port")
                .default_value("9001")
                .value_parser(value_parser!(u16)),
        );

    let matches = app.get_matches();

    let home_directory_path = matches.get_one::<String>("home").unwrap();
    let port = matches.get_one::<u16>("port").unwrap().clone();

    #[cfg(not(feature = "simulation-mode"))]
    let rpc_url = matches.get_one::<String>("rpc").unwrap();

    #[cfg(feature = "simulation-mode")]
    let (rpc_url, password, network_router_port, fake_node_name) = (
        matches.get_one::<String>("rpc"),
        matches.get_one::<String>("password"),
        matches
            .get_one::<u16>("network-router-port")
            .unwrap()
            .clone(),
        matches.get_one::<String>("fake-node-name"),
    );

    if let Err(e) = fs::create_dir_all(home_directory_path).await {
        panic!("failed to create home directory: {:?}", e);
    }
    println!("home at {}\r", home_directory_path);

    // kernel receives system messages via this channel, all other modules send messages
    let (kernel_message_sender, kernel_message_receiver): (MessageSender, MessageReceiver) =
        mpsc::channel(EVENT_LOOP_CHANNEL_CAPACITY);
    // kernel informs other runtime modules of capabilities through this
    let (caps_oracle_sender, caps_oracle_receiver): (CapMessageSender, CapMessageReceiver) =
        mpsc::channel(CAP_CHANNEL_CAPACITY);
    // networking module sends error messages to kernel
    let (network_error_sender, network_error_receiver): (NetworkErrorSender, NetworkErrorReceiver) =
        mpsc::channel(EVENT_LOOP_CHANNEL_CAPACITY);
    // kernel receives debug messages via this channel, terminal sends messages
    let (kernel_debug_message_sender, kernel_debug_message_receiver): (DebugSender, DebugReceiver) =
        mpsc::channel(EVENT_LOOP_DEBUG_CHANNEL_CAPACITY);
    // websocket sender receives send messages via this channel, kernel send messages
    let (net_message_sender, net_message_receiver): (MessageSender, MessageReceiver) =
        mpsc::channel(WEBSOCKET_SENDER_CHANNEL_CAPACITY);
    // filesystem receives request messages via this channel, kernel sends messages
    let (fs_message_sender, fs_message_receiver): (MessageSender, MessageReceiver) =
        mpsc::channel(FILESYSTEM_CHANNEL_CAPACITY);
    // http server channel w/ websockets (eyre)
    let (http_server_sender, http_server_receiver): (MessageSender, MessageReceiver) =
        mpsc::channel(HTTP_CHANNEL_CAPACITY);
    let (timer_service_sender, timer_service_receiver): (MessageSender, MessageReceiver) =
        mpsc::channel(HTTP_CHANNEL_CAPACITY);
    let (eth_rpc_sender, eth_rpc_receiver): (MessageSender, MessageReceiver) =
        mpsc::channel(ETH_RPC_CHANNEL_CAPACITY);
    // http client performs http requests on behalf of processes
    let (http_client_sender, http_client_receiver): (MessageSender, MessageReceiver) =
        mpsc::channel(HTTP_CLIENT_CHANNEL_CAPACITY);
    // vfs maintains metadata about files in fs for processes
    let (vfs_message_sender, vfs_message_receiver): (MessageSender, MessageReceiver) =
        mpsc::channel(VFS_CHANNEL_CAPACITY);
    // terminal receives prints via this channel, all other modules send prints
    let (print_sender, print_receiver): (PrintSender, PrintReceiver) =
        mpsc::channel(TERMINAL_CHANNEL_CAPACITY);

    //  fs config in .env file (todo add -- arguments cleanly (with clap?))
    dotenv::dotenv().ok();

    let mem_buffer_limit = env::var("MEM_BUFFER_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1024 * 1024 * 5); // 5mb default

    let read_cache_limit = env::var("READ_CACHE_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1024 * 1024 * 5); // 5mb default

    let chunk_size = env::var("CHUNK_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1024 * 256); // 256kb default

    let flush_to_cold_interval = env::var("FLUSH_TO_COLD_INTERVAL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60); // 60s default

    let encryption = env::var("ENCRYPTION")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(true); // default true

    let cloud_enabled = env::var("CLOUD_ENABLED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(false); // default false

    let s3_config = if let (Ok(access_key), Ok(secret_key), Ok(region), Ok(bucket), Ok(endpoint)) = (
        env::var("S3_ACCESS_KEY"),
        env::var("S3_SECRET_KEY"),
        env::var("S3_REGION"),
        env::var("S3_BUCKET"),
        env::var("S3_ENDPOINT"),
    ) {
        Some(S3Config {
            access_key,
            secret_key,
            region,
            bucket,
            endpoint,
        })
    } else {
        None
    };

    let fs_config = FsConfig {
        s3_config,
        mem_buffer_limit,
        read_cache_limit,
        chunk_size,
        flush_to_cold_interval,
        encryption,
        cloud_enabled,
    };

    // shutdown signal send and await to fs
    let (fs_kill_send, fs_kill_recv) = oneshot::channel::<()>();
    let (fs_kill_confirm_send, fs_kill_confirm_recv) = oneshot::channel::<()>();

    println!("finding public IP address...");
    let our_ip: std::net::Ipv4Addr = {
        if let Ok(Some(ip)) = timeout(std::time::Duration::from_secs(5), public_ip::addr_v4()).await
        {
            ip
        } else {
            println!(
                "\x1b[38;5;196mfailed to find public IPv4 address: booting as a routed node\x1b[0m"
            );
            std::net::Ipv4Addr::LOCALHOST
        }
    };

    let http_server_port = http::utils::find_open_port(port).await.unwrap();
    println!(
        "login or register at http://localhost:{}\r",
        http_server_port
    );
    #[cfg(not(feature = "simulation-mode"))]
    let (our, encoded_keyfile, decoded_keyfile) = serve_register_fe(
        &home_directory_path,
        our_ip.to_string(),
        http_server_port.clone(),
        rpc_url.clone(),
    )
    .await;
    #[cfg(feature = "simulation-mode")]
    let (our, encoded_keyfile, decoded_keyfile) = match fake_node_name {
        None => {
            match password {
                None => match rpc_url {
                    None => panic!(""),
                    Some(rpc_url) => {
                        serve_register_fe(
                            &home_directory_path,
                            our_ip.to_string(),
                            http_server_port.clone(),
                            rpc_url.clone(),
                        )
                        .await
                    }
                },
                Some(password) => {
                    match fs::read(format!("{}/.keys", home_directory_path)).await {
                        Err(e) => panic!("could not read keyfile: {}", e),
                        Ok(keyfile) => {
                            match keygen::decode_keyfile(&keyfile, &password) {
                                Err(e) => panic!("could not decode keyfile: {}", e),
                                Ok(decoded_keyfile) => {
                                    let our = Identity {
                                        name: decoded_keyfile.username.clone(),
                                        networking_key: format!(
                                            "0x{}",
                                            hex::encode(
                                                decoded_keyfile
                                                    .networking_keypair
                                                    .public_key()
                                                    .as_ref()
                                            )
                                        ),
                                        ws_routing: None, //  TODO
                                        allowed_routers: decoded_keyfile.routers.clone(),
                                    };
                                    (our, keyfile, decoded_keyfile)
                                }
                            }
                        }
                    }
                }
            }
        }
        Some(name) => {
            let password = match password {
                None => "123".to_string(),
                Some(password) => password.to_string(),
            };
            let (pubkey, networking_keypair) = keygen::generate_networking_key();

            let seed = SystemRandom::new();
            let mut jwt_secret = [0u8, 32];
            ring::rand::SecureRandom::fill(&seed, &mut jwt_secret).unwrap();

            let our = Identity {
                name: name.clone(),
                networking_key: pubkey,
                ws_routing: None,
                allowed_routers: vec![],
            };

            let decoded_keyfile = Keyfile {
                username: name.clone(),
                routers: vec![],
                networking_keypair: signature::Ed25519KeyPair::from_pkcs8(
                    networking_keypair.as_ref(),
                )
                .unwrap(),
                jwt_secret_bytes: jwt_secret.to_vec(),
                file_key: keygen::generate_file_key(),
            };

            let encoded_keyfile = keygen::encode_keyfile(
                password,
                name.clone(),
                decoded_keyfile.routers.clone(),
                networking_keypair.as_ref(),
                decoded_keyfile.jwt_secret_bytes.clone(),
                decoded_keyfile.file_key.clone(),
            );

            fs::write(
                format!("{}/.keys", home_directory_path),
                encoded_keyfile.clone(),
            )
            .await
            .unwrap();

            (our, encoded_keyfile, decoded_keyfile)
        }
    };

    // the boolean flag determines whether the runtime module is *public* or not,
    // where public means that any process can always message it.
    #[allow(unused_mut)]
    let mut runtime_extensions = vec![
        (
            ProcessId::new(Some("filesystem"), "sys", "uqbar"),
            fs_message_sender,
            false,
        ),
        (
            ProcessId::new(Some("http_server"), "sys", "uqbar"),
            http_server_sender,
            true,
        ),
        (
            ProcessId::new(Some("http_client"), "sys", "uqbar"),
            http_client_sender,
            false,
        ),
        (
            ProcessId::new(Some("timer"), "sys", "uqbar"),
            timer_service_sender,
            true,
        ),
        (
            ProcessId::new(Some("eth_rpc"), "sys", "uqbar"),
            eth_rpc_sender,
            true,
        ),
        (
            ProcessId::new(Some("vfs"), "sys", "uqbar"),
            vfs_message_sender,
            true,
        ),
    ];

    let (kernel_process_map, manifest, vfs_messages) = filesystem::load_fs(
        our.name.clone(),
        home_directory_path.clone(),
        decoded_keyfile.file_key,
        fs_config,
        runtime_extensions.clone(),
    )
    .await
    .expect("fs load failed!");

    /*
     *  the kernel module will handle our userspace processes and receives
     *  all "messages", the basic message format for uqbar.
     *
     *  if any of these modules fail, the program exits with an error.
     */
    let networking_keypair_arc = Arc::new(decoded_keyfile.networking_keypair);

    let mut tasks = tokio::task::JoinSet::<Result<()>>::new();
    tasks.spawn(kernel::kernel(
        our.clone(),
        networking_keypair_arc.clone(),
        kernel_process_map.clone(),
        caps_oracle_sender.clone(),
        caps_oracle_receiver,
        kernel_message_sender.clone(),
        print_sender.clone(),
        kernel_message_receiver,
        network_error_receiver,
        kernel_debug_message_receiver,
        net_message_sender.clone(),
        runtime_extensions,
    ));
    #[cfg(not(feature = "simulation-mode"))]
    tasks.spawn(net::networking(
        our.clone(),
        our_ip.to_string(),
        networking_keypair_arc.clone(),
        kernel_message_sender.clone(),
        network_error_sender,
        print_sender.clone(),
        net_message_sender,
        net_message_receiver,
        REVEAL_IP,
    ));
    #[cfg(feature = "simulation-mode")]
    tasks.spawn(net::mock_client(
        network_router_port,
        our.name.clone(),
        kernel_message_sender.clone(),
        net_message_receiver,
    ));
    tasks.spawn(filesystem::fs_sender(
        our.name.clone(),
        manifest,
        kernel_message_sender.clone(),
        print_sender.clone(),
        fs_message_receiver,
        fs_kill_recv,
        fs_kill_confirm_send,
    ));
    tasks.spawn(http::server::http_server(
        our.name.clone(),
        http_server_port,
        encoded_keyfile,
        decoded_keyfile.jwt_secret_bytes.clone(),
        http_server_receiver,
        kernel_message_sender.clone(),
        print_sender.clone(),
    ));
    tasks.spawn(http::client::http_client(
        our.name.clone(),
        kernel_message_sender.clone(),
        http_client_receiver,
        print_sender.clone(),
    ));
    tasks.spawn(timer::timer_service(
        our.name.clone(),
        kernel_message_sender.clone(),
        timer_service_receiver,
        print_sender.clone(),
    ));
    #[cfg(not(feature = "simulation-mode"))]
    tasks.spawn(eth_rpc::eth_rpc(
        our.name.clone(),
        rpc_url.clone(),
        kernel_message_sender.clone(),
        eth_rpc_receiver,
        print_sender.clone(),
    ));
    tasks.spawn(vfs::vfs(
        our.name.clone(),
        kernel_message_sender.clone(),
        print_sender.clone(),
        vfs_message_receiver,
        caps_oracle_sender.clone(),
        vfs_messages,
    ));
    // if a runtime task exits, try to recover it,
    // unless it was terminal signaling a quit
    let quit_msg: String = tokio::select! {
        Some(Ok(res)) = tasks.join_next() => {
            format!(
                "\x1b[38;5;196muh oh, a kernel process crashed -- this should never happen: {:?}\x1b[0m",
                res
            )
        }
        quit = terminal::terminal(
            our.clone(),
            VERSION,
            home_directory_path.into(),
            kernel_message_sender.clone(),
            kernel_debug_message_sender,
            print_sender.clone(),
            print_receiver,
        ) => {
            match quit {
                Ok(_) => "graceful exit".into(),
                Err(e) => e.to_string(),
            }
        }
    };

    // shutdown signal to fs for flush
    let _ = fs_kill_send.send(());
    let _ = fs_kill_confirm_recv.await;

    // gracefully abort all running processes in kernel
    let _ = kernel_message_sender
        .send(KernelMessage {
            id: rand::random(),
            source: Address {
                node: our.name.clone(),
                process: KERNEL_PROCESS_ID.clone(),
            },
            target: Address {
                node: our.name.clone(),
                process: KERNEL_PROCESS_ID.clone(),
            },
            rsvp: None,
            message: Message::Request(Request {
                inherit: false,
                expects_response: None,
                ipc: serde_json::to_vec(&KernelCommand::Shutdown).unwrap(),
                metadata: None,
            }),
            payload: None,
            signed_capabilities: None,
        })
        .await;

    // abort all remaining tasks
    tasks.shutdown().await;
    let _ = crossterm::terminal::disable_raw_mode();
    println!("\r\n\x1b[38;5;196m{}\x1b[0m", quit_msg);
    return;
}
