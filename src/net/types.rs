use crate::types::*;
use anyhow::Result;
use elliptic_curve::ecdh::SharedSecret;
use ethers::prelude::k256::Secp256k1;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, RwLock};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

pub type PeerKeys = Arc<RwLock<HashMap<String, (Identity, Arc<SharedSecret<Secp256k1>>)>>>;
pub type Peers = Arc<RwLock<HashMap<String, Peer>>>;
pub type WebSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;
pub type MessageResult = Result<NetworkMessage, (u64, SendErrorKind)>;
pub type ErrorShuttle = mpsc::UnboundedSender<MessageResult>;

/// stored in mapping by their username
pub struct Peer {
    pub identity: Identity,
    // send messages here to have them encrypted and sent across an active connection
    pub sender: mpsc::UnboundedSender<(PeerMessage, Option<ErrorShuttle>)>,
    // send encrypted messages from this peer here to have them decrypted and sent to kernel
    pub decrypter: mpsc::UnboundedSender<(Vec<u8>, ErrorShuttle)>,
    pub socket_tx: mpsc::UnboundedSender<(NetworkMessage, Option<ErrorShuttle>)>,
}

/// parsed from Binary websocket message
/// TODO add a version number somewhere in the serialized format!!
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NetworkMessage {
    Ack(u64),
    Nack(u64),
    Msg {
        id: u64,
        from: String,
        to: String,
        contents: Vec<u8>,
    },
    Handshake(Handshake),
    HandshakeAck(Handshake),
}

pub enum PeerMessage {
    Raw(KernelMessage),
    Net(NetworkMessage),
}

/// contains identity and encryption keys, used in initial handshake.
/// parsed from Text websocket message
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Handshake {
    pub id: u64,
    pub from: String,
    pub target: String,
    pub id_signature: Vec<u8>,
    pub ephemeral_public_key: Vec<u8>,
    pub ephemeral_public_key_signature: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NetActions {
    QnsUpdate(QnsUpdate),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QnsUpdate {
    pub name: String, // actual username / domain name
    pub owner: String,
    pub node: String, // hex namehash of node
    pub public_key: String,
    pub ip: String,
    pub port: u16,
    pub routers: Vec<String>,
}
