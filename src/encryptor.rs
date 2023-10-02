extern crate generic_array;
extern crate num_traits;
extern crate rand;

use crate::types::*;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm,
    Key, // Or `Aes128Gcm`
    Nonce,
};
use anyhow::Result;
use generic_array::GenericArray;
use rand::{thread_rng, Rng};
use ring::signature::Ed25519KeyPair;
use rsa::{BigUint, Oaep, RsaPublicKey};
use serde_json;
use std::collections::HashMap;
use std::sync::Arc;

use crate::encryptor::num_traits::Num;

fn encrypt_data(secret_key_bytes: [u8; 32], data: Vec<u8>) -> Vec<u8> {
    let key = Key::<Aes256Gcm>::from_slice(&secret_key_bytes);
    let cipher = Aes256Gcm::new(key);

    let mut nonce_bytes: [u8; 12] = [0; 12];
    thread_rng().fill(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, data.as_ref())
        .expect("encryption failure!");
    let mut data = ciphertext;
    data.extend(nonce_bytes);

    data
}

fn decrypt_data(secret_key_bytes: [u8; 32], data: Vec<u8>) -> Vec<u8> {
    let nonce_bytes = data[data.len() - 12..].to_vec();
    let encrypted_bytes = data[..data.len() - 12].to_vec();
    let key = Key::<Aes256Gcm>::from_slice(&secret_key_bytes);
    let cipher = Aes256Gcm::new(&key);
    let nonce = GenericArray::from_slice(&nonce_bytes);
    let decrypted_bytes = cipher
        .decrypt(nonce, encrypted_bytes.as_ref())
        .expect("decryption failure!");

    decrypted_bytes
}

pub async fn encryptor(
    our: String,
    keypair: Arc<Ed25519KeyPair>,
    message_tx: MessageSender,
    mut recv_in_encryptor: MessageReceiver,
    print_tx: PrintSender,
) -> Result<()> {
    // Generally, the secret_id will be the ID that corresponds to a particular app or websocket connection
    // For authenticated + encrypted HTTP routes, the secret_id will always be "http_bindings"
    let mut secrets: HashMap<String, [u8; 32]> = HashMap::new(); // Store secrets as hex strings? Or as bytes?

    while let Some(kernel_message) = recv_in_encryptor.recv().await {
        let _ = print_tx
            .send(Printout {
                verbosity: 1,
                content: "ENCRYPTOR MESSAGE".to_string(),
            })
            .await;
        let KernelMessage {
            ref id,
            source,
            rsvp,
            message,
            payload,
            ..
        } = kernel_message;
        let Message::Request(Request { ipc: Some(ipc), .. }) = message else {
            let _ = print_tx
                .send(Printout {
                    verbosity: 1,
                    content: "encryptor: bad message".to_string(),
                })
                .await;
            continue;
        };

        let _ = print_tx
            .send(Printout {
                verbosity: 1,
                content: format!("ENCRYPTOR IPC: {}", ipc.clone()),
            })
            .await;

        match serde_json::from_str::<EncryptorMessage>(&ipc) {
            Ok(message) => {
                match message {
                    EncryptorMessage::GetKeyAction(GetKeyAction {
                        channel_id,
                        public_key_hex,
                    }) => {
                        let n = BigUint::from_str_radix(&public_key_hex.clone(), 16)
                            .expect("failed to parse hex string");
                        let e = BigUint::from(65537u32);

                        match RsaPublicKey::new(n, e) {
                            Ok(public_key) => {
                                let padding = Oaep::new::<sha2::Sha256>();
                                let mut rng = rand::rngs::OsRng;
                                let public_key_bytes = hex::decode(public_key_hex)
                                    .expect("failed to decode hex string");

                                let signed_public_key =
                                    keypair.sign(&public_key_bytes).as_ref().to_vec();

                                let encrypted_secret: Vec<u8>;
                                if let Some(secret) = secrets.get(&channel_id) {
                                    // Secret already exists
                                    // Encrypt the secret with the public key and return it
                                    encrypted_secret = public_key
                                        .encrypt(&mut rng, padding, secret)
                                        .expect("failed to encrypt message");
                                } else {
                                    // Secret does not exist, must create
                                    // Create a new secret, store it, encrypt it with the public key, and return it
                                    let mut secret = [0u8; 32];
                                    thread_rng().fill(&mut secret);
                                    secrets.insert(channel_id, secret);

                                    // Create a new AES-GCM cipher with the given key
                                    // So do I encrypt the
                                    encrypted_secret = public_key
                                        .encrypt(&mut rng, padding, &secret)
                                        .expect("failed to encrypt message");
                                }

                                let mut headers = HashMap::new();
                                headers.insert(
                                    "Content-Type".to_string(),
                                    "application/json".to_string(),
                                );

                                let target = match rsvp {
                                    Some(rsvp) => rsvp,
                                    None => Address {
                                        node: source.node.clone(),
                                        process: ProcessId::Name("http_server".into()),
                                    },
                                };
                                // Generate and send the response
                                let response = KernelMessage {
                                    id: *id,
                                    source: Address {
                                        node: our.clone(),
                                        process: ProcessId::Name("encryptor".into()),
                                    },
                                    target,
                                    rsvp: None,
                                    message: Message::Response((
                                        Response {
                                            ipc: Some(serde_json::json!({
                                                "status": 201,
                                                "headers": headers,
                                            }).to_string()),
                                            metadata: None,
                                        },
                                        None,
                                    )),
                                    payload: Some(Payload {
                                        mime: Some("application/json".to_string()),
                                        bytes: serde_json::json!({
                                            "encrypted_secret": hex::encode(encrypted_secret).to_string(),
                                            "signed_public_key": hex::encode(&signed_public_key).to_string(),
                                        }).to_string().as_bytes().to_vec(),
                                    }),
                                    signed_capabilities: None,
                                };

                                message_tx.send(response).await.unwrap();
                            }
                            Err(e) => {
                                let _ = print_tx
                                    .send(Printout {
                                        verbosity: 1,
                                        content: format!("Error: {}", e),
                                    })
                                    .await;
                            }
                        }
                    }
                    EncryptorMessage::DecryptAndForwardAction(DecryptAndForwardAction {
                        channel_id,
                        forward_to,
                        json,
                    }) => {
                        let _ = print_tx
                            .send(Printout {
                                verbosity: 1,
                                content: format!(
                                    "DECRYPTOR TO FORWARD: {}",
                                    json.clone().unwrap_or_default().to_string()
                                ),
                            })
                            .await;

                        // The payload.bytes should be the encrypted data, with the last 12 bytes being the nonce
                        let Some(payload) = payload else {
                            let _ = print_tx
                                .send(Printout {
                                    verbosity: 1,
                                    content: "No payload".to_string(),
                                })
                                .await;
                            continue;
                        };

                        let data = payload.bytes.clone();

                        if let Some(secret_key_bytes) = secrets.get(&channel_id) {
                            let decrypted_bytes = decrypt_data(secret_key_bytes.clone(), data);

                            // Forward the unencrypted data to the target
                            let id: u64 = rand::random();
                            let message = KernelMessage {
                                id: id.clone(),
                                source: Address {
                                    node: our.clone(),
                                    process: ProcessId::Name("encryptor".into()),
                                },
                                target: forward_to,
                                rsvp: None,
                                message: Message::Request(Request {
                                    inherit: false,
                                    expects_response: None, // A forwarded message does not expect a response
                                    ipc: Some(json.clone().unwrap_or_default().to_string()),
                                    metadata: None,
                                }),
                                payload: Some(Payload {
                                    mime: Some("application/octet-stream".to_string()), // TODO adjust MIME type as needed
                                    bytes: decrypted_bytes,
                                }),
                                signed_capabilities: None,
                            };
                            message_tx.send(message).await.unwrap();
                        } else {
                            panic!("No secret found");
                        }
                    }
                    EncryptorMessage::EncryptAndForwardAction(EncryptAndForwardAction {
                        channel_id,
                        forward_to,
                        json,
                    }) => {
                        let _ = print_tx
                            .send(Printout {
                                verbosity: 1,
                                content: format!("ENCRYPTOR TO FORWARD"),
                            })
                            .await;

                        let Some(payload) = payload else {
                            let _ = print_tx
                                .send(Printout {
                                    verbosity: 1,
                                    content: "No payload".to_string(),
                                })
                                .await;
                            continue;
                        };

                        let data = payload.bytes.clone();

                        if let Some(secret_key_bytes) = secrets.get(&channel_id) {
                            let encrypted_bytes = encrypt_data(secret_key_bytes.clone(), data);

                            // Forward the ciphertext and nonce_hex to the specified process
                            let id: u64 = rand::random();
                            let message = KernelMessage {
                                id,
                                source: Address {
                                    node: our.clone(),
                                    process: ProcessId::Name("encryptor".into()),
                                },
                                target: forward_to,
                                rsvp: None,
                                message: Message::Request(Request {
                                    inherit: false,
                                    expects_response: None, // A forwarded message does not expect a response
                                    ipc: Some(json.clone().unwrap_or_default().to_string()),
                                    metadata: None,
                                }),
                                payload: Some(Payload {
                                    mime: Some("application/octet-stream".to_string()), // TODO adjust MIME type as needed
                                    bytes: encrypted_bytes,
                                }),
                                signed_capabilities: None,
                            };

                            message_tx.send(message).await.unwrap();
                        } else {
                            let _ = print_tx
                                .send(Printout {
                                    verbosity: 1,
                                    content: format!("ERROR: No secret found"),
                                })
                                .await;
                        }
                    }
                    EncryptorMessage::DecryptAction(DecryptAction { channel_id }) => {
                        let _ = print_tx
                            .send(Printout {
                                verbosity: 1,
                                content: format!("ENCRYPTOR TO DECRYPT"),
                            })
                            .await;

                        let Some(payload) = payload else {
                            let _ = print_tx
                                .send(Printout {
                                    verbosity: 1,
                                    content: "No payload".to_string(),
                                })
                                .await;
                            continue;
                        };

                        let data = payload.bytes.clone();

                        if let Some(secret_key_bytes) = secrets.get(&channel_id) {
                            let decrypted_bytes = decrypt_data(secret_key_bytes.clone(), data);

                            let message = KernelMessage {
                                id: *id,
                                source: Address {
                                    node: our.clone(),
                                    process: ProcessId::Name("encryptor".into()),
                                },
                                target: source,
                                rsvp: None,
                                message: Message::Response((
                                    Response {
                                        ipc: None,
                                        metadata: None,
                                    },
                                    None,
                                )),
                                payload: Some(Payload {
                                    mime: Some("application/octet-stream".to_string()), // TODO adjust MIME type as needed
                                    bytes: decrypted_bytes,
                                }),
                                signed_capabilities: None,
                            };

                            message_tx.send(message).await.unwrap();
                        } else {
                            let _ = print_tx
                                .send(Printout {
                                    verbosity: 1,
                                    content: format!("ERROR: No secret found"),
                                })
                                .await;
                        }
                    }
                    EncryptorMessage::EncryptAction(EncryptAction { channel_id }) => {
                        let _ = print_tx
                            .send(Printout {
                                verbosity: 1,
                                content: format!("ENCRYPTOR TO ENCRYPT"),
                            })
                            .await;

                        let Some(payload) = payload else {
                            let _ = print_tx
                                .send(Printout {
                                    verbosity: 1,
                                    content: "No payload".to_string(),
                                })
                                .await;
                            continue;
                        };

                        let data = payload.bytes.clone();

                        if let Some(secret_key_bytes) = secrets.get(&channel_id) {
                            let encrypted_bytes = encrypt_data(secret_key_bytes.clone(), data);

                            let message = KernelMessage {
                                id: *id,
                                source: Address {
                                    node: our.clone(),
                                    process: ProcessId::Name("encryptor".into()),
                                },
                                target: source,
                                rsvp: None,
                                message: Message::Response((
                                    Response {
                                        ipc: None,
                                        metadata: None,
                                    },
                                    None,
                                )),
                                payload: Some(Payload {
                                    mime: Some("application/octet-stream".to_string()), // TODO adjust MIME type as needed
                                    bytes: encrypted_bytes,
                                }),
                                signed_capabilities: None,
                            };

                            message_tx.send(message).await.unwrap();
                        } else {
                            let _ = print_tx
                                .send(Printout {
                                    verbosity: 1,
                                    content: format!("ERROR: No secret found"),
                                })
                                .await;
                        }
                    }
                }
            }
            Err(_) => {
                let _ = print_tx
                    .send(Printout {
                        verbosity: 1,
                        content: "Not a valid EncryptorMessage".to_string(),
                    })
                    .await;
            }
        }
    }
    Err(anyhow::anyhow!("encryptor: exited"))
}
