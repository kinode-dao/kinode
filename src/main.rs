use crate::types::*;
use anyhow::Result;
use clap::{arg, Command};
use std::env;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio::{fs, time::timeout};

mod encryptor;
mod eth_rpc;
mod filesystem;
mod http_client;
mod http_server;
mod kernel;
mod keygen;
mod net;
mod register;
mod terminal;
mod types;
mod vfs;

// extensions
#[cfg(feature = "llm")]
mod llm;

const EVENT_LOOP_CHANNEL_CAPACITY: usize = 10_000;
const EVENT_LOOP_DEBUG_CHANNEL_CAPACITY: usize = 50;
const TERMINAL_CHANNEL_CAPACITY: usize = 32;
const WEBSOCKET_SENDER_CHANNEL_CAPACITY: usize = 32;
const FILESYSTEM_CHANNEL_CAPACITY: usize = 32;
const HTTP_CHANNEL_CAPACITY: usize = 32;
const HTTP_CLIENT_CHANNEL_CAPACITY: usize = 32;
const ETH_RPC_CHANNEL_CAPACITY: usize = 32;
const VFS_CHANNEL_CAPACITY: usize = 1_000;
const ENCRYPTOR_CHANNEL_CAPACITY: usize = 32;
const CAP_CHANNEL_CAPACITY: usize = 1_000;
const LLM_CHANNEL_CAPACITY: usize = 32;

// const QNS_SEPOLIA_ADDRESS: &str = "0x9e5ed0e7873E0d7f10eEb6dE72E87fE087A12776";

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// This can and should be an environment variable / setting. It configures networking
/// such that indirect nodes always use routers, even when target is a direct node,
/// such that only their routers can ever see their physical networking details.
const REVEAL_IP: bool = true;

#[tokio::main]
async fn main() {
    // For use with https://github.com/tokio-rs/console
    // console_subscriber::init();

    // DEMO ONLY: remove all CLI arguments
    let matches = Command::new("Uqbar")
        .version("0.1.0")
        .author("Uqbar DAO")
        .about("A decentralized operating system")
        .arg(arg!([home] "Path to home directory").required(true))
        .arg(arg!(--rpc <WS_URL> "Ethereum RPC endpoint (must be wss://)").required(true))
        .arg(arg!(--llm <LLM_URL> "LLM endpoint"))
        .get_matches();
    let home_directory_path = matches.get_one::<String>("home").unwrap();
    let rpc_url = matches.get_one::<String>("rpc").unwrap();
    let llm_url = matches.get_one::<String>("llm");

    #[cfg(not(feature = "llm"))]
    if let Some(llm_url) = llm_url {
        panic!("You passed in --llm {:?} but you do not have the llm feature enabled. Please re-run with `--features llm`", llm_url);
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
    // filesystem receives request messages via this channel, kernel sends messages
    let (fs_message_sender, fs_message_receiver): (MessageSender, MessageReceiver) =
        mpsc::channel(FILESYSTEM_CHANNEL_CAPACITY);
    // http server channel w/ websockets (eyre)
    let (http_server_sender, http_server_receiver): (MessageSender, MessageReceiver) =
        mpsc::channel(HTTP_CHANNEL_CAPACITY);
    // http client performs http requests on behalf of processes
    let (eth_rpc_sender, eth_rpc_receiver): (MessageSender, MessageReceiver) =
        mpsc::channel(ETH_RPC_CHANNEL_CAPACITY);
    let (http_client_sender, http_client_receiver): (MessageSender, MessageReceiver) =
        mpsc::channel(HTTP_CLIENT_CHANNEL_CAPACITY);
    // vfs maintains metadata about files in fs for processes
    let (vfs_message_sender, vfs_message_receiver): (MessageSender, MessageReceiver) =
        mpsc::channel(VFS_CHANNEL_CAPACITY);
    // encryptor handles end-to-end encryption for client messages
    let (encryptor_sender, encryptor_receiver): (MessageSender, MessageReceiver) =
        mpsc::channel(ENCRYPTOR_CHANNEL_CAPACITY);
    // optional llm extension
    let (llm_sender, llm_receiver): (MessageSender, MessageReceiver) =
        mpsc::channel(LLM_CHANNEL_CAPACITY);
    // terminal receives prints via this channel, all other modules send prints
    let (print_sender, print_receiver): (PrintSender, PrintReceiver) =
        mpsc::channel(TERMINAL_CHANNEL_CAPACITY);

    //  fs config in .env file (todo add -- arguments cleanly (with clap?))
    dotenv::dotenv().ok();

    let mem_buffer_limit = env::var("MEM_BUFFER_LIMIT")
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

    // check if we have keys saved on disk, encrypted
    // if so, prompt user for "password" to decrypt with

    // once password is received, use to decrypt local keys file,
    // and pass the keys into boot process as is done in registration.

    // NOTE: when we log in, we MUST check the PKI to make sure our
    // information matches what we think it should be. this includes
    // username, networking key, and routing info.
    // if any do not match, we should prompt user to create a "transaction"
    // that updates their PKI info on-chain.
    let http_server_port = http_server::find_open_port(8080).await.unwrap();
    println!("login or register at http://localhost:{}", http_server_port);
    let (kill_tx, kill_rx) = oneshot::channel::<bool>();

    let disk_keyfile = match fs::read(format!("{}/.keys", home_directory_path)).await {
        Ok(keyfile) => keyfile,
        Err(_) => Vec::new(),
    };

    let (tx, mut rx) = mpsc::channel::<(Identity, Keyfile, Vec<u8>)>(1);
    let (our, decoded_keyfile, encoded_keyfile) = tokio::select! {
        _ = register::register(tx, kill_rx, our_ip.to_string(), http_server_port, disk_keyfile)
            => panic!("registration failed"),
        (our, decoded_keyfile, encoded_keyfile) = async {
            rx.recv().await.expect("registration failed")
        } => (our, decoded_keyfile, encoded_keyfile),
    };

    println!(
        "saving encrypted networking keys to {}/.keys",
        home_directory_path
    );

    fs::write(format!("{}/.keys", home_directory_path), encoded_keyfile)
        .await
        .unwrap();

    println!("registration complete!");

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
            ProcessId::new(Some("eth_rpc"), "sys", "uqbar"),
            eth_rpc_sender,
            true,
        ),
        (
            ProcessId::new(Some("vfs"), "sys", "uqbar"),
            vfs_message_sender,
            true,
        ),
        (
            ProcessId::new(Some("encryptor"), "sys", "uqbar"),
            encryptor_sender,
            false,
        ),
    ];

    #[cfg(feature = "llm")]
    {
        if llm_url.is_none() {
            panic!("You did not pass in --llm <LLM_URL> but you have the llm feature enabled. Please re-run with `--llm <LLM_URL>`");
        }
        runtime_extensions.push((
            ProcessId::new(Some("llm"), "sys", "uqbar"), // TODO llm:extensions:uqbar ?
            llm_sender,
            true,
        ));
    }

    let (kernel_process_map, manifest, vfs_messages) = filesystem::load_fs(
        our.name.clone(),
        home_directory_path.clone(),
        decoded_keyfile.file_key,
        fs_config,
        runtime_extensions.clone(),
    )
    .await
    .expect("fs load failed!");

    let _ = kill_tx.send(true);
    let _ = print_sender
        .send(Printout {
            verbosity: 0,
            content: format!("our networking public key: {}", our.networking_key),
        })
        .await;

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
    tasks.spawn(filesystem::fs_sender(
        our.name.clone(),
        manifest,
        kernel_message_sender.clone(),
        print_sender.clone(),
        fs_message_receiver,
        fs_kill_recv,
        fs_kill_confirm_send,
    ));
    tasks.spawn(http_server::http_server(
        our.name.clone(),
        http_server_port,
        decoded_keyfile.jwt_secret_bytes.clone(),
        http_server_receiver,
        kernel_message_sender.clone(),
        print_sender.clone(),
    ));
    tasks.spawn(http_client::http_client(
        our.name.clone(),
        kernel_message_sender.clone(),
        http_client_receiver,
        print_sender.clone(),
    ));
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
    tasks.spawn(encryptor::encryptor(
        our.name.clone(),
        networking_keypair_arc.clone(),
        kernel_message_sender.clone(),
        encryptor_receiver,
        print_sender.clone(),
    ));
    #[cfg(feature = "llm")]
    {
        tasks.spawn(llm::llm(
            our.name.clone(),
            kernel_message_sender.clone(),
            llm_receiver,
            llm_url.unwrap().to_string(),
            print_sender.clone(),
        ));
    }
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
