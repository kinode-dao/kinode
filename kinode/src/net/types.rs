use crate::net::utils;
use lib::types::core::{
    Identity, KernelMessage, MessageSender, NetworkErrorSender, NodeId, PrintSender,
};
use {
    dashmap::DashMap,
    ring::signature::Ed25519KeyPair,
    serde::{Deserialize, Serialize},
    std::sync::Arc,
    tokio::net::TcpStream,
    tokio::sync::mpsc::UnboundedSender,
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
pub struct Peers(pub Arc<DashMap<String, Peer>>);

impl Peers {
    pub fn get(&self, name: &str) -> Option<dashmap::mapref::one::Ref<'_, String, Peer>> {
        self.0.get(name)
    }

    pub fn get_mut(
        &self,
        name: &str,
    ) -> std::option::Option<dashmap::mapref::one::RefMut<'_, String, Peer>> {
        self.0.get_mut(name)
    }

    pub fn contains_key(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    /// when a peer is inserted, if the total number of peers exceeds the limit,
    /// remove the one with the oldest last_message.
    pub fn insert(&self, name: String, peer: Peer) {
        self.0.insert(name, peer);
        if self.0.len() > utils::MAX_PEERS {
            let oldest = self.0.iter().min_by_key(|p| p.last_message).unwrap();
            self.0.remove(oldest.key());
        }
    }

    pub fn remove(&self, name: &str) -> Option<(String, Peer)> {
        self.0.remove(name)
    }
}

pub type OnchainPKI = Arc<DashMap<String, Identity>>;

/// (from, target) -> from's socket
pub type PendingPassthroughs = Arc<DashMap<(NodeId, NodeId), PendingStream>>;
pub enum PendingStream {
    WebSocket(WebSocketStream<MaybeTlsStream<TcpStream>>),
    Tcp(TcpStream),
}

impl PendingStream {
    pub fn is_ws(&self) -> bool {
        matches!(self, PendingStream::WebSocket(_))
    }
    pub fn is_tcp(&self) -> bool {
        matches!(self, PendingStream::Tcp(_))
    }
}

pub struct Peer {
    pub identity: Identity,
    /// If true, we are routing for them and have a RoutingClientConnection
    /// associated with them. We can send them prompts to establish Passthroughs.
    pub routing_for: bool,
    pub sender: UnboundedSender<KernelMessage>,
    /// unix timestamp of last message sent *or* received
    pub last_message: u64,
}

impl Peer {
    pub fn set_last_message(&mut self) {
        self.last_message = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
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
    pub pending_passthroughs: PendingPassthroughs,
}
