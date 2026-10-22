//! AriadShift cloud API and conversion workers.
#![forbid(unsafe_code)]

pub mod api;
pub mod app;
pub mod auth;
pub mod config;
pub mod db;
pub mod entitlements;
pub mod error;
pub mod packs;
pub mod queue;
pub mod rate_limit;
pub mod routing;
pub mod sandbox;
pub mod storage;
pub mod sweeper;
pub mod telemetry;
pub mod worker;

/// Selects ring as the process-wide TLS crypto provider. Safe to call more
/// than once.
pub fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// The HTTP client for outbound calls (JWKS, Turnstile).
#[must_use]
pub fn http_client() -> reqwest::Client {
    install_crypto_provider();
    reqwest::Client::builder()
        .user_agent(concat!("ariad-server/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}
