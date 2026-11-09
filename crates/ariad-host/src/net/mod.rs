//! Reusable SSRF-safe network client with DNS pinning, all-address public-IP filtering,
//! redirect re-validation, response size and time caps, and injectable resolver/transport.

pub mod client;
pub mod error;
pub mod ip;
pub mod resolver;
pub mod transport;
pub mod url_policy;

#[cfg(test)]
mod tests;

pub use client::{Client, ClientBuilder, ClientPolicy};
pub use error::NetError;
pub use ip::{IpCategory, classify_ip, classify_ipv4, classify_ipv6, is_public_ip};
pub use resolver::{DefaultResolver, MockResolver, Resolver, resolve_and_pin_public};
pub use transport::{
    DefaultTransport, HttpRequest, HttpResponse, Method, MockHandler, MockTransport, Timeouts,
    Transport,
};
pub use url_policy::{UrlPolicy, validate_redirect, validate_url};
