use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::{
    connect_async,
    tungstenite::protocol::Message::{Binary, Text},
};

use crate::types;

type Sender = mpsc::Sender<types::KernelMessage>;
type Receiver = mpsc::Receiver<types::KernelMessage>;

pub async fn mock_client(
    port: u16,
    node_identity: types::NodeId,
    send_to_loop: Sender,
    mut recv_from_loop: Receiver,
    print_tx: types::PrintSender,
    _network_error_sender: types::NetworkErrorSender,
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
                    let km: types::KernelMessage = rmp_serde::from_slice(bin)?;
                    if km.target.process == "net:distro:sys" {
                        if let types::Message::Request(types::Request { ref body, .. }) = km.message {
                            print_tx
                                .send(types::Printout {
                                    verbosity: 0,
                                    content: format!(
                                        "\x1b[3;32m{}: {}\x1b[0m",
                                        km.source.node,
                                        std::str::from_utf8(body).unwrap_or("!!message parse error!!")
                                    ),
                                })
                                .await?;
                            send_to_loop
                                .send(types::KernelMessage {
                                    id: km.id,
                                    source: types::Address {
                                        node: node_identity.clone(),
                                        process: types::ProcessId::new(Some("net"), "distro", "sys"),
                                    },
                                    target: km.rsvp.as_ref().unwrap_or(&km.source).clone(),
                                    rsvp: None,
                                    message: types::Message::Response((
                                        types::Response {
                                            inherit: false,
                                            body: "delivered".as_bytes().to_vec(),
                                            metadata: None,
                                            capabilities: vec![],
                                        },
                                        None,
                                    )),
                                    lazy_load_blob: None,
                                })
                                .await?;
                        }
                    } else {
                        send_to_loop.send(km).await?;
                    }
                }
            },
        }
    }
}
