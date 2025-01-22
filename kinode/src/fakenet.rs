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
const FAKE_DOTDEV_TBA: &str = "0x27e913BF6dcd08E9E68530812B277224Be07890B";
const FAKE_DOTOS_TBA: &str = "0xC026fE4950c12AdACF284689d900AcC74987c555";
const _FAKE_ZEROTH_TBA: &str = "0x33b687295Cb095d9d962BA83732c67B96dffC8eA";

const KINO_ACCOUNT_IMPL: &str = "0x00ee0e0d00F01f6FF3aCcBA2986E07f99181b9c2";

const MULTICALL: &str = "0xcA11bde05977b3631167028862bE2a173976CA11";

const KIMAP: &str = "0x9CE8cCD2932DC727c70f9ae4f8C2b68E6Abed58C";

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

    // get tba to see if name is already registered
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
                who: wallet_address,
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
