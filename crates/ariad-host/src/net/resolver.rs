//! Pluggable DNS resolver and public address validation.

use std::{
    collections::HashMap,
    fmt::Debug,
    net::{SocketAddr, ToSocketAddrs},
    sync::{Arc, Mutex},
};

use super::{
    error::NetError,
    ip::{IpCategory, classify_ip},
};

/// Trait for DNS resolvers. Injectable so tests run without network access.
pub trait Resolver: Debug + Send + Sync + 'static {
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, NetError>;
}

/// Standard resolver delegating to operating system name resolution (`ToSocketAddrs`).
#[derive(Debug, Default)]
pub struct DefaultResolver;

impl Resolver for DefaultResolver {
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, NetError> {
        let addrs = (host, port)
            .to_socket_addrs()
            .map_err(|source| NetError::ResolutionFailed {
                host: host.to_owned(),
                source,
            })?
            .collect::<Vec<_>>();

        if addrs.is_empty() {
            return Err(NetError::HostNotFound(host.to_owned()));
        }

        Ok(addrs)
    }
}

/// Mock resolver for testing without touching the network.
#[derive(Clone, Debug, Default)]
pub struct MockResolver {
    map: Arc<Mutex<HashMap<String, Vec<SocketAddr>>>>,
}

impl MockResolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a fixed list of socket addresses for a given host or host:port.
    pub fn insert(&self, host: impl Into<String>, addrs: Vec<SocketAddr>) {
        self.map.lock().unwrap().insert(host.into(), addrs);
    }
}

impl Resolver for MockResolver {
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, NetError> {
        let guard = self.map.lock().unwrap();
        let key_with_port = format!("{host}:{port}");
        if let Some(addrs) = guard.get(&key_with_port).or_else(|| guard.get(host)) {
            if addrs.is_empty() {
                return Err(NetError::HostNotFound(host.to_owned()));
            }
            return Ok(addrs.clone());
        }

        // Direct IP literal fallback if host parses as an IP address
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            return Ok(vec![SocketAddr::new(ip, port)]);
        }

        Err(NetError::HostNotFound(host.to_owned()))
    }
}

/// Resolves a host, strictly checks ALL returned addresses against public-IP rules, and
/// returns the first pinned socket address. If *any* address in the answer is non-public,
/// the entire answer is rejected (mitigating DNS rebinding / mixed-record attacks).
pub fn resolve_and_pin_public(
    resolver: &dyn Resolver,
    host: &str,
    port: u16,
    allow_loopback: bool,
) -> Result<SocketAddr, NetError> {
    let addrs = resolver.resolve(host, port)?;
    if addrs.is_empty() {
        return Err(NetError::HostNotFound(host.to_owned()));
    }

    for addr in &addrs {
        let ip = addr.ip();
        if let Err(reason) = classify_ip(ip) {
            if allow_loopback && reason == IpCategory::Loopback {
                continue;
            }
            return Err(NetError::NonPublicAddress {
                host: host.to_owned(),
                ip,
                reason,
            });
        }
    }

    // Pinned connection target
    Ok(addrs[0])
}
