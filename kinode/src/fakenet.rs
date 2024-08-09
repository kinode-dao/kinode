use alloy::network::{eip2718::Encodable2718, EthereumWallet, TransactionBuilder};
use alloy::providers::{Provider, ProviderBuilder, RootProvider};
use alloy::pubsub::PubSubFrontend;
use alloy::rpc::client::WsConnect;
use alloy::rpc::types::eth::{TransactionInput, TransactionRequest};
use alloy::signers::local::PrivateKeySigner;
use alloy_primitives::{Address, Bytes, FixedBytes, U256};
use alloy_sol_types::SolCall;
use lib::core::{Identity, NodeRouting};
use std::net::Ipv4Addr;
use std::str::FromStr;

use crate::{keygen, sol::*, KIMAP_ADDRESS, MULTICALL_ADDRESS};

// TODO move these into contracts registry, doublecheck optimism deployments
const FAKE_DOTDEV_TBA: &str = "0x1a5447E634aa056Fa302E48630Da8425EC15A53A";
const FAKE_DOTOS_TBA: &str = "0xF5FaB379Eb87599d7B5BaBeDDEFe6EfDEC6164b0";
const _FAKE_ZEROTH_TBA: &str = "0x02dd7FB5ca377b1a6E2960EB139aF390a24D28FA";

const KINO_ACCOUNT_IMPL: &str = "0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0";

const MULTICALL: &str = "0xcA11bde05977b3631167028862bE2a173976CA11";

const KIMAP: &str = "0xEce71a05B36CA55B895427cD9a440eEF7Cf3669D";

/// Attempts to connect to a local anvil fakechain,
/// registering a name with its KiMap contract.
/// If name is already registered, resets it.
pub async fn mint_local(
    name: &str,
    ws_port: u16,
    pubkey: &str,
    fakechain_port: u16,
) -> Result<(), anyhow::Error> {
    let privkey_signer = PrivateKeySigner::from_str(
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
    )?;

    let wallet_address = privkey_signer.address();

    let wallet: EthereumWallet = privkey_signer.into();

    let multicall_address = Address::from_str(MULTICALL)?;
    let dotos = Address::from_str(FAKE_DOTOS_TBA)?;
    let dotdev = Address::from_str(FAKE_DOTDEV_TBA)?;
    let kimap = Address::from_str(KIMAP)?;

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
        .with_gas_limit(2_000_000)
        .with_max_priority_fee_per_gas(200_000_000_000)
        .with_max_fee_per_gas(300_000_000_000);

    // Build the transaction using the `EthereumSigner` with the provided signer.
    let tx_envelope = tx.build(&wallet).await?;

    // Encode the transaction using EIP-2718 encoding.
    let tx_encoded = tx_envelope.encoded_2718();

    // Send the raw transaction and retrieve the transaction receipt.
    let tx_hash = provider.send_raw_transaction(&tx_encoded).await?;
    let _tx_receipt = tx_hash.get_receipt().await?;

    // get tba to set KNS records
    let namehash: [u8; 32] = keygen::namehash(name);

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
    let bytes = decoded.data;
    // now set ip, port and pubkey

    println!("tba, owner and bytes: {:?}, {:?}, {:?}", tba, _owner, bytes);

    let localhost = Ipv4Addr::new(127, 0, 0, 1);
    let ip = keygen::ip_to_bytes(localhost.into());
    let pubkey = hex::decode(pubkey)?;
    let multicalls: Vec<Call> = vec![
        Call {
            target: kimap,
            callData: Bytes::from(
                noteCall {
                    note: "~ip".into(),
                    data: ip.into(),
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
                    data: Bytes::from(pubkey),
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
        .with_gas_limit(2_000_000)
        .with_max_priority_fee_per_gas(200_000_000_000)
        .with_max_fee_per_gas(300_000_000_000);

    let tx_envelope = tx.build(&wallet).await?;
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
    let kimap = Address::from_str(KIMAP_ADDRESS)?;
    let multicall = Address::from_str(MULTICALL_ADDRESS)?;

    let endpoint = format!("ws://localhost:{}", fakechain_port);
    let ws = WsConnect::new(endpoint);

    let provider: RootProvider<PubSubFrontend> = ProviderBuilder::default().on_ws(ws).await?;

    let netkey_hash = FixedBytes::<32>::from_slice(&keygen::namehash(&format!("~ip.{}", our.name)));
    let ws_hash =
        FixedBytes::<32>::from_slice(&keygen::namehash(&format!("~ws-port.{}", our.name)));
    let ip_hash = FixedBytes::<32>::from_slice(&keygen::namehash(&format!("~ip.{}", our.name)));

    let multicalls = vec![
        Call {
            target: kimap,
            callData: Bytes::from(getCall { node: netkey_hash }.abi_encode()),
        },
        Call {
            target: kimap,
            callData: Bytes::from(getCall { node: ws_hash }.abi_encode()),
        },
        Call {
            target: kimap,
            callData: Bytes::from(getCall { node: ip_hash }.abi_encode()),
        },
    ];

    let multicall_call = aggregateCall { calls: multicalls }.abi_encode();

    let tx_input = TransactionInput::new(Bytes::from(multicall_call));
    let tx = TransactionRequest::default().to(multicall).input(tx_input);

    let Ok(multicall_return) = provider.call(&tx).await else {
        return Err(anyhow::anyhow!("Failed to fetch node IP data from kimap"));
    };

    let Ok(results) = aggregateCall::abi_decode_returns(&multicall_return, false) else {
        return Err(anyhow::anyhow!("Failed to decode kimap multicall data"));
    };

    let netkey = getCall::abi_decode_returns(&results.returnData[0], false)?;
    let _netkey_data = netkey.data;

    let ws = getCall::abi_decode_returns(&results.returnData[1], false)?;
    let ws_data = ws.data;

    let ip = getCall::abi_decode_returns(&results.returnData[2], false)?;
    let ip_data = ip.data;

    let ip = keygen::bytes_to_ip(&ip_data);
    let ws = keygen::bytes_to_port(&ws_data);

    // tweak
    if ip.is_ok() && ws.is_ok() {
        // direct node
        let ws = ws.unwrap();
        let ip = ip.unwrap();
        if ws_port != ws {
            return Err(anyhow::anyhow!(
                "Binary used --ws-port flag to set port to {}, but node is using port {} onchain.",
                ws_port,
                ws
            ));
        }

        our.routing = NodeRouting::Direct {
            ip: ip.to_string(),
            ports: std::collections::BTreeMap::from([("ws".to_string(), ws)]),
        };
    }
    Ok(())
}
