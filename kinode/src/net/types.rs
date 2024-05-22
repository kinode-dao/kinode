use {
    dashmap::DashMap,
    lib::types::core::{
        Identity, KernelMessage, MessageSender, NetworkErrorSender, NodeId, PrintSender,
    },
    ring::signature::Ed25519KeyPair,
    serde::{Deserialize, Serialize},
    std::sync::Arc,
    tokio::sync::mpsc::UnboundedSender,
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

pub type Peers = Arc<DashMap<String, Peer>>;
pub type PKINames = Arc<DashMap<String, NodeId>>;
pub type OnchainPKI = Arc<DashMap<String, Identity>>;

#[derive(Clone)]
pub struct Peer {
    pub identity: Identity,
    /// If true, we are routing for them and have a RoutingClientConnection
    /// associated with them. We can send them prompts to establish Passthroughs.
    pub routing_for: bool,
    pub sender: UnboundedSender<KernelMessage>,
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
    pub self_message_tx: MessageSender,
    pub reveal_ip: bool,
}

#[derive(Clone)]
pub struct NetData {
    pub pki: OnchainPKI,
    pub peers: Peers,
    pub names: PKINames,
}
