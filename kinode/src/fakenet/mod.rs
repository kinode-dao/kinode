use alloy::network::{eip2718::Encodable2718, EthereumSigner, TransactionBuilder};
use alloy::providers::{Provider, ProviderBuilder, RootProvider};
use alloy::pubsub::PubSubFrontend;
use alloy::rpc::client::WsConnect;
use alloy::rpc::types::eth::{TransactionInput, TransactionRequest};
use alloy::signers::wallet::LocalWallet;
use alloy_primitives::{Address, Bytes, FixedBytes, B256, U256};
use alloy_sol_types::{SolCall, SolValue};
use lib::core::{Identity, NodeRouting};
use std::str::FromStr;

pub mod helpers;

use crate::{keygen, KNS_ADDRESS};
pub use helpers::RegisterHelpers::*;
pub use helpers::*;

const FAKE_DOTDEV: &str = "0xDc64a140Aa3E981100a9becA4E685f962f0cF6C9";
const FAKE_DOTOS: &str = "0xC466dc53e3e2a29A296fE38Bdeab60a7C023A383";

const KINO_ACCOUNT_IMPL: &str = "0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0";
const KINOMAP: &str = "0x5FC8d32690cc91D4c39d9d3abcBD16989F875707"; // "0x68B1D87F95878fE05B998F19b66F4baba5De1aed";

/// Attempts to connect to a local anvil fakechain,
/// registering a name with its KiMap contract.
/// If name is already registered, resets it.
pub async fn mint_local(
    name: &str,
    ws_port: u16,
    pubkey: &str,
    fakechain_port: u16,
) -> Result<(), anyhow::Error> {
    let wallet = LocalWallet::from_str(
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
    )?;

    let wallet_address = wallet.address();

    let signer: EthereumSigner = wallet.into();

    let dotos = Address::from_str(FAKE_DOTOS)?;
    let kimap = Address::from_str(KINOMAP)?;

    let endpoint = format!("ws://localhost:{}", fakechain_port);
    let ws = WsConnect::new(endpoint);

    let provider: RootProvider<PubSubFrontend> = ProviderBuilder::default().on_ws(ws).await?;

    let namehash = encode_namehash(name);

    // interesting, even if we have a minted name, this does not explicitly fail.
    let replicate_call = replicateCall {
        who: wallet.address(),
        name: name.as_bytes().to_vec(),
        initialization: vec![],
        erc721Data: vec![],
        implementation: Address::from_str(KINO_ACCOUNT_IMPL).unwrap(),
    }
    .abi_encode();

    let nonce = provider
        .get_transaction_count(wallet.address(), None)
        .await?;

    let tx = TransactionRequest::default()
        .to(dotos)
        .input(TransactionInput::new(replicate_call.into()));

    let nonce = provider.get_transaction_count(wallet_address).await?;

    let tx = TransactionRequest::default()
        .to(to)
        .input(input)
        .nonce(nonce)
        .with_chain_id(31337)
        .with_gas_limit(500_000)
        .with_max_priority_fee_per_gas(1_000_000_000)
        .with_max_fee_per_gas(20_000_000_000);

    // Build the transaction using the `EthereumSigner` with the provided signer.
    let tx_envelope = tx.build(&signer).await?;

    // Encode the transaction using EIP-2718 encoding.
    let tx_encoded = tx_envelope.encoded_2718();

    // Send the raw transaction and retrieve the transaction receipt.
    let _tx_hash = provider.send_raw_transaction(&tx_encoded).await?;

    // try to get name, if there isn't one, replicate it from .dev
    let get_call = getCall {
        node: namehash.into(),
    }
    .abi_encode();

    let get_tx = TransactionRequest::default()
        .to(Some(kimap))
        .input(TransactionInput::new(get_call.into()));

    let exists = provider.call(get_tx, None).await?;
    println!("exists: {:?}", exists);

    // todo abi_decode() properly into getReturn.
    // tba, owner.
    // also note, should be (Address, Address, Bytes), but that fails on a normal node!?
    // fix the sol Value decoding, update alloy deps. currently we do not need the note bytes but will in the future.
    let decoded = <(Address, Address)>::abi_decode(&exists, false)?;

    let tba = decoded.0;
    let _owner = decoded.1;
    // now set ip, port and pubkey
    // multicall coming on contracts soon, will make it easier.

    let port_call = noteCall {
        note: "~wsport".as_bytes().to_vec(),
        data: ws_port.to_string().as_bytes().to_vec(),
    };

    let ip_call = noteCall {
        note: "~ip".as_bytes().to_vec(),
        data: "127.0.0.1".as_bytes().to_vec(),
    };

    let pubkey_call = noteCall {
        note: "~networkingkey".as_bytes().to_vec(),
        data: pubkey.as_bytes().to_vec(),
    };

    let calls = vec![port_call, ip_call, pubkey_call];

    for call in calls {
        let note_call = call.abi_encode();

        let execute_call = executeCall {
            to: kimap,
            value: U256::from(0),
            data: note_call,
            operation: 0,
        }
        .abi_encode();

        let nonce = provider
            .get_transaction_count(wallet.address(), None)
            .await?;

        let mut tx = TxLegacy {
            to: TxKind::Call(tba),
            nonce: nonce.to::<u64>(),
            input: execute_call.into(),
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
    }

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
    let ws = WsConnect::new(endpoint);

    let provider: RootProvider<PubSubFrontend> = ProviderBuilder::default().on_ws(ws).await?;

    let namehash = FixedBytes::<32>::from_slice(&keygen::namehash(&our.name));
    let ip_call = ipCall { _0: namehash }.abi_encode();
    let tx_input = TransactionInput::new(Bytes::from(ip_call));
    let tx = TransactionRequest::default().to(kns).input(tx_input);

    let Ok(ip_data) = provider.call(&tx).await else {
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

        our.routing = NodeRouting::Direct {
            ip: node_ip,
            ports: std::collections::BTreeMap::from([("ws".to_string(), ws)]),
        };
    }
    Ok(())
}
