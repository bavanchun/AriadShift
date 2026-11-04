//! Transport abstraction and HTTP/1.1 engine over TCP and rustls.

use std::{
    fmt::{self, Debug},
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpStream},
    sync::Arc,
    time::{Duration, Instant},
};

use url::Url;

use super::error::NetError;

const MAX_HEADERS_BYTES: usize = 64 * 1024;
const MAX_LINE_BYTES: usize = 8 * 1024;

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

    pub fn host_header(&self) -> String {
        let host_str = match self.url.host() {
            Some(url::Host::Ipv6(ipv6)) => format!("[{ipv6}]"),
            Some(url::Host::Ipv4(ipv4)) => ipv4.to_string(),
            Some(url::Host::Domain(domain)) => domain.to_owned(),
            None => self.url.host_str().unwrap_or_default().to_owned(),
        };
        match self.url.port() {
            Some(p) => format!("{host_str}:{p}"),
            None => host_str,
        }
    }

    pub fn sni_name(&self) -> Result<rustls::pki_types::ServerName<'static>, NetError> {
        match self.url.host() {
            Some(url::Host::Ipv6(ipv6)) => Ok(rustls::pki_types::ServerName::IpAddress(
                std::net::IpAddr::V6(ipv6).into(),
            )),
            Some(url::Host::Ipv4(ipv4)) => Ok(rustls::pki_types::ServerName::IpAddress(
                std::net::IpAddr::V4(ipv4).into(),
            )),
            Some(url::Host::Domain(domain)) => {
                rustls::pki_types::ServerName::try_from(domain.to_owned())
                    .map_err(|_| NetError::InvalidHost(domain.to_owned()))
            }
            None => {
                let h = self.url.host_str().unwrap_or_default();
                rustls::pki_types::ServerName::try_from(h.to_owned())
                    .map_err(|_| NetError::InvalidHost(h.to_owned()))
            }
        }
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
        deadline: Instant,
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

/// Stream wrapper that bounds every read/write operation to the remaining deadline budget
/// and updates per-operation socket timeouts dynamically.
struct DeadlineStream<S> {
    inner: S,
    tcp: Option<TcpStream>,
    deadline: Instant,
    read_timeout: Duration,
    write_timeout: Duration,
}

impl<S> DeadlineStream<S> {
    fn new(
        inner: S,
        tcp: Option<TcpStream>,
        deadline: Instant,
        read_timeout: Duration,
        write_timeout: Duration,
    ) -> Self {
        Self {
            inner,
            tcp,
            deadline,
            read_timeout,
            write_timeout,
        }
    }

    fn check_deadline(&self) -> Result<Duration, std::io::Error> {
        let now = Instant::now();
        if now >= self.deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "deadline exceeded",
            ));
        }
        Ok(self.deadline.duration_since(now))
    }
}

impl<S: Read> Read for DeadlineStream<S> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let remaining = self.check_deadline()?;
        if let Some(ref tcp) = self.tcp {
            let timeout = self.read_timeout.min(remaining);
            let _ = tcp.set_read_timeout(Some(timeout));
        }
        match self.inner.read(buf) {
            Ok(n) => Ok(n),
            Err(e)
                if e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock =>
            {
                if Instant::now() >= self.deadline {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "deadline exceeded",
                    ))
                } else {
                    Err(e)
                }
            }
            Err(e) => Err(e),
        }
    }
}

impl<S: Write> Write for DeadlineStream<S> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let remaining = self.check_deadline()?;
        if let Some(ref tcp) = self.tcp {
            let timeout = self.write_timeout.min(remaining);
            let _ = tcp.set_write_timeout(Some(timeout));
        }
        match self.inner.write(buf) {
            Ok(n) => Ok(n),
            Err(e)
                if e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock =>
            {
                if Instant::now() >= self.deadline {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "deadline exceeded",
                    ))
                } else {
                    Err(e)
                }
            }
            Err(e) => Err(e),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let _ = self.check_deadline()?;
        self.inner.flush()
    }
}

impl Transport for DefaultTransport {
    fn execute(
        &self,
        request: &HttpRequest,
        pinned_addr: SocketAddr,
        timeouts: &Timeouts,
        max_response_bytes: usize,
        deadline: Instant,
    ) -> Result<HttpResponse, NetError> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(NetError::Timeout);
        }
        let connect_timeout = timeouts.connect.min(remaining);

        // TCP connection directly to the pinned IP address (no host re-resolution)
        let tcp = TcpStream::connect_timeout(&pinned_addr, connect_timeout).map_err(|source| {
            NetError::ConnectFailed {
                addr: pinned_addr,
                source,
            }
        })?;

        let tcp_clone = tcp.try_clone().ok();
        let _ = tcp.set_nodelay(true);

        let result = if request.url.scheme() == "https" {
            let server_name = request.sni_name()?;

            let conn = rustls::ClientConnection::new(Arc::clone(&self.tls_config), server_name)
                .map_err(|e| NetError::Tls(e.to_string()))?;

            let tls_stream = rustls::StreamOwned::new(conn, tcp);
            let mut stream = DeadlineStream::new(
                tls_stream,
                tcp_clone,
                deadline,
                timeouts.read,
                timeouts.write,
            );
            send_and_receive(&mut stream, request, max_response_bytes, deadline)
        } else {
            let mut stream =
                DeadlineStream::new(tcp, tcp_clone, deadline, timeouts.read, timeouts.write);
            send_and_receive(&mut stream, request, max_response_bytes, deadline)
        };

        result.map_err(|err| match err {
            NetError::Io(ref io_err) if request.url.scheme() == "https" => {
                let msg = io_err.to_string();
                if msg.contains("cert")
                    || msg.contains("peer")
                    || msg.contains("handshake")
                    || msg.contains("tls")
                {
                    NetError::Tls(msg)
                } else {
                    err
                }
            }
            other => other,
        })
    }
}

fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    buf: &mut String,
    max_line_bytes: usize,
    deadline: Instant,
) -> Result<usize, NetError> {
    buf.clear();
    let mut total = 0;
    loop {
        if Instant::now() >= deadline {
            return Err(NetError::Timeout);
        }
        let available = match reader.fill_buf() {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e)
                if e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock =>
            {
                return Err(NetError::Timeout);
            }
            Err(e) => return Err(NetError::Io(e)),
        };
        if available.is_empty() {
            break; // EOF
        }
        let (found_newline, take_len) = match available.iter().position(|&b| b == b'\n') {
            Some(pos) => (true, pos + 1),
            None => (false, available.len()),
        };
        if total + take_len > max_line_bytes {
            return Err(NetError::LineTooLong {
                limit: max_line_bytes,
            });
        }
        let s = std::str::from_utf8(&available[..take_len]).map_err(|_| {
            NetError::InvalidResponse("invalid UTF-8 in HTTP header or status line".to_owned())
        })?;
        buf.push_str(s);
        total += take_len;
        reader.consume(take_len);
        if found_newline {
            break;
        }
    }
    Ok(total)
}

fn send_and_receive<S: Read + Write>(
    stream: &mut S,
    request: &HttpRequest,
    max_response_bytes: usize,
    deadline: Instant,
) -> Result<HttpResponse, NetError> {
    // Validate headers for CR or LF injection
    for (k, v) in &request.headers {
        if k.bytes().any(|b| b == b'\r' || b == b'\n')
            || v.bytes().any(|b| b == b'\r' || b == b'\n')
        {
            return Err(NetError::InvalidHeader(format!("{k}: {v}")));
        }
    }

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

    let host_header = request.host_header();

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
    let n = read_bounded_line(&mut reader, &mut status_line, MAX_LINE_BYTES, deadline)?;
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
        let bytes_read = read_bounded_line(&mut reader, &mut line, MAX_LINE_BYTES, deadline)?;
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
            read_bounded_line(&mut reader, &mut size_line, 128, deadline)?;
            let size_str = size_line.trim().split(';').next().unwrap_or("0");
            let chunk_size = usize::from_str_radix(size_str.trim(), 16).map_err(|e| {
                NetError::InvalidResponse(format!("invalid chunk size '{size_str}': {e}"))
            })?;

            if chunk_size == 0 {
                // Read trailing CRLF / trailers until empty line
                let mut trailer = String::new();
                read_bounded_line(&mut reader, &mut trailer, MAX_LINE_BYTES, deadline)?;
                break;
            }

            if chunk_size > max_response_bytes
                || chunks
                    .len()
                    .checked_add(chunk_size)
                    .is_none_or(|new_len| new_len > max_response_bytes)
            {
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
        deadline: Instant,
    ) -> Result<HttpResponse, NetError> {
        if Instant::now() >= deadline {
            return Err(NetError::Timeout);
        }
        let resp = (self.handler)(request, pinned_addr)?;
        if resp.body.len() > max_response_bytes {
            return Err(NetError::ResponseTooLarge {
                limit: max_response_bytes,
            });
        }
        Ok(resp)
    }
}
