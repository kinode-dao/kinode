use crate::types::*;
use crate::llm::types::*;
use anyhow::Result;
use reqwest::Response as ReqwestResponse;

mod types;

pub async fn llm(
    our_name: String,
    send_to_loop: MessageSender,
    mut recv_in_client: MessageReceiver,
    llm_url: String,
    print_tx: PrintSender,
) -> Result<()> {
    while let Some(message) = recv_in_client.recv().await {
        let KernelMessage {
            id,
            source,
            rsvp,
            message:
                Message::Request(Request {
                    expects_response,
                    ipc,
                    ..
                }),
            ..
        } = message.clone()
        else {
            return Err(anyhow::anyhow!("llm: bad message"));
        };

        let our_name = our_name.clone();
        let llm_url = llm_url.clone();
        let send_to_loop = send_to_loop.clone();
        let print_tx = print_tx.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_message(
                our_name.clone(),
                send_to_loop.clone(),
                llm_url.clone(),
                id,
                rsvp,
                expects_response,
                source.clone(),
                ipc,
                print_tx.clone(),
            )
            .await
            {
                send_to_loop
                    .send(make_error_message(our_name.clone(), id, source, e))
                    .await
                    .unwrap();
            }
        });
    }
    Err(anyhow::anyhow!("llm: exited"))
}

async fn handle_message(
    our: String,
    send_to_loop: MessageSender,
    llm_url: String,
    id: u64,
    rsvp: Option<Address>,
    expects_response: Option<u64>,
    source: Address,
    json: Vec<u8>,
    _print_tx: PrintSender,
) -> Result<(), LlmError> {
    let target = if expects_response.is_some() {
        source.clone()
    } else if source.process == ProcessId::from_str("terminal:terminal:uqbar").unwrap() {
        source.clone()
    } else {
        let Some(rsvp) = rsvp else {
            return Err(LlmError::BadRsvp);
        };
        rsvp.clone()
    };

    let req: LlmPrompt = match serde_json::from_slice(&json) {
        Ok(req) => req,
        Err(e) => {
            return Err(LlmError::BadJson {
                json: String::from_utf8(json).unwrap_or_default(),
                error: format!("{}", e),
            })
        }
    };

    let client = reqwest::Client::new();

    let res: ReqwestResponse = match client
        .post(&format!("{}/completion", llm_url))
        .json(&req)
        .send()
        .await
    {
        Ok(res) => res,
        Err(e) => {
            return Err(LlmError::RequestFailed {
                error: format!("{}", e),
            });
        }
    };

    let llm_response = match res.json::<LlmResponse>().await {
        Ok(response) => response,
        Err(e) => {
            return Err(LlmError::DeserializationToLlmResponseFailed {
                error: format!("{}", e),
            });
        }
    };

    let _ = _print_tx
        .send(Printout {
            verbosity: 0,
            content: format!("llm: {:?}", llm_response.clone().content),
        })
        .await;

    let message = KernelMessage {
        id,
        source: Address {
            node: our,
            process: ProcessId::new(Some("llm"), "sys", "uqbar"),
        },
        target,
        rsvp: None,
        message: Message::Response((
            Response {
                inherit: false,
                ipc: serde_json::to_vec::<Result<LlmResponse, LlmError>>(&Ok(llm_response))
                    .unwrap(),
                metadata: None,
            },
            None,
        )),
        payload: None,
        signed_capabilities: None,
    };

    send_to_loop.send(message).await.unwrap();

    Ok(())
}

//
//  helpers
//
fn make_error_message(
    our_name: String,
    id: u64,
    source: Address,
    error: LlmError,
) -> KernelMessage {
    KernelMessage {
        id,
        source: source.clone(),
        target: Address {
            node: our_name.clone(),
            process: source.process.clone(),
        },
        rsvp: None,
        message: Message::Response((
            Response {
                inherit: false,
                ipc: serde_json::to_vec::<Result<HttpClientResponse, LlmError>>(&Err(error))
                    .unwrap(),
                metadata: None,
            },
            None,
        )),
        payload: None,
        signed_capabilities: None,
    }
}
