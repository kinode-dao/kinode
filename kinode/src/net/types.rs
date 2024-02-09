use dashmap::DashMap;
use futures::stream::{SplitSink, SplitStream};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::mpsc::UnboundedSender;
use tokio_tungstenite::{tungstenite, MaybeTlsStream, WebSocketStream};

use lib::types::core::*;

/// Sent to a node when you want to connect directly to them.
/// Sent in the 'e, ee, s, es' and 's, se' phases of XX noise protocol pattern.
#[derive(Debug, Deserialize, Serialize)]
pub struct HandshakePayload {
    pub protocol_version: u8,
    pub name: NodeId,
    // signature is created by their networking key, of their static key
    // someone could reuse this signature, but then they will be unable
    // to encrypt messages to us.
    pub signature: Vec<u8>,
    /// Set to true when you want them to act as a router for you, sending
    /// messages from potentially many remote sources over this connection,
    /// including from the router itself.
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
#[derive(Debug, Deserialize, Serialize)]
pub struct RoutingRequest {
    pub protocol_version: u8,
    pub source: NodeId,
    // signature is created by their networking key, of the [target, router name].concat()
    // someone could reuse this signature, and TODO need to make sure that's useless.
    pub signature: Vec<u8>,
    pub target: NodeId,
}

pub enum Connection {
    Peer(PeerConnection),
    Passthrough(PassthroughConnection),
    PendingPassthrough(PendingPassthroughConnection),
}

pub struct PeerConnection {
    pub noise: snow::TransportState,
    pub buf: Vec<u8>,
    pub write_stream: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::Message>,
    pub read_stream: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
}

pub struct PassthroughConnection {
    pub write_stream_1: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::Message>,
    pub read_stream_1: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    pub write_stream_2: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::Message>,
    pub read_stream_2: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
}

pub struct PendingPassthroughConnection {
    pub target: NodeId,
    pub write_stream: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::Message>,
    pub read_stream: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
}

pub type Peers = Arc<DashMap<String, Peer>>;
pub type PKINames = Arc<DashMap<String, NodeId>>;
pub type OnchainPKI = Arc<DashMap<String, Identity>>;
pub type PendingPassthroughs = HashMap<(NodeId, NodeId), PendingPassthroughConnection>;

#[derive(Clone)]
pub struct Peer {
    pub identity: Identity,
    /// If true, we are routing for them and have a RoutingClientConnection
    /// associated with them. We can send them prompts to establish Passthroughs.
    pub routing_for: bool,
    pub sender: UnboundedSender<KernelMessage>,
}

/// Must be parsed from message pack vector.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NetActions {
    /// Received from a router of ours when they have a new pending passthrough for us.
    /// We should respond (if we desire) by using them to initialize a routed connection
    /// with the NodeId given.
    ConnectionRequest(NodeId),
    /// can only receive from trusted source, for now just ourselves locally,
    /// in the future could get from remote provider
    KnsUpdate(KnsUpdate),
    KnsBatchUpdate(Vec<KnsUpdate>),
}

/// For now, only sent in response to a ConnectionRequest.
/// Must be parsed from message pack vector
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NetResponses {
    Accepted(NodeId),
    Rejected(NodeId),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KnsUpdate {
    pub name: String, // actual username / domain name
    pub owner: String,
    pub node: String, // hex namehash of node
    pub public_key: String,
    pub ip: String,
    pub port: u16,
    pub routers: Vec<String>,
}
