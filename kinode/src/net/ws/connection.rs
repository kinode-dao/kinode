use futures::stream::{SplitSink, SplitStream};
use lib::types::core::*;
use std::collections::HashMap;
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite, MaybeTlsStream, WebSocketStream};

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

pub type PendingPassthroughs = HashMap<(NodeId, NodeId), PendingPassthroughConnection>;

pub struct PendingPassthroughConnection {
    pub target: NodeId,
    pub write_stream: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::Message>,
    pub read_stream: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
}
