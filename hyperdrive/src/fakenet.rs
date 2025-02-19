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

use crate::{keygen, sol::*, HYPERMAP_ADDRESS, KINO_ACCOUNT_IMPL, MULTICALL_ADDRESS};

const FAKE_DOTDEV_TBA: &str = "0xcc3A576b8cE5340f5CE23d0DDAf133C0822C3B6d";
const FAKE_DOTOS_TBA: &str = "0xbE46837617f8304Aa5E6d0aE62B74340251f48Bf";
const _FAKE_ZEROTH_TBA: &str = "0x4bb0778bb92564bf8e82d0b3271b7512443fb060";

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

    let multicall_address = Address::from_str(MULTICALL_ADDRESS)?;
    let dotos = Address::from_str(FAKE_DOTOS_TBA)?;
    let dotdev = Address::from_str(FAKE_DOTDEV_TBA)?;
    let hypermap = Address::from_str(HYPERMAP_ADDRESS)?;

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

    // get tba to see if name is already registered
    let namehash: [u8; 32] = keygen::namehash(name);

    let get_call = getCall {
        namehash: namehash.into(),
    }
    .abi_encode();

    let get_tx = TransactionRequest::default()
        .to(hypermap)
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
            target: hypermap,
            callData: Bytes::from(
                noteCall {
                    note: "~ip".into(),
                    data: ip.into(),
                }
                .abi_encode(),
            ),
        },
        Call {
            target: hypermap,
            callData: Bytes::from(
                noteCall {
                    note: "~ws-port".into(),
                    data: ws_port.to_be_bytes().into(),
                }
                .abi_encode(),
            ),
        },
        Call {
            target: hypermap,
            callData: Bytes::from(
                noteCall {
                    note: "~net-key".into(),
                    data: Bytes::from(pubkey),
                }
                .abi_encode(),
            ),
        },
    ];

    let is_reset = tba != Address::default();

    let multicall = aggregateCall { calls: multicalls }.abi_encode();

    let execute_call: Vec<u8> = executeCall {
        to: multicall_address,
        value: U256::from(0), // free mint
        data: multicall.into(),
        operation: 1,
    }
    .abi_encode();

    let (input_bytes, to) = if is_reset {
        // name is already registered, multicall reset it
        (execute_call, tba)
    } else {
        // name is not registered, mint it with multicall in initialization param
        (
            mintCall {
                to: wallet_address,
                label: Bytes::from(label.as_bytes().to_vec()),
                initialization: execute_call.into(),
                implementation: Address::from_str(KINO_ACCOUNT_IMPL).unwrap(),
            }
            .abi_encode(),
            minter,
        )
    };

    let nonce = provider.get_transaction_count(wallet_address).await?;

    let tx = TransactionRequest::default()
        .to(to)
        .input(TransactionInput::new(input_bytes.into()))
        .nonce(nonce)
        .with_chain_id(31337)
        .with_gas_limit(12_000_00)
        .with_max_priority_fee_per_gas(200_000_000_000)
        .with_max_fee_per_gas(300_000_000_000);

    // Build the transaction using the `EthereumSigner` with the provided signer.
    let tx_envelope = tx.build(&wallet).await?;

    // Encode the transaction using EIP-2718 encoding.
    let tx_encoded = tx_envelope.encoded_2718();

    // Send the raw transaction and retrieve the transaction receipt.
    let tx_hash = provider.send_raw_transaction(&tx_encoded).await?;
    let _receipt = tx_hash.get_receipt().await?;

    // send a small amount of ETH to the zero address
    // this is a workaround to get anvil to mine a block after our registration tx
    // instead of doing block-time 1s or similar, which leads to runaway mem-usage.
    let zero_address = Address::default();
    let small_amount = U256::from(10); // 10 wei (0.00000001 ETH)

    let nonce = provider.get_transaction_count(wallet_address).await?;

    let small_tx = TransactionRequest::default()
        .to(zero_address)
        .value(small_amount)
        .nonce(nonce)
        .with_chain_id(31337)
        .with_gas_limit(21_000)
        .with_max_priority_fee_per_gas(200_000_000_000)
        .with_max_fee_per_gas(300_000_000_000);

    let small_tx_envelope = small_tx.build(&wallet).await?;
    let small_tx_encoded = small_tx_envelope.encoded_2718();

    let small_tx_hash = provider.send_raw_transaction(&small_tx_encoded).await?;
    let _small_receipt = small_tx_hash.get_receipt().await?;

    Ok(())
}

/// Booting from a keyfile, fetches the node's IP data from the HNS contract
/// and assigns it to the Identity struct.
pub async fn assign_ws_local_helper(
    our: &mut Identity,
    ws_port: u16,
    fakechain_port: u16,
) -> Result<(), anyhow::Error> {
    let hypermap = Address::from_str(HYPERMAP_ADDRESS)?;
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
            target: hypermap,
            callData: Bytes::from(
                getCall {
                    namehash: netkey_hash,
                }
                .abi_encode(),
            ),
        },
        Call {
            target: hypermap,
            callData: Bytes::from(getCall { namehash: ws_hash }.abi_encode()),
        },
        Call {
            target: hypermap,
            callData: Bytes::from(getCall { namehash: ip_hash }.abi_encode()),
        },
    ];

    let multicall_call = aggregateCall { calls: multicalls }.abi_encode();

    let tx_input = TransactionInput::new(Bytes::from(multicall_call));
    let tx = TransactionRequest::default().to(multicall).input(tx_input);

    let Ok(multicall_return) = provider.call(&tx).await else {
        return Err(anyhow::anyhow!("Failed to fetch node IP data from hypermap"));
    };

    let Ok(results) = aggregateCall::abi_decode_returns(&multicall_return, false) else {
        return Err(anyhow::anyhow!("Failed to decode hypermap multicall data"));
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
