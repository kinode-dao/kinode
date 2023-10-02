use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key,
};
use digest::generic_array;
use lazy_static::__Deref;
use ring::pbkdf2;
use ring::pkcs8::Document;
use ring::rand::SystemRandom;
use ring::signature::{self, KeyPair};
use ring::{digest as ring_digest, rand::SecureRandom};
use std::num::NonZeroU32;

type DiskKey = [u8; CREDENTIAL_LEN];

pub const CREDENTIAL_LEN: usize = ring_digest::SHA256_OUTPUT_LEN;
pub const ITERATIONS: u32 = 1_000_000;
pub static PBKDF2_ALG: pbkdf2::Algorithm = pbkdf2::PBKDF2_HMAC_SHA256; // TODO maybe look into Argon2

pub fn encode_keyfile(
    password: String,
    username: String,
    routers: Vec<String>,
    networking_key: Document,
    jwt: Vec<u8>,
    file_key: Vec<u8>,
) -> Vec<u8> {
    println!("generating disk encryption keys...");
    let mut disk_key: DiskKey = [0u8; CREDENTIAL_LEN];

    let rng = SystemRandom::new();

    let mut salt = [0u8; 32]; // generate a unique salt
    rng.fill(&mut salt).unwrap();

    pbkdf2::derive(
        PBKDF2_ALG,
        NonZeroU32::new(ITERATIONS).unwrap(),
        &salt,
        password.as_bytes(),
        &mut disk_key,
    );

    let key = Key::<Aes256Gcm>::from_slice(&disk_key);
    let cipher = Aes256Gcm::new(&key);

    let network_nonce = Aes256Gcm::generate_nonce(&mut OsRng); // 96-bits; unique per message
    let jwt_nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let file_nonce = Aes256Gcm::generate_nonce(&mut OsRng);

    let keyciphertext: Vec<u8> = cipher
        .encrypt(&network_nonce, networking_key.as_ref())
        .unwrap();
    let jwtciphertext: Vec<u8> = cipher.encrypt(&jwt_nonce, jwt.as_ref()).unwrap();
    let fileciphertext: Vec<u8> = cipher.encrypt(&file_nonce, file_key.as_ref()).unwrap();

    bincode::serialize(&(
        username.clone(),
        routers.clone(),
        salt.to_vec(),
        [network_nonce.deref().to_vec(), keyciphertext].concat(),
        [jwt_nonce.deref().to_vec(), jwtciphertext].concat(),
        [file_nonce.deref().to_vec(), fileciphertext].concat(),
    ))
    .unwrap()
}

pub fn decode_keyfile(
    keyfile: Vec<u8>,
    password: &str,
) -> (
    String,
    Vec<String>,
    signature::Ed25519KeyPair,
    Vec<u8>,
    Vec<u8>,
) {
    let (username, routers, salt, key_enc, jtw_enc, file_enc) =
        bincode::deserialize::<(String, Vec<String>, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>)>(&keyfile)
            .unwrap();

    // rederive disk key
    let mut disk_key: DiskKey = [0u8; CREDENTIAL_LEN];
    pbkdf2::derive(
        PBKDF2_ALG,
        NonZeroU32::new(ITERATIONS).unwrap(),
        &salt,
        password.as_bytes(),
        &mut disk_key,
    );

    let cipher_key = Key::<Aes256Gcm>::from_slice(&disk_key);
    let cipher = Aes256Gcm::new(&cipher_key);

    let net_nonce = generic_array::GenericArray::from_slice(&key_enc[..12]);
    let jwt_nonce = generic_array::GenericArray::from_slice(&jtw_enc[..12]);
    let file_nonce = generic_array::GenericArray::from_slice(&file_enc[..12]);

    println!("decrypting saved networking key...");
    let serialized_networking_keypair: Vec<u8> = match cipher.decrypt(net_nonce, &key_enc[12..]) {
        Ok(p) => p,
        Err(e) => {
            panic!("failed to decrypt networking keys: {}", e);
        }
    };
    let networking_keypair =
        match signature::Ed25519KeyPair::from_pkcs8(&serialized_networking_keypair) {
            Ok(k) => k,
            Err(_) => panic!("failed to parse networking keys"),
        };

    // TODO: check if jwt_secret_file is valid and then proceed to unwrap and decrypt. If there is a failure, generate a new jwt_secret and save it
    // use password to decrypt jwt secret
    println!("decrypting saved jwt secret...");

    let jwt: Vec<u8> = match cipher.decrypt(jwt_nonce, &jtw_enc[12..]) {
        Ok(p) => p,
        Err(e) => {
            panic!("failed to decrypt jwt secret: {}", e);
        }
    };

    let file_key: Vec<u8> = match cipher.decrypt(file_nonce, &file_enc[12..]) {
        Ok(p) => p,
        Err(e) => {
            panic!("failed to decrypt file key: {}", e);
        }
    };

    (username, routers, networking_keypair, jwt, file_key)
}

/// # Returns
/// a pair of (public key (encoded as a hex string), serialized key as a pkcs8 Document)
pub fn generate_networking_key() -> (String, Document) {
    let seed = SystemRandom::new();
    let doc = signature::Ed25519KeyPair::generate_pkcs8(&seed).unwrap();
    let keys = signature::Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap();
    (hex::encode(keys.public_key().as_ref()), doc)
}
/// randomly generated key to encrypt file chunks, encrypted on-disk with disk_key
pub fn generate_file_key() -> Vec<u8> {
    let mut key = [0u8; 32];
    let rng = SystemRandom::new();
    rng.fill(&mut key).unwrap();
    key.to_vec()
}
