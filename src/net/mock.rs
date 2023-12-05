use futures::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::{
    connect_async,
    tungstenite::protocol::Message::{Binary, Text},
    WebSocketStream,
};
use url::Url;

use crate::types;

type Sender = mpsc::Sender<types::KernelMessage>;
type Receiver = mpsc::Receiver<types::KernelMessage>;

pub async fn mock_client(
    port: u16,
    node_identity: types::NodeId,
    send_to_loop: Sender,
    mut recv_from_loop: Receiver,
) -> anyhow::Result<()> {
    let url = format!("ws://127.0.0.1:{}", port);

    let (ws_stream, _) = connect_async(url).await?;
    let (mut send_to_ws, mut recv_from_ws) = ws_stream.split();

    // Send node identity
    send_to_ws.send(Text(node_identity.clone())).await?;

    loop {
        tokio::select! {
            Some(kernel_message) = recv_from_loop.recv() => {
                if kernel_message.target.node != node_identity {
                    // Serialize and send the message through WebSocket
                    // println!("{}:mock: outgoing {}\r", node_identity ,kernel_message);
                    let message = Binary(rmp_serde::to_vec(&kernel_message)?);
                    send_to_ws.send(message).await?;
                }
            },
            Some(Ok(message)) = recv_from_ws.next() => {
                // Deserialize and forward the message to the loop
                // println!("{}:mock: incoming {}\r", node_identity, message);
                if let Binary(ref bin) = message {
                    let kernel_message: types::KernelMessage = rmp_serde::from_slice(bin)?;
                    send_to_loop.send(kernel_message).await?;
                }
            },
        }
    }
}
