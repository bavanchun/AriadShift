//! Fresh signing material for cryptographic test fixtures.

pub fn random_signing_key() -> [u8; 32] {
    let mut key = [0; 32];
    rustls::crypto::ring::default_provider()
        .secure_random
        .fill(&mut key)
        .expect("generate test signing key");
    key
}
