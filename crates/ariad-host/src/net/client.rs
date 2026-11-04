//! High-level SSRF-safe HTTP client with redirect loop, time & size caps, and DNS pinning.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use url::Url;

use super::{
    error::NetError,
    resolver::{DefaultResolver, Resolver, resolve_and_pin_public},
    transport::{DefaultTransport, HttpRequest, HttpResponse, Method, Timeouts, Transport},
    url_policy::{UrlPolicy, validate_redirect, validate_url},
};

/// Configuration policy for the SSRF-safe network client.
#[derive(Clone, Debug)]
pub struct ClientPolicy {
    /// Maximum number of redirects to follow before failing.
    pub max_redirects: u32,
    /// Maximum response payload size in bytes.
    pub max_response_bytes: usize,
    /// Connection setup timeout.
    pub connect_timeout: Duration,
    /// Socket read timeout.
    pub read_timeout: Duration,
    /// Socket write timeout.
    pub write_timeout: Duration,
    /// Overall deadline for the entire request including all redirects.
    pub total_timeout: Duration,
    /// Enforce HTTPS-only requests.
    pub https_only: bool,
    /// Allow loopback addresses (for internal integration tests only).
    pub allow_loopback: bool,
    /// Allowed ports beyond default 443 (HTTPS) and 80 (HTTP).
    pub allowed_ports: Vec<u16>,
}

impl Default for ClientPolicy {
    fn default() -> Self {
        Self {
            max_redirects: 5,
            max_response_bytes: 10 * 1024 * 1024, // 10 MiB
            connect_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(15),
            write_timeout: Duration::from_secs(10),
            total_timeout: Duration::from_secs(30),
            https_only: true,
            allow_loopback: false,
            allowed_ports: Vec::new(),
        }
    }
}

/// SSRF-safe HTTP client providing pinned-IP connections, public-IP enforcement,
/// redirect validation, and time/size caps.
#[derive(Clone, Debug)]
pub struct Client {
    policy: ClientPolicy,
    resolver: Arc<dyn Resolver>,
    transport: Arc<dyn Transport>,
}

impl Client {
    /// Creates a default SSRF-safe HTTPS client.
    pub fn new() -> Result<Self, NetError> {
        Self::builder().build()
    }

    /// Creates a builder for custom client configuration.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// Access the client policy.
    pub fn policy(&self) -> &ClientPolicy {
        &self.policy
    }

    /// Executes an HTTP GET request to `url`.
    pub fn get(&self, url: Url) -> Result<HttpResponse, NetError> {
        self.send(HttpRequest::get(url))
    }

    /// Executes an HTTP POST request to `url` with `body` and `content_type`.
    pub fn post(
        &self,
        url: Url,
        body: Vec<u8>,
        content_type: &str,
    ) -> Result<HttpResponse, NetError> {
        self.send(HttpRequest::post(url, body, content_type))
    }

    /// Executes an SSRF-safe HTTP request with redirect following and DNS pinning.
    pub fn send(&self, mut request: HttpRequest) -> Result<HttpResponse, NetError> {
        let start = Instant::now();
        let url_policy = UrlPolicy {
            https_only: self.policy.https_only,
            allow_loopback: self.policy.allow_loopback,
            allowed_ports: self.policy.allowed_ports.clone(),
        };

        let mut redirect_count = 0;

        loop {
            // 1. Overall deadline check
            let elapsed = start.elapsed();
            if elapsed >= self.policy.total_timeout {
                return Err(NetError::Timeout);
            }

            // 2. Validate URL according to policy
            validate_url(&request.url, &url_policy)?;

            // 3. Resolve host and pin connection IP (refusing any non-public IPs)
            let pinned_addr = resolve_and_pin_public(
                &*self.resolver,
                request.host(),
                request.port(),
                self.policy.allow_loopback,
            )?;

            // 4. Compute remaining time budget
            let remaining = self.policy.total_timeout.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                return Err(NetError::Timeout);
            }

            let timeouts = Timeouts {
                connect: self.policy.connect_timeout.min(remaining),
                read: self.policy.read_timeout.min(remaining),
                write: self.policy.write_timeout.min(remaining),
            };

            let deadline = start + self.policy.total_timeout;

            // 5. Execute request against the pinned socket address
            let response = self.transport.execute(
                &request,
                pinned_addr,
                &timeouts,
                self.policy.max_response_bytes,
                deadline,
            )?;

            if start.elapsed() >= self.policy.total_timeout {
                return Err(NetError::Timeout);
            }

            // 6. Handle redirects
            if response.is_redirect() {
                if redirect_count >= self.policy.max_redirects {
                    return Err(NetError::TooManyRedirects {
                        max: self.policy.max_redirects,
                    });
                }

                let location = response.header("location").ok_or_else(|| {
                    NetError::InvalidRedirect("missing Location header".to_owned())
                })?;

                let next_url = validate_redirect(&request.url, location, &url_policy)?;

                // RFC 7231 / RFC 9110 redirect method rewrite semantics:
                match response.status {
                    303 => {
                        // See Other: always switch to GET with empty body
                        request.method = Method::Get;
                        request.body = None;
                        request.headers.retain(|(k, _)| {
                            !k.eq_ignore_ascii_case("content-type")
                                && !k.eq_ignore_ascii_case("content-length")
                        });
                    }
                    301 | 302 => {
                        // Moved Permanently / Found: POST switches to GET
                        if request.method == Method::Post {
                            request.method = Method::Get;
                            request.body = None;
                            request.headers.retain(|(k, _)| {
                                !k.eq_ignore_ascii_case("content-type")
                                    && !k.eq_ignore_ascii_case("content-length")
                            });
                        }
                    }
                    307 | 308 => {
                        // Temporary / Permanent Redirect: preserve method and body
                    }
                    _ => {}
                }

                let is_cross_origin = request.url.origin() != next_url.origin();
                if is_cross_origin {
                    request.headers.retain(|(k, _)| {
                        matches!(
                            k.to_ascii_lowercase().as_str(),
                            "accept" | "accept-language" | "user-agent"
                        )
                    });
                }

                request.url = next_url;
                redirect_count += 1;
                continue;
            }

            return Ok(response);
        }
    }
}

/// Builder for constructing a configured SSRF-safe `Client`.
#[derive(Debug, Default)]
pub struct ClientBuilder {
    policy: ClientPolicy,
    resolver: Option<Arc<dyn Resolver>>,
    transport: Option<Arc<dyn Transport>>,
}

impl ClientBuilder {
    pub fn policy(mut self, policy: ClientPolicy) -> Self {
        self.policy = policy;
        self
    }

    pub fn max_redirects(mut self, max: u32) -> Self {
        self.policy.max_redirects = max;
        self
    }

    pub fn max_response_bytes(mut self, bytes: usize) -> Self {
        self.policy.max_response_bytes = bytes;
        self
    }

    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.policy.connect_timeout = timeout;
        self
    }

    pub fn read_timeout(mut self, timeout: Duration) -> Self {
        self.policy.read_timeout = timeout;
        self
    }

    pub fn total_timeout(mut self, timeout: Duration) -> Self {
        self.policy.total_timeout = timeout;
        self
    }

    pub fn https_only(mut self, https_only: bool) -> Self {
        self.policy.https_only = https_only;
        self
    }

    pub fn allow_loopback(mut self, allow: bool) -> Self {
        self.policy.allow_loopback = allow;
        self
    }

    pub fn allowed_ports(mut self, ports: Vec<u16>) -> Self {
        self.policy.allowed_ports = ports;
        self
    }

    pub fn resolver(mut self, resolver: Arc<dyn Resolver>) -> Self {
        self.resolver = Some(resolver);
        self
    }

    pub fn transport(mut self, transport: Arc<dyn Transport>) -> Self {
        self.transport = Some(transport);
        self
    }

    pub fn build(self) -> Result<Client, NetError> {
        let resolver = self.resolver.unwrap_or_else(|| Arc::new(DefaultResolver));

        let transport = match self.transport {
            Some(t) => t,
            None => Arc::new(DefaultTransport::new()?),
        };

        Ok(Client {
            policy: self.policy,
            resolver,
            transport,
        })
    }
}
