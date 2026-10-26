//! Deterministic cache tests: fake monotonic time and a controllable fetcher.

use super::*;
use jsonwebtoken::{EncodingKey, Header, encode};
use serde_json::json;
use std::sync::{
    Mutex as StdMutex,
    atomic::{AtomicUsize, Ordering},
};

struct Source {
    now: Arc<StdMutex<Instant>>,
    keys: Arc<StdMutex<Option<JwkSet>>>,
    calls: Arc<AtomicUsize>,
    key: EncodingKey,
}

impl Source {
    fn new() -> Self {
        // RFC 8410 PKCS#8 wrapping of a public test seed, never a service key.
        let mut der = hex::decode("302e020100300506032b657004220420").unwrap();
        der.extend_from_slice(&[7; 32]);
        let key = EncodingKey::from_ed_der(&der);
        let mut jwk = Jwk::from_encoding_key(&key, Algorithm::EdDSA).unwrap();
        jwk.common.key_id = Some("known".to_owned());
        Self {
            now: Arc::new(StdMutex::new(Instant::now())),
            keys: Arc::new(StdMutex::new(Some(JwkSet { keys: vec![jwk] }))),
            calls: Arc::default(),
            key,
        }
    }

    fn verifier(&self) -> JwtVerifier {
        let keys = Arc::clone(&self.keys);
        let calls = Arc::clone(&self.calls);
        let now = Arc::clone(&self.now);
        JwtVerifier {
            config: JwtConfig {
                jwks_url: "https://issuer.example/jwks".parse().unwrap(),
                issuer: "https://issuer.example".to_owned(),
                audience: "ariad-api".to_owned(),
            },
            fetch: Arc::new(move || {
                calls.fetch_add(1, Ordering::SeqCst);
                let keys = keys.lock().unwrap().clone();
                Box::pin(async move { keys.ok_or(JwtError::Fetch) })
            }),
            now: Arc::new(move || *now.lock().unwrap()),
            cache: RwLock::new(None),
            last_fetch: Mutex::new(None),
        }
    }

    fn advance(&self, duration: Duration) {
        *self.now.lock().unwrap() += duration;
    }

    fn token(&self, kid: &str, issuer: &str, audience: &str, exp: i64) -> String {
        let mut header = Header::new(Algorithm::EdDSA);
        header.kid = Some(kid.to_owned());
        encode(
            &header,
            &json!({
                "sub": "alice", "iss": issuer, "aud": audience, "exp": exp,
            }),
            &self.key,
        )
        .unwrap()
    }

    fn valid_token(&self) -> String {
        self.token(
            "known",
            "https://issuer.example",
            "ariad-api",
            4_102_444_800,
        )
    }
}

#[tokio::test]
async fn failed_refresh_has_a_finite_stale_grace_even_when_retries_are_throttled() {
    let source = Source::new();
    let verifier = source.verifier();
    let token = source.valid_token();
    assert_eq!(verifier.verify(&token).await.unwrap(), "alice");
    *source.keys.lock().unwrap() = None;
    source.advance(JWKS_TTL);
    assert_eq!(verifier.verify(&token).await.unwrap(), "alice");
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
    source.advance(JWKS_MAX_AGE - JWKS_TTL - Duration::from_secs(1));
    assert_eq!(verifier.verify(&token).await.unwrap(), "alice");
    // The next lookup is inside refresh spacing. Cached presence must not
    // turn that skipped retry into permission to trust the expired key set.
    source.advance(Duration::from_secs(1));
    assert!(matches!(
        verifier.verify(&token).await,
        Err(JwtError::Fetch)
    ));
    assert_eq!(source.calls.load(Ordering::SeqCst), 3);
    source.advance(Duration::from_secs(3600));
    assert!(matches!(
        verifier.verify(&token).await,
        Err(JwtError::Fetch)
    ));
    *source.keys.lock().unwrap() = Some(JwkSet { keys: vec![] });
    source.advance(REFRESH_SPACING);
    assert!(matches!(
        verifier.verify(&token).await,
        Err(JwtError::UnknownKey)
    ));
    let mut confirmed = Jwk::from_encoding_key(&source.key, Algorithm::EdDSA).unwrap();
    confirmed.common.key_id = Some("known".to_owned());
    *source.keys.lock().unwrap() = Some(JwkSet {
        keys: vec![confirmed],
    });
    source.advance(REFRESH_SPACING);
    assert_eq!(verifier.verify(&token).await.unwrap(), "alice");
}

#[tokio::test]
async fn successful_rotation_removes_the_old_key_and_unknown_ids_are_throttled() {
    let source = Source::new();
    let verifier = source.verifier();
    let token = source.valid_token();
    assert!(verifier.verify(&token).await.is_ok());
    let mut rotated = source.keys.lock().unwrap().clone().unwrap();
    rotated.keys[0].common.key_id = Some("rotated".to_owned());
    *source.keys.lock().unwrap() = Some(rotated);
    let rotated_token = source.token(
        "rotated",
        "https://issuer.example",
        "ariad-api",
        4_102_444_800,
    );
    // Unknown ids cannot trigger requests faster than the refresh spacing.
    for _ in 0..10 {
        assert!(matches!(
            verifier.verify(&rotated_token).await,
            Err(JwtError::UnknownKey)
        ));
    }
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    source.advance(REFRESH_SPACING);
    assert!(verifier.verify(&rotated_token).await.is_ok());
    assert!(matches!(
        verifier.verify(&token).await,
        Err(JwtError::UnknownKey)
    ));
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn empty_cache_failures_use_the_short_retry_spacing() {
    let source = Source::new();
    *source.keys.lock().unwrap() = None;
    let verifier = source.verifier();
    for _ in 0..10 {
        assert!(matches!(
            verifier.verify(&source.valid_token()).await,
            Err(JwtError::Fetch)
        ));
    }
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    source.advance(RETRY_SPACING);
    assert!(matches!(
        verifier.verify(&source.valid_token()).await,
        Err(JwtError::Fetch)
    ));
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn eddsa_algorithm_issuer_audience_and_expiry_checks_are_preserved() {
    let source = Source::new();
    let verifier = source.verifier();
    assert_eq!(
        verifier.verify(&source.valid_token()).await.unwrap(),
        "alice"
    );
    let expired = chrono::Utc::now().timestamp() - 120;
    for token in [
        source.token(
            "known",
            "https://foreign.example",
            "ariad-api",
            4_102_444_800,
        ),
        source.token("known", "https://issuer.example", "foreign", 4_102_444_800),
        source.token("known", "https://issuer.example", "ariad-api", expired),
    ] {
        assert!(matches!(
            verifier.verify(&token).await,
            Err(JwtError::Invalid(_))
        ));
    }
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some("known".to_owned());
    let symmetric = encode(
        &header,
        &json!({"sub": "alice"}),
        &EncodingKey::from_secret(b"public test secret"),
    )
    .unwrap();
    assert!(matches!(
        verifier.verify(&symmetric).await,
        Err(JwtError::Invalid(_))
    ));
    let mut keys = source.keys.lock().unwrap().clone().unwrap();
    keys.keys[0].common.key_algorithm = Some(KeyAlgorithm::RS256);
    *source.keys.lock().unwrap() = Some(keys);
    source.advance(JWKS_TTL);
    assert!(matches!(
        verifier.verify(&source.valid_token()).await,
        Err(JwtError::Invalid(_))
    ));
}
