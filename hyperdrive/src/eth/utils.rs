use crate::eth::{Providers, UrlProvider};
use alloy::providers::ProviderBuilder;
use alloy::rpc::client::WsConnect;
use anyhow::Result;
use lib::types::core::*;
use lib::types::eth::*;
use serde::Serialize;
use url::Url;

pub async fn activate_url_provider(provider: &mut UrlProvider) -> Result<()> {
    match Url::parse(&provider.url)?.scheme() {
        "ws" | "wss" => {
            let ws = WsConnect {
                url: provider.url.to_string(),
                auth: provider.auth.clone().map(|a| a.into()),
                config: None,
            };

            let client = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                ProviderBuilder::new().on_ws(ws),
            )
            .await??;
            provider.pubsub.push(client);
            Ok(())
        }
        _ => Err(anyhow::anyhow!(
            "Only `ws://` or `wss://` providers are supported."
        )),
    }
}

pub fn providers_to_saved_configs(providers: &Providers) -> SavedConfigs {
    SavedConfigs(
        providers
            .iter()
            .map(|entry| {
                entry
                    .urls
                    .iter()
                    .map(|url_provider| ProviderConfig {
                        chain_id: *entry.key(),
                        provider: NodeOrRpcUrl::RpcUrl {
                            url: url_provider.url.clone(),
                            auth: url_provider.auth.clone(),
                        },
                        trusted: url_provider.trusted,
                    })
                    .chain(entry.nodes.iter().map(|node_provider| ProviderConfig {
                        chain_id: *entry.key(),
                        provider: NodeOrRpcUrl::Node {
                            hns_update: node_provider.hns_update.clone(),
                            use_as_provider: node_provider.usable,
                        },
                        trusted: node_provider.trusted,
                    }))
                    .collect::<Vec<_>>()
            })
            .flatten()
            .collect(),
    )
}

pub async fn check_for_root_cap(
    our: &str,
    process: &ProcessId,
    caps_oracle: &CapMessageSender,
) -> bool {
    let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
    caps_oracle
        .send(CapMessage::Has {
            on: process.clone(),
            cap: Capability::new((our, ETH_PROCESS_ID.clone()), "{\"root\":true}"),
            responder: send_cap_bool,
        })
        .await
        .expect("eth: capability oracle died!");
    recv_cap_bool.await.unwrap_or(false)
}

pub async fn verbose_print(print_tx: &PrintSender, content: &str) {
    let _ = print_tx
        .send(Printout::new(
            2,
            NET_PROCESS_ID.clone(),
            content.to_string(),
        ))
        .await;
}

pub async fn error_message(
    our: &str,
    km_id: u64,
    target: Address,
    error: EthError,
    send_to_loop: &MessageSender,
) {
    kernel_message(
        our,
        km_id,
        target,
        None,
        false,
        None,
        EthResponse::Err(error),
        send_to_loop,
    )
    .await
}

pub async fn kernel_message<T: Serialize>(
    our: &str,
    km_id: u64,
    target: Address,
    rsvp: Option<Address>,
    req: bool,
    timeout: Option<u64>,
    body: T,
    send_to_loop: &MessageSender,
) {
    let Err(e) = send_to_loop.try_send(KernelMessage {
        id: km_id,
        source: Address {
            node: our.to_string(),
            process: ETH_PROCESS_ID.clone(),
        },
        target,
        rsvp,
        message: if req {
            Message::Request(Request {
                inherit: false,
                expects_response: timeout,
                body: serde_json::to_vec(&body).unwrap(),
                metadata: None,
                capabilities: vec![],
            })
        } else {
            Message::Response((
                Response {
                    inherit: false,
                    body: serde_json::to_vec(&body).unwrap(),
                    metadata: None,
                    capabilities: vec![],
                },
                None,
            ))
        },
        lazy_load_blob: None,
    }) else {
        // not Err -> send successful; done here
        return;
    };
    // its an Err: handle
    match e {
        tokio::sync::mpsc::error::TrySendError::Closed(_) => {
            return;
        }
        tokio::sync::mpsc::error::TrySendError::Full(_) => {
            // TODO: implement backpressure
            panic!("(eth) kernel overloaded with messages: TODO: implement backpressure");
        }
    }
}

pub fn find_index(vec: &Vec<&str>, item: &str) -> Option<usize> {
    vec.iter().enumerate().find_map(
        |(index, value)| {
            if *value == item {
                Some(index)
            } else {
                None
            }
        },
    )
}

pub async fn set_node_unusable(
    providers: &Providers,
    chain_id: &u64,
    node_name: &str,
    print_tx: &PrintSender,
) -> bool {
    let mut is_replacement_successful = true;
    providers.entry(chain_id.clone()).and_modify(|aps| {
        let Some(index) = find_index(
            &aps.nodes
                .iter()
                .map(|n| n.hns_update.name.as_str())
                .collect(),
            &node_name,
        ) else {
            is_replacement_successful = false;
            return ();
        };
        let mut node = aps.nodes.remove(index);
        node.usable = false;
        aps.nodes.insert(index, node);
    });
    if !is_replacement_successful {
        verbose_print(
            print_tx,
            &format!("eth: unexpectedly couldn't find provider to be modified"),
        )
        .await;
    }
    is_replacement_successful
}
