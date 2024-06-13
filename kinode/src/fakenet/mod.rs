use alloy::network::{eip2718::Encodable2718, EthereumSigner, TransactionBuilder};
use alloy::providers::{Provider, ProviderBuilder, RootProvider};
use alloy::pubsub::PubSubFrontend;
use alloy::rpc::client::WsConnect;
use alloy::rpc::types::eth::{TransactionInput, TransactionRequest};
use alloy::signers::wallet::LocalWallet;
use alloy_primitives::{Address, Bytes, FixedBytes, U256};
use alloy_sol_types::{SolCall, SolValue};
use lib::core::{Identity, NodeRouting};
use std::net::Ipv4Addr;
use std::str::FromStr;

pub mod helpers;

use crate::{keygen, KNS_ADDRESS};
pub use helpers::RegisterHelpers::*;
pub use helpers::*;

// TODO move these into contracts registry, doublecheck optimism deployments
const FAKE_DOTDEV_TBA: &str = "0xB624D86187c2495888D42AbE0a15b6f6aaa557CF";
const FAKE_DOTOS_TBA: &str = "0xC466dc53e3e2a29A296fE38Bdeab60a7C023A383";
const FAKE_ZEROTH_TBA: &str = "0x10AfFE8d293d5c07be2633a67917502FefAdEef6";

const KINO_ACCOUNT_IMPL: &str = "0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0";
const KINO_MINTER_IMPL: &str = "0xCf7Ed3AccA5a467e9e704C703E8D87F634fB0Fc9";

const MULTICALL: &str = "0xcA11bde05977b3631167028862bE2a173976CA11";

const KINOMAP: &str = "0x5FC8d32690cc91D4c39d9d3abcBD16989F875707";

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

    let multicall_address = Address::from_str(MULTICALL)?;
    let dotos = Address::from_str(FAKE_DOTOS_TBA)?;
    let dotdev = Address::from_str(FAKE_DOTDEV_TBA)?;
    let kimap = Address::from_str(KINOMAP)?;

    let parts: Vec<&str> = name.split('.').collect();
    let label = parts[0];
    let minter = match parts.get(1) {
        Some(&"os") => dotos,
        Some(&"dev") => dotdev,
        _ => dotdev,
    };

    let endpoint = format!("ws://localhost:{}", fakechain_port);
    let ws = WsConnect::new(endpoint);

    let provider: RootProvider<PubSubFrontend> = ProviderBuilder::default().on_ws(ws).await?;

    // interesting, even if we have a minted name, this does not explicitly fail.
    // also note, fake.dev.os seems to currently work, need to gate dots from names?
    let mint_call = mintCall {
        who: wallet_address,
        label: Bytes::from(label.as_bytes().to_vec()),
        initialization: vec![].into(),
        erc721Data: vec![].into(),
        implementation: Address::from_str(KINO_ACCOUNT_IMPL).unwrap(),
    }
    .abi_encode();

    let nonce = provider.get_transaction_count(wallet_address).await?;

    let tx = TransactionRequest::default()
        .to(minter)
        .input(TransactionInput::new(mint_call.into()))
        .nonce(nonce)
        .with_chain_id(31337)
        .with_gas_limit(12_000_00)
        .with_max_priority_fee_per_gas(200_000_000_000)
        .with_max_fee_per_gas(300_000_000_000);

    // Build the transaction using the `EthereumSigner` with the provided signer.
    let tx_envelope = tx.build(&signer).await?;

    // Encode the transaction using EIP-2718 encoding.
    let tx_encoded = tx_envelope.encoded_2718();

    // Send the raw transaction and retrieve the transaction receipt.
    let _tx_hash = provider.send_raw_transaction(&tx_encoded).await?;

    // get tba to set KNS records
    let namehash: [u8; 32] = encode_namehash(name);

    let get_call = getCall {
        node: namehash.into(),
    }
    .abi_encode();

    let get_tx = TransactionRequest::default()
        .to(kimap)
        .input(TransactionInput::new(get_call.into()));

    let exists = provider.call(&get_tx).await?;
    println!("exists: {:?}", exists);

    let decoded = getCall::abi_decode_returns(&exists, false)?;

    let tba = decoded.tba;
    let _owner = decoded.owner;
    let bytes = decoded._2;
    // now set ip, port and pubkey

    println!("tba, owner and bytes: {:?}, {:?}, {:?}", tba, _owner, bytes);

    let localhost = Ipv4Addr::new(127, 0, 0, 1);
    // let ip = helpers::encode_ipv4_as_u128(localhost);

    let multicalls: Vec<Call> = vec![
        Call {
            target: kimap,
            callData: Bytes::from(
                noteCall {
                    note: "~ip".into(),
                    data: localhost.to_string().into(),
                }
                .abi_encode(),
            ),
        },
        Call {
            target: kimap,
            callData: Bytes::from(
                noteCall {
                    note: "~ws-port".into(),
                    data: ws_port.to_be_bytes().into(),
                }
                .abi_encode(),
            ),
        },
        Call {
            target: kimap,
            callData: Bytes::from(
                noteCall {
                    note: "~net-key".into(),
                    data: Bytes::from(pubkey.as_bytes().to_vec()),
                }
                .abi_encode(),
            ),
        },
    ];

    let multicall = aggregateCall { calls: multicalls }.abi_encode();

    let execute_call = executeCall {
        to: multicall_address,
        value: U256::from(0), // free mint
        data: multicall.into(),
        operation: 1, // ?
    }
    .abi_encode();

    let nonce = provider.get_transaction_count(wallet_address).await?;

    let tx = TransactionRequest::default()
        .to(tba)
        .input(TransactionInput::new(execute_call.into()))
        .nonce(nonce)
        .with_chain_id(31337)
        .with_gas_limit(12_000_00)
        .with_max_priority_fee_per_gas(200_000_000_000)
        .with_max_fee_per_gas(300_000_000_000);

    let tx_envelope = tx.build(&signer).await?;
    let tx_encoded = tx_envelope.encoded_2718();
    let _tx_hash = provider.send_raw_transaction(&tx_encoded).await?;

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
