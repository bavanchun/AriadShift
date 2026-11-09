//! Network and SSRF error types.

use std::net::{IpAddr, SocketAddr};
use thiserror::Error;

use super::ip::IpCategory;

#[derive(Debug, Error)]
pub enum NetError {
    #[error("invalid URL: {0}")]
    InvalidUrl(String),

    #[error("unsupported URL scheme '{scheme}'; expected HTTPS")]
    UnsupportedScheme { scheme: String },

    #[error("HTTPS to HTTP downgrade refused for redirect target '{0}'")]
    DowngradeRefused(String),

    #[error("URL contains user credentials, which are forbidden")]
    CredentialsNotAllowed,

    #[error("port {port} is not permitted")]
    PortNotAllowed { port: u16 },

    #[error("invalid header: {0}")]
    InvalidHeader(String),

    #[error("line exceeded maximum allowed length ({limit} bytes)")]
    LineTooLong { limit: usize },

    #[error("DNS resolution failed for '{host}': {source}")]
    ResolutionFailed {
        host: String,
        source: std::io::Error,
    },

    #[error("host '{0}' resolved to no addresses")]
    HostNotFound(String),

    #[error("non-public address {ip} ({reason}) rejected for host '{host}'")]
    NonPublicAddress {
        host: String,
        ip: IpAddr,
        reason: IpCategory,
    },

    #[error("too many redirects (limit: {max})")]
    TooManyRedirects { max: u32 },

    #[error("cross-origin redirect refused from '{from}' to '{to}'")]
    CrossOriginRedirectRefused { from: String, to: String },

    #[error("invalid redirect: {0}")]
    InvalidRedirect(String),

    #[error("connection failed to {addr}: {source}")]
    ConnectFailed {
        addr: SocketAddr,
        source: std::io::Error,
    },

    #[error("TLS error: {0}")]
    Tls(String),

    #[error("invalid host for TLS SNI: {0}")]
    InvalidHost(String),

    #[error("I/O error during transfer: {0}")]
    Io(#[from] std::io::Error),

    #[error("response headers too large (exceeded {limit} bytes)")]
    HeadersTooLarge { limit: usize },

    #[error("response body too large (exceeded {limit} bytes)")]
    ResponseTooLarge { limit: usize },

    #[error("request timed out")]
    Timeout,

    #[error("invalid HTTP response: {0}")]
    InvalidResponse(String),
}
