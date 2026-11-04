//! Transport abstraction and HTTP/1.1 engine over TCP and rustls.

use std::{
    fmt::{self, Debug},
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpStream},
    sync::Arc,
    time::Duration,
};

use url::Url;

use super::error::NetError;

const MAX_HEADERS_BYTES: usize = 64 * 1024;

/// HTTP request method.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Method {
    Get,
    Post,
    Head,
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Get => write!(f, "GET"),
            Self::Post => write!(f, "POST"),
            Self::Head => write!(f, "HEAD"),
        }
    }
}

/// HTTP request structure for outbound SSRF-safe requests.
#[derive(Clone, Debug)]
pub struct HttpRequest {
    pub method: Method,
    pub url: Url,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

impl HttpRequest {
    pub fn new(method: Method, url: Url) -> Self {
        Self {
            method,
            url,
            headers: Vec::new(),
            body: None,
        }
    }

    pub fn get(url: Url) -> Self {
        Self::new(Method::Get, url)
    }

    pub fn post(url: Url, body: Vec<u8>, content_type: &str) -> Self {
        let mut req = Self::new(Method::Post, url);
        req.headers
            .push(("content-type".to_owned(), content_type.to_owned()));
        req.body = Some(body);
        req
    }

    pub fn host(&self) -> &str {
        self.url.host_str().unwrap_or_default()
    }

    pub fn port(&self) -> u16 {
        self.url
            .port_or_known_default()
            .unwrap_or(if self.url.scheme() == "https" {
                443
            } else {
                80
            })
    }
}

/// HTTP response received from a verified server.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        let name_lower = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| k.to_ascii_lowercase() == name_lower)
            .map(|(_, v)| v.as_str())
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn is_redirect(&self) -> bool {
        [301, 302, 303, 307, 308].contains(&self.status)
    }
}

/// Timeouts for connection setup, socket reads, and socket writes.
#[derive(Clone, Copy, Debug)]
pub struct Timeouts {
    pub connect: Duration,
    pub read: Duration,
    pub write: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(10),
            read: Duration::from_secs(15),
            write: Duration::from_secs(10),
        }
    }
}

/// Pluggable transport trait executing an HTTP request against a pinned IP.
pub trait Transport: Debug + Send + Sync + 'static {
    fn execute(
        &self,
        request: &HttpRequest,
        pinned_addr: SocketAddr,
        timeouts: &Timeouts,
        max_response_bytes: usize,
    ) -> Result<HttpResponse, NetError>;
}

/// Standard transport connecting over TCP and rustls.
pub struct DefaultTransport {
    tls_config: Arc<rustls::ClientConfig>,
}

impl Debug for DefaultTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DefaultTransport").finish_non_exhaustive()
    }
}

impl DefaultTransport {
    /// Creates a new default transport with operating system root certificates.
    pub fn new() -> Result<Self, NetError> {
        let loaded = rustls_native_certs::load_native_certs();
        let mut root_store = rustls::RootCertStore::empty();
        for cert in loaded.certs {
            let _ = root_store.add(cert);
        }
        Self::with_root_store(Arc::new(root_store))
    }

    /// Creates a transport with an explicit root certificate store (useful for tests and custom trust anchors).
    pub fn with_root_store(root_store: Arc<rustls::RootCertStore>) -> Result<Self, NetError> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let tls_config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| NetError::Tls(e.to_string()))?
            .with_root_certificates(root_store)
            .with_no_client_auth();

        Ok(Self {
            tls_config: Arc::new(tls_config),
        })
    }
}

impl Transport for DefaultTransport {
    fn execute(
        &self,
        request: &HttpRequest,
        pinned_addr: SocketAddr,
        timeouts: &Timeouts,
        max_response_bytes: usize,
    ) -> Result<HttpResponse, NetError> {
        // TCP connection directly to the pinned IP address (no host re-resolution)
        let tcp = TcpStream::connect_timeout(&pinned_addr, timeouts.connect).map_err(|source| {
            NetError::ConnectFailed {
                addr: pinned_addr,
                source,
            }
        })?;

        let _ = tcp.set_read_timeout(Some(timeouts.read));
        let _ = tcp.set_write_timeout(Some(timeouts.write));
        let _ = tcp.set_nodelay(true);

        if request.url.scheme() == "https" {
            let host_str = request.host();
            let server_name = rustls::pki_types::ServerName::try_from(host_str.to_owned())
                .map_err(|_| NetError::InvalidHost(host_str.to_owned()))?;

            let conn = rustls::ClientConnection::new(Arc::clone(&self.tls_config), server_name)
                .map_err(|e| NetError::Tls(e.to_string()))?;

            let mut tls_stream = rustls::StreamOwned::new(conn, tcp);
            send_and_receive(&mut tls_stream, request, max_response_bytes)
        } else {
            let mut stream = tcp;
            send_and_receive(&mut stream, request, max_response_bytes)
        }
    }
}

fn send_and_receive<S: Read + Write>(
    stream: &mut S,
    request: &HttpRequest,
    max_response_bytes: usize,
) -> Result<HttpResponse, NetError> {
    // 1. Build and send request
    let path_and_query = match request.url.query() {
        Some(q) => format!("{}?{}", request.url.path(), q),
        None => request.url.path().to_owned(),
    };
    let path_and_query = if path_and_query.is_empty() {
        "/".to_owned()
    } else {
        path_and_query
    };

    let host_header = match request.url.port() {
        Some(p) => format!("{}:{}", request.host(), p),
        None => request.host().to_owned(),
    };

    let mut header_buf = Vec::new();
    write!(
        header_buf,
        "{} {} HTTP/1.1\r\n",
        request.method, path_and_query
    )?;
    write!(header_buf, "Host: {}\r\n", host_header)?;
    write!(
        header_buf,
        "User-Agent: ashift/{}\r\n",
        env!("CARGO_PKG_VERSION")
    )?;
    write!(header_buf, "Connection: close\r\n")?;

    for (k, v) in &request.headers {
        if !k.eq_ignore_ascii_case("host")
            && !k.eq_ignore_ascii_case("connection")
            && !k.eq_ignore_ascii_case("content-length")
        {
            write!(header_buf, "{}: {}\r\n", k, v)?;
        }
    }

    if let Some(body) = &request.body {
        write!(header_buf, "Content-Length: {}\r\n", body.len())?;
    }
    write!(header_buf, "\r\n")?;

    stream.write_all(&header_buf)?;
    if let Some(body) = &request.body {
        stream.write_all(body)?;
    }
    stream.flush()?;

    // 2. Read response
    let mut reader = BufReader::new(stream);
    let mut total_header_bytes = 0;

    let mut status_line = String::new();
    let n = reader.read_line(&mut status_line)?;
    if n == 0 {
        return Err(NetError::InvalidResponse("empty response".to_owned()));
    }
    total_header_bytes += n;

    let parts = status_line.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 2 {
        return Err(NetError::InvalidResponse(format!(
            "malformed status line: {status_line}"
        )));
    }
    let status = parts[1]
        .parse::<u16>()
        .map_err(|_| NetError::InvalidResponse(format!("invalid status code: {}", parts[1])))?;

    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            break;
        }
        total_header_bytes += bytes_read;
        if total_header_bytes > MAX_HEADERS_BYTES {
            return Err(NetError::HeadersTooLarge {
                limit: MAX_HEADERS_BYTES,
            });
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_owned()));
        }
    }

    // 3. Read body
    let is_chunked = headers
        .iter()
        .any(|(k, v)| k == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked"));
    let content_length = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse::<usize>().ok());

    let body = if is_chunked {
        let mut chunks = Vec::new();
        loop {
            let mut size_line = String::new();
            reader.read_line(&mut size_line)?;
            let size_str = size_line.trim().split(';').next().unwrap_or("0");
            let chunk_size = usize::from_str_radix(size_str.trim(), 16).map_err(|e| {
                NetError::InvalidResponse(format!("invalid chunk size '{size_str}': {e}"))
            })?;

            if chunk_size == 0 {
                // Read trailing CRLF / trailers until empty line
                let mut trailer = String::new();
                reader.read_line(&mut trailer)?;
                break;
            }

            if chunks.len() + chunk_size > max_response_bytes {
                return Err(NetError::ResponseTooLarge {
                    limit: max_response_bytes,
                });
            }

            let start = chunks.len();
            chunks.resize(start + chunk_size, 0);
            reader.read_exact(&mut chunks[start..])?;

            let mut crlf = [0u8; 2];
            reader.read_exact(&mut crlf)?;
        }
        chunks
    } else if let Some(len) = content_length {
        if len > max_response_bytes {
            return Err(NetError::ResponseTooLarge {
                limit: max_response_bytes,
            });
        }
        let mut buf = vec![0u8; len];
        reader.read_exact(&mut buf)?;
        buf
    } else {
        let mut buf = Vec::new();
        let mut take = (&mut reader).take((max_response_bytes + 1) as u64);
        take.read_to_end(&mut buf)?;
        if buf.len() > max_response_bytes {
            return Err(NetError::ResponseTooLarge {
                limit: max_response_bytes,
            });
        }
        buf
    };

    Ok(HttpResponse {
        status,
        headers,
        body,
    })
}

/// Handler function type for `MockTransport`.
pub type MockHandler =
    Arc<dyn Fn(&HttpRequest, SocketAddr) -> Result<HttpResponse, NetError> + Send + Sync>;

/// Mock transport for unit tests without network access.
#[derive(Clone)]
pub struct MockTransport {
    handler: MockHandler,
}

impl Debug for MockTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MockTransport").finish_non_exhaustive()
    }
}

impl MockTransport {
    pub fn new(
        handler: impl Fn(&HttpRequest, SocketAddr) -> Result<HttpResponse, NetError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            handler: Arc::new(handler),
        }
    }

    pub fn fixed(response: HttpResponse) -> Self {
        Self::new(move |_, _| Ok(response.clone()))
    }
}

impl Transport for MockTransport {
    fn execute(
        &self,
        request: &HttpRequest,
        pinned_addr: SocketAddr,
        _timeouts: &Timeouts,
        max_response_bytes: usize,
    ) -> Result<HttpResponse, NetError> {
        let resp = (self.handler)(request, pinned_addr)?;
        if resp.body.len() > max_response_bytes {
            return Err(NetError::ResponseTooLarge {
                limit: max_response_bytes,
            });
        }
        Ok(resp)
    }
}
