use lib::types::core::{Identity, MessageReceiver, MessageSender, NetworkErrorSender, PrintSender};
use {anyhow::Result, ring::signature::Ed25519KeyPair, std::sync::Arc};

pub mod types;
mod utils;
mod ws;

pub async fn networking(
    our: Identity,
    our_ip: String,
    keypair: Arc<Ed25519KeyPair>,
    kernel_message_tx: MessageSender,
    network_error_tx: NetworkErrorSender,
    print_tx: PrintSender,
    self_message_tx: MessageSender,
    message_rx: MessageReceiver,
    reveal_ip: bool,
) -> Result<()> {
    // TODO add additional networking modalities here
    ws::networking(
        our,
        our_ip,
        keypair,
        kernel_message_tx,
        network_error_tx,
        print_tx,
        self_message_tx,
        message_rx,
        reveal_ip,
    )
    .await
}
