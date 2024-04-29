use alloy_consensus::TxLegacy;
use alloy_network::{Transaction, TxKind};
use alloy_primitives::Address;
use alloy_providers::provider::{Provider, TempProvider};
use alloy_rpc_client::ClientBuilder;
use alloy_rpc_types::request::{TransactionInput, TransactionRequest};
use alloy_signer::{LocalWallet, Signer, SignerSync};
use alloy_sol_types::SolCall;
use alloy_transport_ws::WsConnect;
use std::str::FromStr;

pub mod helpers;

pub use helpers::RegisterHelpers::*;
pub use helpers::*;

pub async fn register_local(
    name: &str,
    port: u16,
    pubkey: &str,
    router_port: u16,
) -> Result<(), anyhow::Error> {
    let wallet = LocalWallet::from_str(
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
    )?;

    let dotdev = Address::from_str("0xDc64a140Aa3E981100a9becA4E685f962f0cF6C9")?;
    let kns = Address::from_str("0x5FbDB2315678afecb367f032d93F642f64180aa3")?;

    let endpoint = format!("ws://localhost:{}", router_port);
    let ws = WsConnect {
        url: endpoint,
        auth: None,
    };

    let client = ClientBuilder::default().ws(ws).await?;
    let provider = Provider::new_with_client(client);

    let fqdn = dns_encode_fqdn(name);
    let namehash = encode_namehash(name);

    let ip: u128 = 0x7F000001; // localhost IP (127.0.0.1)

    let set_ip = setAllIpCall {
        _node: namehash.into(),
        _ip: ip,
        _ws: port,
        _wt: 0,
        _tcp: 0,
        _udp: 0,
    }
    .abi_encode();

    let set_key = setKeyCall {
        _node: namehash.into(),
        _key: pubkey.parse()?,
    }
    .abi_encode();

    // TODO: set this up so that we do not call .register on something twice.
    // let existsCall = _getOwnerCall {
    //     node: namehash.into(),
    // }
    // .abi_encode();

    // let tx = TransactionRequest::default()
    //     .to(Some(dotdev))
    //     .input(TransactionInput::new(existsCall.into()));

    // let exists = provider.call(tx, None).await?;

    let register = registerCall {
        _name: fqdn.into(),
        _to: Address::from_str("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266")?,
        _data: vec![set_ip.into(), set_key.into()],
    }
    .abi_encode();

    let nonce = provider
        .get_transaction_count(wallet.address(), None)
        .await?;
    let mut tx = TxLegacy {
        to: TxKind::Call(dotdev),
        nonce: nonce.to::<u64>(),
        input: register.into(),
        chain_id: Some(31337),
        gas_limit: 3000000,
        gas_price: 100000000000,
        ..Default::default()
    };

    let sig = wallet.sign_transaction_sync(&mut tx)?;
    let signed_tx = tx.into_signed(sig);
    let mut buf = vec![];
    signed_tx.encode_signed(&mut buf);

    let _tx_hash = provider.send_raw_transaction(buf.into()).await?;
    Ok(())
}
