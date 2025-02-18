use lib::types::core::{
    Address, Identity, KernelMessage, MessageSender, NetworkErrorSender, NodeId, PrintSender,
    NET_PROCESS_ID,
};
use {
    dashmap::DashMap,
    ring::signature::Ed25519KeyPair,
    serde::{Deserialize, Serialize},
    std::sync::atomic::AtomicU64,
    std::sync::Arc,
    tokio::net::TcpStream,
    tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender},
    tokio_tungstenite::{MaybeTlsStream, WebSocketStream},
};

pub const WS_PROTOCOL: &str = "ws";
pub const TCP_PROTOCOL: &str = "tcp";

/// Sent to a node when you want to connect directly to them.
/// Sent in the 'e, ee, s, es' and 's, se' phases of XX noise protocol pattern.
///
/// Should always be serialized and deserialized using MessagePack.
#[derive(Debug, Deserialize, Serialize)]
pub struct HandshakePayload {
    pub protocol_version: u8,
    pub name: NodeId,
    // signature is created by their networking key, of their static key
    // someone could reuse this signature, but then they will be unable
    // to encrypt messages to us.
    pub signature: Vec<u8>,
    /// Set to true when you want them to act as a router for you.
    /// This is not relevant in a handshake sent from the receiver side.
    pub proxy_request: bool,
}

/// Sent to a node when you want them to connect you to an indirect node.
/// If the receiver of the request has an open connection to your target,
/// and is willing, they will send a message to the target prompting them
/// to build the other side of the connection, at which point they will
/// hold open a Passthrough for you two.
///
/// Alternatively, if the receiver does not have an open connection but the
/// target is a direct node, they can create a Passthrough for you two if
/// they are willing to proxy for you.
///
/// Sent in the 'e' phase of XX noise protocol pattern.
///
/// Should always be serialized and deserialized using MessagePack.
#[derive(Debug, Deserialize, Serialize)]
pub struct RoutingRequest {
    pub protocol_version: u8,
    pub source: NodeId,
    // signature is created by their networking key, of the [target, router name].concat()
    // someone could reuse this signature, and TODO need to make sure that's useless.
    pub signature: Vec<u8>,
    pub target: NodeId,
}

#[derive(Clone)]
pub struct Peers {
    max_peers: Arc<AtomicU64>,
    send_to_loop: MessageSender,
    peers: Arc<DashMap<String, Peer>>,
}

impl Peers {
    pub fn new(max_peers: u64, send_to_loop: MessageSender) -> Self {
        Self {
            max_peers: Arc::new(max_peers.into()),
            send_to_loop,
            peers: Arc::new(DashMap::new()),
        }
    }

    pub fn peers(&self) -> &DashMap<String, Peer> {
        &self.peers
    }

    pub fn max_peers(&self) -> u64 {
        self.max_peers.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set_max_peers(&self, max_peers: u64) {
        self.max_peers
            .store(max_peers, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn get(&self, name: &str) -> Option<dashmap::mapref::one::Ref<'_, String, Peer>> {
        self.peers.get(name)
    }

    pub fn get_mut(
        &self,
        name: &str,
    ) -> std::option::Option<dashmap::mapref::one::RefMut<'_, String, Peer>> {
        self.peers.get_mut(name)
    }

    pub fn contains_key(&self, name: &str) -> bool {
        self.peers.contains_key(name)
    }

    /// when a peer is inserted, if the total number of peers exceeds the limit,
    /// remove the one with the oldest last_message.
    pub async fn insert(&self, name: String, peer: Peer) {
        self.peers.insert(name, peer);
        if self.peers.len() as u64 > self.max_peers.load(std::sync::atomic::Ordering::Relaxed) {
            let oldest = self
                .peers
                .iter()
                .min_by_key(|p| p.last_message)
                .unwrap()
                .key()
                .clone();
            self.remove(&oldest).await;
            crate::fd_manager::send_fd_manager_hit_fds_limit(
                &Address::new("our", NET_PROCESS_ID.clone()),
                &self.send_to_loop,
            )
            .await;
        }
    }

    pub async fn remove(&self, name: &str) -> Option<(String, Peer)> {
        self.peers.remove(name)
    }

    /// close the n oldest connections
    pub async fn cull(&self, n: usize) {
        let mut to_remove = Vec::with_capacity(n);
        let mut sorted_peers: Vec<_> = self.peers.iter().collect();
        sorted_peers.sort_by_key(|p| p.last_message);
        to_remove.extend(sorted_peers.iter().take(n));
        for peer in to_remove {
            self.remove(&peer.identity.name).await;
        }
        crate::fd_manager::send_fd_manager_hit_fds_limit(
            &Address::new("our", NET_PROCESS_ID.clone()),
            &self.send_to_loop,
        )
        .await;
    }
}

pub type OnchainPKI = Arc<DashMap<String, Identity>>;

/// (from, target) -> from's socket
///
/// only used by routers
pub type PendingPassthroughs = Arc<DashMap<(NodeId, NodeId), (PendingStream, u64)>>;
pub enum PendingStream {
    WebSocket(WebSocketStream<MaybeTlsStream<TcpStream>>),
    Tcp(TcpStream),
}

/// (from, target)
///
/// only used by routers
pub type ActivePassthroughs = Arc<DashMap<(NodeId, NodeId), (u64, KillSender)>>;

impl PendingStream {
    pub fn is_ws(&self) -> bool {
        matches!(self, PendingStream::WebSocket(_))
    }
    pub fn is_tcp(&self) -> bool {
        matches!(self, PendingStream::Tcp(_))
    }
}

type KillSender = tokio::sync::mpsc::Sender<()>;

pub struct Peer {
    pub identity: Identity,
    /// If true, we are routing for them and have a RoutingClientConnection
    /// associated with them. We can send them prompts to establish Passthroughs.
    pub routing_for: bool,
    pub sender: UnboundedSender<KernelMessage>,
    pub handle: Option<tokio::task::JoinHandle<()>>,
    /// unix timestamp of last message sent *or* received
    pub last_message: u64,
}

impl Peer {
    /// Create a new Peer.
    /// If `routing_for` is true, we are routing for them.
    pub fn new(identity: Identity, routing_for: bool) -> (Self, UnboundedReceiver<KernelMessage>) {
        let (peer_tx, peer_rx) = tokio::sync::mpsc::unbounded_channel();
        (
            Self {
                identity,
                routing_for,
                sender: peer_tx,
                handle: None,
                last_message: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            },
            peer_rx,
        )
    }

    /// Send a message to the peer.
    pub fn send(
        &mut self,
        km: KernelMessage,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<KernelMessage>> {
        self.sender.send(km)?;
        self.set_last_message();
        Ok(())
    }

    /// Update the last message time to now.
    pub fn set_last_message(&mut self) {
        self.last_message = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    pub fn kill(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}
/// [`Identity`], with additional fields for networking.
#[derive(Clone)]
pub struct IdentityExt {
    pub our: Arc<Identity>,
    pub our_ip: Arc<String>,
    pub keypair: Arc<Ed25519KeyPair>,
    pub kernel_message_tx: MessageSender,
    pub network_error_tx: NetworkErrorSender,
    pub print_tx: PrintSender,
    pub _reveal_ip: bool, // TODO use
}

#[derive(Clone)]
pub struct NetData {
    pub pki: OnchainPKI,
    pub peers: Peers,
    /// only used by routers
    pub pending_passthroughs: PendingPassthroughs,
    /// only used by routers
    pub active_passthroughs: ActivePassthroughs,
    pub max_passthroughs: u64,
    pub fds_limit: u64,
}
