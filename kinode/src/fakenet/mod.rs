use alloy_consensus::TxLegacy;
use alloy_network::{Transaction, TxKind};
use alloy_primitives::{Address, Bytes, FixedBytes, B256, U256};
use alloy_providers::provider::{Provider, TempProvider};
use alloy_rpc_client::ClientBuilder;
use alloy_rpc_types::request::{TransactionInput, TransactionRequest};
use alloy_signer::{LocalWallet, Signer, SignerSync};
use alloy_sol_types::{SolCall, SolValue};
use alloy_transport_ws::WsConnect;
use lib::core::Identity;
use std::str::FromStr;

pub mod helpers;

use crate::{keygen, KNS_ADDRESS};
pub use helpers::RegisterHelpers::*;
pub use helpers::*;

const FAKE_DOTDEV: &str = "0xDc64a140Aa3E981100a9becA4E685f962f0cF6C9";

/// Attempts to connect to a local anvil fakechain,
/// registering a name with its KNS contract.
/// If name is already registered, resets it.
pub async fn register_local(
    name: &str,
    ws_port: u16,
    pubkey: &str,
    fakechain_port: u16,
) -> Result<(), anyhow::Error> {
    let wallet = LocalWallet::from_str(
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
    )?;

    let dotdev = Address::from_str(FAKE_DOTDEV)?;
    let kns = Address::from_str(KNS_ADDRESS)?;

    let endpoint = format!("ws://localhost:{}", fakechain_port);
    let ws = WsConnect {
        url: endpoint,
        auth: None,
    };

    let client = ClientBuilder::default().ws(ws).await?;
    let provider = Provider::new_with_client(client);

    let fqdn = dns_encode_fqdn(name);
    let namehash = encode_namehash(name);
    // todo: find a better way?
    let namehash_bint: B256 = namehash.into();
    let namehash_uint: U256 = namehash_bint.into();

    let ip: u128 = 0x7F000001; // localhost IP (127.0.0.1)

    let set_ip = setAllIpCall {
        _node: namehash.into(),
        _ip: ip,
        _ws: ws_port,
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

    let exists_call = ownerOfCall {
        node: namehash_uint,
    }
    .abi_encode();

    let exists_tx = TransactionRequest::default()
        .to(Some(dotdev))
        .input(TransactionInput::new(exists_call.into()));

    let exists = provider.call(exists_tx, None).await;

    let (call_input, to) = match exists {
        Err(_e) => {
            // name is not taken, register normally
            let register = registerCall {
                _name: fqdn.into(),
                _to: Address::from_str("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266")?,
                _data: vec![set_ip.into(), set_key.into()],
            }
            .abi_encode();

            (register, dotdev)
        }
        Ok(_owner) => {
            // name is taken, call setAllIp an setKey directly with multicall
            let set_ip = setAllIpCall {
                _node: namehash.into(),
                _ip: ip,
                _ws: ws_port,
                _wt: 0,
                _tcp: 0,
                _udp: 0,
            };
            let set_key = setKeyCall {
                _node: namehash.into(),
                _key: pubkey.parse()?,
            };

            let multicall = multicallCall {
                data: vec![set_ip.abi_encode(), set_key.abi_encode()],
            }
            .abi_encode();

            (multicall, kns)
        }
    };
    let nonce = provider
        .get_transaction_count(wallet.address(), None)
        .await?;

    let mut tx = TxLegacy {
        to: TxKind::Call(to),
        nonce: nonce.to::<u64>(),
        input: call_input.into(),
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

/// Booting from a keyfile, fetches the node's IP data from the KNS contract
/// and assigns it to the Identity struct.
pub async fn assign_ws_local_helper(
    our: &mut Identity,
    ws_port: u16,
    fakechain_port: u16,
) -> Result<(), anyhow::Error> {
    let kns = Address::from_str(KNS_ADDRESS)?;
    let endpoint = format!("ws://localhost:{}", fakechain_port);

    let ws = WsConnect {
        url: endpoint,
        auth: None,
    };

    let client = ClientBuilder::default().ws(ws).await?;
    let provider = Provider::new_with_client(client);

    let namehash = FixedBytes::<32>::from_slice(&keygen::namehash(&our.name));
    let ip_call = ipCall { _0: namehash }.abi_encode();
    let tx_input = TransactionInput::new(Bytes::from(ip_call));
    let tx = TransactionRequest {
        to: Some(kns),
        input: tx_input,
        ..Default::default()
    };

    let Ok(ip_data) = provider.call(tx, None).await else {
        return Err(anyhow::anyhow!("Failed to fetch node IP data from PKI"));
    };

    let Ok((ip, ws, _wt, _tcp, _udp)) = <(u128, u16, u16, u16, u16)>::abi_decode(&ip_data, false)
    else {
        return Err(anyhow::anyhow!("Failed to decode node IP data from PKI"));
    };

    let node_ip = format!(
        "{}.{}.{}.{}",
        (ip >> 24) & 0xFF,
        (ip >> 16) & 0xFF,
        (ip >> 8) & 0xFF,
        ip & 0xFF
    );

    if node_ip != *"0.0.0.0" || ws != 0 {
        // direct node
        if ws_port != ws {
            return Err(anyhow::anyhow!(
                "Binary used --ws-port flag to set port to {}, but node is using port {} onchain.",
                ws_port,
                ws
            ));
        }

        our.ws_routing = Some((node_ip, ws));
    }
    Ok(())
}
