#![feature(async_closure)]
#![feature(btree_extract_if)]
use anyhow::Result;
use clap::{arg, value_parser, Command};
use lib::types::core::*;
#[cfg(feature = "simulation-mode")]
use ring::{rand::SystemRandom, signature, signature::KeyPair};
use std::env;
use std::sync::Arc;
use tokio::sync::mpsc;

mod eth;
#[cfg(feature = "simulation-mode")]
mod fakenet;
mod http;
mod kernel;
mod keygen;
mod kv;
mod net;
#[cfg(not(feature = "simulation-mode"))]
mod register;
mod sqlite;
mod state;
mod terminal;
mod timer;
mod vfs;

const EVENT_LOOP_CHANNEL_CAPACITY: usize = 10_000;
const EVENT_LOOP_DEBUG_CHANNEL_CAPACITY: usize = 50;
const TERMINAL_CHANNEL_CAPACITY: usize = 32;
const WEBSOCKET_SENDER_CHANNEL_CAPACITY: usize = 32;
const HTTP_CHANNEL_CAPACITY: usize = 32;
const HTTP_CLIENT_CHANNEL_CAPACITY: usize = 32;
const ETH_PROVIDER_CHANNEL_CAPACITY: usize = 32;
const VFS_CHANNEL_CAPACITY: usize = 1_000;
const CAP_CHANNEL_CAPACITY: usize = 1_000;
const KV_CHANNEL_CAPACITY: usize = 1_000;
const SQLITE_CHANNEL_CAPACITY: usize = 1_000;
const VERSION: &str = env!("CARGO_PKG_VERSION");
const WS_MIN_PORT: u16 = 9_000;
const TCP_MIN_PORT: u16 = 10_000;
const MAX_PORT: u16 = 65_535;
/// default routers as a eth-provider fallback
const DEFAULT_ETH_PROVIDERS: &str = include_str!("eth/default_providers_mainnet.json");
#[cfg(not(feature = "simulation-mode"))]
pub const CHAIN_ID: u64 = 10;
#[cfg(feature = "simulation-mode")]
pub const CHAIN_ID: u64 = 31337;
#[cfg(not(feature = "simulation-mode"))]
pub const KNS_ADDRESS: &str = "0xca5b5811c0c40aab3295f932b1b5112eb7bb4bd6";
#[cfg(feature = "simulation-mode")]
pub const KNS_ADDRESS: &str = "0x5FbDB2315678afecb367f032d93F642f64180aa3";

#[tokio::main]
async fn main() {
    let app = build_command();

    let matches = app.get_matches();
    let home_directory_path = matches
        .get_one::<String>("home")
        .expect("home directory required");
    create_home_directory(&home_directory_path).await;
    let http_server_port = set_http_server_port(matches.get_one::<u16>("port")).await;
    let ws_networking_port = matches.get_one::<u16>("ws-port");
    #[cfg(not(feature = "simulation-mode"))]
    let tcp_networking_port = matches.get_one::<u16>("tcp-port");
    let verbose_mode = *matches
        .get_one::<u8>("verbosity")
        .expect("verbosity required");
    let rpc = matches.get_one::<String>("rpc");
    let password = matches.get_one::<String>("password");

    // if we are in sim-mode, detached determines whether terminal is interactive
    #[cfg(not(feature = "simulation-mode"))]
    let is_detached = false;

    #[cfg(feature = "simulation-mode")]
    let (fake_node_name, is_detached, fakechain_port) = (
        matches.get_one::<String>("fake-node-name"),
        *matches.get_one::<bool>("detached").unwrap(),
        matches.get_one::<u16>("fakechain-port").cloned(),
    );

    // default eth providers/routers
    let mut eth_provider_config: lib::eth::SavedConfigs = if let Ok(contents) =
        tokio::fs::read_to_string(format!("{}/.eth_providers", home_directory_path)).await
    {
        if let Ok(contents) = serde_json::from_str(&contents) {
            println!("loaded saved eth providers\r");
            contents
        } else {
            println!("error loading saved eth providers, using default providers\r");
            serde_json::from_str(DEFAULT_ETH_PROVIDERS).unwrap()
        }
    } else {
        serde_json::from_str(DEFAULT_ETH_PROVIDERS).unwrap()
    };
    if let Some(rpc) = rpc {
        eth_provider_config.insert(lib::eth::ProviderConfig {
            chain_id: CHAIN_ID,
            trusted: true,
            provider: lib::eth::NodeOrRpcUrl::RpcUrl(rpc.to_string()),
        });
        // save the new provider config
        tokio::fs::write(
            format!("{}/.eth_providers", home_directory_path),
            serde_json::to_string(&eth_provider_config).unwrap(),
        )
        .await
        .expect("failed to save new eth provider config!");
    }

    #[cfg(feature = "simulation-mode")]
    {
        let local_chain_port = matches
            .get_one::<u16>("fakechain-port")
            .cloned()
            .unwrap_or(8545);
        eth_provider_config.insert(lib::eth::ProviderConfig {
            chain_id: 31337,
            trusted: true,
            provider: lib::eth::NodeOrRpcUrl::RpcUrl(format!(
                "ws://localhost:{}",
                local_chain_port
            )),
        });
    }

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
    // kernel_state sender and receiver
    let (state_sender, state_receiver): (MessageSender, MessageReceiver) =
        mpsc::channel(VFS_CHANNEL_CAPACITY);
    // kv sender and receiver
    let (kv_sender, kv_receiver): (MessageSender, MessageReceiver) =
        mpsc::channel(KV_CHANNEL_CAPACITY);
    // sqlite sender and receiver
    let (sqlite_sender, sqlite_receiver): (MessageSender, MessageReceiver) =
        mpsc::channel(SQLITE_CHANNEL_CAPACITY);
    // http server channel w/ websockets (eyre)
    let (http_server_sender, http_server_receiver): (MessageSender, MessageReceiver) =
        mpsc::channel(HTTP_CHANNEL_CAPACITY);
    let (timer_service_sender, timer_service_receiver): (MessageSender, MessageReceiver) =
        mpsc::channel(HTTP_CHANNEL_CAPACITY);
    let (eth_provider_sender, eth_provider_receiver): (MessageSender, MessageReceiver) =
        mpsc::channel(ETH_PROVIDER_CHANNEL_CAPACITY);
    let (eth_net_error_sender, eth_net_error_receiver): (NetworkErrorSender, NetworkErrorReceiver) =
        mpsc::channel(EVENT_LOOP_CHANNEL_CAPACITY);
    // http client performs http requests on behalf of processes
    let (http_client_sender, http_client_receiver): (MessageSender, MessageReceiver) =
        mpsc::channel(HTTP_CLIENT_CHANNEL_CAPACITY);
    // vfs maintains metadata about files in fs for processes
    let (vfs_message_sender, vfs_message_receiver): (MessageSender, MessageReceiver) =
        mpsc::channel(VFS_CHANNEL_CAPACITY);
    // terminal receives prints via this channel, all other modules send prints
    let (print_sender, print_receiver): (PrintSender, PrintReceiver) =
        mpsc::channel(TERMINAL_CHANNEL_CAPACITY);

    let our_ip = find_public_ip().await;
    let (ws_tcp_handle, ws_flag_used) = setup_networking("ws", ws_networking_port).await;
    #[cfg(not(feature = "simulation-mode"))]
    let (tcp_tcp_handle, tcp_flag_used) = setup_networking("tcp", tcp_networking_port).await;

    #[cfg(feature = "simulation-mode")]
    let (our, encoded_keyfile, decoded_keyfile) = simulate_node(
        fake_node_name.cloned(),
        password.cloned(),
        home_directory_path,
        (
            ws_tcp_handle.expect("need ws networking for simulation mode"),
            ws_flag_used,
        ),
        // NOTE: fakenodes only using WS protocol at the moment
        fakechain_port,
    )
    .await;

    #[cfg(not(feature = "simulation-mode"))]
    println!(
        "login or register at http://localhost:{}\r",
        http_server_port
    );
    #[cfg(not(feature = "simulation-mode"))]
    let (our, encoded_keyfile, decoded_keyfile) = match password {
        None => {
            serve_register_fe(
                &home_directory_path,
                our_ip.to_string(),
                (ws_tcp_handle, ws_flag_used),
                (tcp_tcp_handle, tcp_flag_used),
                http_server_port,
                rpc.cloned(),
            )
            .await
        }
        Some(password) => {
            login_with_password(
                &home_directory_path,
                our_ip.to_string(),
                (ws_tcp_handle, ws_flag_used),
                (tcp_tcp_handle, tcp_flag_used),
                rpc.cloned(),
                password,
            )
            .await
        }
    };

    // the boolean flag determines whether the runtime module is *public* or not,
    // where public means that any process can always message it.
    #[allow(unused_mut)]
    let mut runtime_extensions = vec![
        (
            ProcessId::new(Some("http_server"), "distro", "sys"),
            http_server_sender,
            None,
            false,
        ),
        (
            ProcessId::new(Some("http_client"), "distro", "sys"),
            http_client_sender,
            None,
            false,
        ),
        (
            ProcessId::new(Some("timer"), "distro", "sys"),
            timer_service_sender,
            None,
            true,
        ),
        (
            ProcessId::new(Some("eth"), "distro", "sys"),
            eth_provider_sender,
            Some(eth_net_error_sender),
            false,
        ),
        (
            ProcessId::new(Some("vfs"), "distro", "sys"),
            vfs_message_sender,
            None,
            false,
        ),
        (
            ProcessId::new(Some("state"), "distro", "sys"),
            state_sender,
            None,
            false,
        ),
        (
            ProcessId::new(Some("kv"), "distro", "sys"),
            kv_sender,
            None,
            false,
        ),
        (
            ProcessId::new(Some("sqlite"), "distro", "sys"),
            sqlite_sender,
            None,
            false,
        ),
    ];

    /*
     *  the kernel module will handle our userspace processes and receives
     *  the basic message format for this OS.
     *
     *  if any of these modules fail, the program exits with an error.
     */
    let networking_keypair_arc = Arc::new(decoded_keyfile.networking_keypair);

    let (kernel_process_map, db, reverse_cap_index) = state::load_state(
        our.name.clone(),
        networking_keypair_arc.clone(),
        home_directory_path.clone(),
        runtime_extensions.clone(),
    )
    .await
    .expect("state load failed!");

    let mut tasks = tokio::task::JoinSet::<Result<()>>::new();
    tasks.spawn(kernel::kernel(
        our.clone(),
        networking_keypair_arc.clone(),
        kernel_process_map.clone(),
        reverse_cap_index,
        caps_oracle_sender.clone(),
        caps_oracle_receiver,
        kernel_message_sender.clone(),
        print_sender.clone(),
        kernel_message_receiver,
        network_error_receiver,
        kernel_debug_message_receiver,
        net_message_sender,
        home_directory_path.clone(),
        runtime_extensions,
        // from saved eth provider config, filter for node identities which will be
        // bootstrapped into the networking module, so that this node can start
        // getting PKI info ("bootstrap")
        eth_provider_config
            .clone()
            .into_iter()
            .filter_map(|config| {
                if let lib::eth::NodeOrRpcUrl::Node { kns_update, .. } = config.provider {
                    Some(kns_update)
                } else {
                    None
                }
            })
            .collect(),
    ));
    tasks.spawn(net::networking(
        our.clone(),
        our_ip.to_string(),
        networking_keypair_arc.clone(),
        kernel_message_sender.clone(),
        network_error_sender,
        print_sender.clone(),
        net_message_receiver,
        *matches.get_one::<bool>("reveal-ip").unwrap_or(&true),
    ));
    tasks.spawn(state::state_sender(
        our.name.clone(),
        kernel_message_sender.clone(),
        print_sender.clone(),
        state_receiver,
        db,
        home_directory_path.clone(),
    ));
    tasks.spawn(kv::kv(
        our.name.clone(),
        kernel_message_sender.clone(),
        print_sender.clone(),
        kv_receiver,
        caps_oracle_sender.clone(),
        home_directory_path.clone(),
    ));
    tasks.spawn(sqlite::sqlite(
        our.name.clone(),
        kernel_message_sender.clone(),
        print_sender.clone(),
        sqlite_receiver,
        caps_oracle_sender.clone(),
        home_directory_path.clone(),
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
    tasks.spawn(eth::provider(
        our.name.clone(),
        home_directory_path.clone(),
        eth_provider_config,
        kernel_message_sender.clone(),
        eth_provider_receiver,
        eth_net_error_receiver,
        caps_oracle_sender.clone(),
        print_sender.clone(),
    ));
    tasks.spawn(vfs::vfs(
        our.name.clone(),
        kernel_message_sender.clone(),
        print_sender.clone(),
        vfs_message_receiver,
        caps_oracle_sender.clone(),
        home_directory_path.clone(),
    ));

    // if a runtime task exits, try to recover it,
    // unless it was terminal signaling a quit
    // or a SIG* was intercepted
    let quit_msg: String = tokio::select! {
        Some(Ok(res)) = tasks.join_next() => {
            match res {
                Ok(_) => "graceful exit".into(),
                Err(e) => format!(
                    "runtime crash: {e:?}"
                ),
            }

        }
        quit = terminal::terminal(
            our.clone(),
            VERSION,
            home_directory_path.into(),
            kernel_message_sender.clone(),
            kernel_debug_message_sender,
            print_sender.clone(),
            print_receiver,
            is_detached,
            verbose_mode,
        ) => {
            match quit {
                Ok(_) => match kernel_message_sender
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
                            body: serde_json::to_vec(&KernelCommand::Shutdown).unwrap(),
                            metadata: None,
                            capabilities: vec![],
                        }),
                        lazy_load_blob: None,
                    })
                    .await
                {
                    Ok(()) => "graceful exit".into(),
                    Err(_) => {
                        "failed to gracefully shut down kernel".into()
                    }
                },
                Err(e) => e.to_string(),
            }
        }
    };

    // abort all remaining tasks
    tasks.shutdown().await;
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    crossterm::execute!(
        stdout,
        crossterm::event::DisableBracketedPaste,
        crossterm::terminal::SetTitle(""),
        crossterm::style::SetForegroundColor(crossterm::style::Color::Red),
        crossterm::style::Print(format!("\r\n{quit_msg}\r\n")),
        crossterm::style::ResetColor,
    )
    .expect("failed to clean up terminal visual state! your terminal window might be funky now");
}

async fn set_http_server_port(set_port: Option<&u16>) -> u16 {
    if let Some(port) = set_port {
        match http::utils::find_open_port(*port, port + 1).await {
            Some(bound) => bound.local_addr().unwrap().port(),
            None => {
                println!(
                    "error: couldn't bind {}; first available port found was {}. \
                        Set an available port with `--port` and try again.",
                    port,
                    http::utils::find_open_port(*port, port + 1000)
                        .await
                        .expect("no ports found in range")
                        .local_addr()
                        .unwrap()
                        .port(),
                );
                panic!();
            }
        }
    } else {
        match http::utils::find_open_port(8080, 8999).await {
            Some(bound) => bound.local_addr().unwrap().port(),
            None => {
                println!(
                    "error: couldn't bind any ports between 8080 and 8999. \
                        Set an available port with `--port` and try again."
                );
                panic!();
            }
        }
    }
}

/// Sets up networking by finding an open port and creating a TCP listener.
/// If a specific port is provided, it attempts to bind to it directly.
/// If no port is provided, it searches for the first available port between 9000 and 65535.
/// Returns a tuple containing the TcpListener and a boolean indicating if a specific port was used.
async fn setup_networking(
    protocol: &str,
    networking_port: Option<&u16>,
) -> (Option<tokio::net::TcpListener>, bool) {
    if let Some(&0) = networking_port {
        return (None, true);
    }
    match networking_port {
        Some(port) => {
            let listener = http::utils::find_open_port(*port, port + 1)
                .await
                .expect("port selected with flag could not be bound");
            (Some(listener), true)
        }
        None => {
            let min_port = if protocol == "ws" {
                WS_MIN_PORT
            } else {
                TCP_MIN_PORT
            };
            let listener = http::utils::find_open_port(min_port, MAX_PORT)
                .await
                .expect("no ports found in range 9000-65535 for kinode networking");
            (Some(listener), false)
        }
    }
}

/// On simulation mode, we either boot from existing keys, or generate and post keys to chain.
#[cfg(feature = "simulation-mode")]
pub async fn simulate_node(
    fake_node_name: Option<String>,
    password: Option<String>,
    home_directory_path: &str,
    (ws_networking, _ws_used): (tokio::net::TcpListener, bool),
    fakechain_port: Option<u16>,
) -> (Identity, Vec<u8>, Keyfile) {
    match fake_node_name {
        None => {
            match password {
                None => {
                    panic!("Fake node must be booted with either a --fake-node-name, --password, or both.");
                }
                Some(password) => {
                    let keyfile = tokio::fs::read(format!("{}/.keys", home_directory_path))
                        .await
                        .expect("could not read keyfile");
                    let decoded = keygen::decode_keyfile(&keyfile, &password)
                        .expect("could not decode keyfile");
                    let mut identity = Identity {
                        name: decoded.username.clone(),
                        networking_key: format!(
                            "0x{}",
                            hex::encode(decoded.networking_keypair.public_key().as_ref())
                        ),
                        routing: NodeRouting::Routers(decoded.routers.clone()),
                    };

                    fakenet::assign_ws_local_helper(
                        &mut identity,
                        ws_networking.local_addr().unwrap().port(),
                        fakechain_port.unwrap_or(8545),
                    )
                    .await
                    .unwrap();

                    (identity, keyfile, decoded)
                }
            }
        }
        Some(name) => {
            let password_hash = password.unwrap_or_else(|| "secret".to_string());
            let (pubkey, networking_keypair) = keygen::generate_networking_key();
            let seed = SystemRandom::new();
            let mut jwt_secret = [0u8; 32];
            ring::rand::SecureRandom::fill(&seed, &mut jwt_secret).unwrap();

            let fakechain_port: u16 = fakechain_port.unwrap_or(8545);
            let ws_port = ws_networking.local_addr().unwrap().port();

            fakenet::register_local(&name, ws_port, &pubkey, fakechain_port)
                .await
                .unwrap();

            let identity = Identity {
                name: name.clone(),
                networking_key: pubkey,
                routing: NodeRouting::Direct {
                    ip: "127.0.0.1".into(),
                    ports: std::collections::BTreeMap::from([("ws".to_string(), ws_port)]),
                },
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
                password_hash,
                name.clone(),
                decoded_keyfile.routers.clone(),
                networking_keypair.as_ref(),
                &decoded_keyfile.jwt_secret_bytes,
                &decoded_keyfile.file_key,
            );

            tokio::fs::write(
                format!("{}/.keys", home_directory_path),
                encoded_keyfile.clone(),
            )
            .await
            .expect("Failed to write keyfile");

            (identity, encoded_keyfile, decoded_keyfile)
        }
    }
}

async fn create_home_directory(home_directory_path: &str) {
    if let Err(e) = tokio::fs::create_dir_all(home_directory_path).await {
        panic!("failed to create home directory: {:?}", e);
    }
    println!("home at {}\r", home_directory_path);
}

/// build the command line interface for kinode
///
fn build_command() -> Command {
    let app = Command::new("kinode")
        .version(VERSION)
        .author("Kinode DAO: https://github.com/kinode-dao")
        .about("A General Purpose Sovereign Cloud Computing Platform")
        .arg(arg!([home] "Path to home directory").required(true))
        .arg(
            arg!(--port <PORT> "Port to bind [default: first unbound at or above 8080]")
                .value_parser(value_parser!(u16)),
        )
        .arg(
            arg!(--"ws-port" <PORT> "Kinode internal WebSockets protocol port [default: first unbound at or above 9000]")
                .alias("--ws-port")
                .value_parser(value_parser!(u16)),
        )
        .arg(
            arg!(--"tcp-port" <PORT> "Kinode internal TCP protocol port [default: first unbound at or above 10000]")
                .alias("--tcp-port")
                .value_parser(value_parser!(u16)),
        )
        .arg(
            arg!(--verbosity <VERBOSITY> "Verbosity level: higher is more verbose")
                .default_value("0")
                .value_parser(value_parser!(u8)),
        )
        .arg(
            arg!(--"reveal-ip" "If set to false, as an indirect node, always use routers to connect to other nodes.")
                .default_value("true")
                .value_parser(value_parser!(bool)),
        )
        .arg(arg!(--rpc <RPC> "Add a WebSockets RPC URL at boot"))
        .arg(arg!(--password <PASSWORD> "Node password (in double quotes)"));

    #[cfg(feature = "simulation-mode")]
    let app = app
        .arg(arg!(--"fake-node-name" <NAME> "Name of fake node to boot"))
        .arg(
            arg!(--"fakechain-port" <FAKECHAIN_PORT> "Port to bind to for fakechain")
                .value_parser(value_parser!(u16)),
        )
        .arg(
            arg!(--detached <IS_DETACHED> "Run in detached mode (don't accept input)")
                .action(clap::ArgAction::SetTrue),
        );
    app
}

/// Attempts to find the public IPv4 address of the node.
/// If in simulation mode, it immediately returns localhost.
/// Otherwise, it tries to find the public IP and defaults to localhost on failure.
async fn find_public_ip() -> std::net::Ipv4Addr {
    #[cfg(feature = "simulation-mode")]
    {
        std::net::Ipv4Addr::LOCALHOST
    }

    #[cfg(not(feature = "simulation-mode"))]
    {
        println!("Finding public IP address...");
        match tokio::time::timeout(std::time::Duration::from_secs(5), public_ip::addr_v4()).await {
            Ok(Some(ip)) => {
                println!("Public IP found: {}", ip);
                ip
            }
            _ => {
                println!("Failed to find public IPv4 address: booting as a routed node.");
                std::net::Ipv4Addr::LOCALHOST
            }
        }
    }
}

/// check if we have keys saved on disk, encrypted
/// if so, prompt user for "password" to decrypt with
///
/// once password is received, use to decrypt local keys file,
/// and pass the keys into boot process as is done in registration.
///
/// NOTE: when we log in, we MUST check the PKI to make sure our
/// information matches what we think it should be. this includes
/// username, networking key, and routing info.
/// if any do not match, we should prompt user to create a "transaction"
/// that updates their PKI info on-chain.
#[cfg(not(feature = "simulation-mode"))]
async fn serve_register_fe(
    home_directory_path: &str,
    our_ip: String,
    ws_networking: (Option<tokio::net::TcpListener>, bool),
    tcp_networking: (Option<tokio::net::TcpListener>, bool),
    http_server_port: u16,
    maybe_rpc: Option<String>,
) -> (Identity, Vec<u8>, Keyfile) {
    let (kill_tx, kill_rx) = tokio::sync::oneshot::channel::<bool>();

    let disk_keyfile: Option<Vec<u8>> = tokio::fs::read(format!("{}/.keys", home_directory_path))
        .await
        .ok();

    let (tx, mut rx) = mpsc::channel::<(Identity, Keyfile, Vec<u8>)>(1);
    let (our, decoded_keyfile, encoded_keyfile) = tokio::select! {
        _ = register::register(
                tx,
                kill_rx,
                our_ip,
                (ws_networking.0.as_ref(), ws_networking.1),
                (tcp_networking.0.as_ref(), tcp_networking.1),
                http_server_port,
                disk_keyfile,
                maybe_rpc) => {
            panic!("registration failed")
        }
        Some((our, decoded_keyfile, encoded_keyfile)) = rx.recv() => {
            (our, decoded_keyfile, encoded_keyfile)
        }
    };

    tokio::fs::write(
        format!("{}/.keys", home_directory_path),
        encoded_keyfile.clone(),
    )
    .await
    .unwrap();

    let _ = kill_tx.send(true);

    drop(ws_networking.0);
    drop(tcp_networking.0);

    (our, encoded_keyfile, decoded_keyfile)
}

#[cfg(not(feature = "simulation-mode"))]
async fn login_with_password(
    home_directory_path: &str,
    our_ip: String,
    ws_networking: (Option<tokio::net::TcpListener>, bool),
    tcp_networking: (Option<tokio::net::TcpListener>, bool),
    maybe_rpc: Option<String>,
    password: &str,
) -> (Identity, Vec<u8>, Keyfile) {
    use {
        alloy_primitives::Address as EthAddress,
        ring::signature::KeyPair,
        sha2::{Digest, Sha256},
    };

    let disk_keyfile: Vec<u8> = tokio::fs::read(format!("{}/.keys", home_directory_path))
        .await
        .expect("could not read keyfile");

    let password_hash = format!("0x{}", hex::encode(Sha256::digest(password)));

    // KnsRegistrar contract address
    let kns_address: EthAddress = KNS_ADDRESS.parse().unwrap();

    let provider = Arc::new(register::connect_to_provider(maybe_rpc).await);

    let k = keygen::decode_keyfile(&disk_keyfile, &password_hash)
        .expect("could not decode keyfile, password incorrect");

    let mut our = Identity {
        name: k.username.clone(),
        networking_key: format!(
            "0x{}",
            hex::encode(k.networking_keypair.public_key().as_ref())
        ),
        routing: if k.routers.is_empty() {
            NodeRouting::Direct {
                ip: our_ip,
                ports: std::collections::BTreeMap::new(),
            }
        } else {
            NodeRouting::Routers(k.routers.clone())
        },
    };

    register::assign_routing(
        &mut our,
        kns_address,
        provider,
        match ws_networking.0 {
            Some(listener) => (listener.local_addr().unwrap().port(), ws_networking.1),
            None => (0, ws_networking.1),
        },
        match tcp_networking.0 {
            Some(listener) => (listener.local_addr().unwrap().port(), tcp_networking.1),
            None => (0, tcp_networking.1),
        },
    )
    .await
    .expect("information used to boot does not match information onchain");

    tokio::fs::write(
        format!("{}/.keys", home_directory_path),
        disk_keyfile.clone(),
    )
    .await
    .unwrap();

    (our, disk_keyfile, k)
}
