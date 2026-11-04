use std::{
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener},
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};

use url::Url;

use super::{
    client::Client,
    error::NetError,
    ip::{IpCategory, classify_ip, classify_ipv4, classify_ipv6, is_public_ip},
    resolver::{MockResolver, Resolver, resolve_and_pin_public},
    transport::{
        DefaultTransport, HttpRequest, HttpResponse, Method, MockTransport, Timeouts, Transport,
    },
    url_policy::{UrlPolicy, validate_redirect, validate_url},
};

#[test]
fn test_ipv4_classification() {
    // Loopback
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(127, 0, 0, 1)),
        Err(IpCategory::Loopback)
    );
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(127, 255, 255, 254)),
        Err(IpCategory::Loopback)
    );

    // Unspecified
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(0, 0, 0, 0)),
        Err(IpCategory::Unspecified)
    );

    // Private RFC 1918
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(10, 0, 0, 1)),
        Err(IpCategory::Private)
    );
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(172, 16, 0, 1)),
        Err(IpCategory::Private)
    );
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(172, 31, 255, 254)),
        Err(IpCategory::Private)
    );
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(192, 168, 1, 1)),
        Err(IpCategory::Private)
    );

    // Link-local and cloud metadata (169.254.169.254, 168.63.129.16)
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(169, 254, 1, 1)),
        Err(IpCategory::LinkLocal)
    );
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(169, 254, 169, 254)),
        Err(IpCategory::CloudMetadata)
    );
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(168, 63, 129, 16)),
        Err(IpCategory::CloudMetadata)
    );

    // Reserved (AS112-v4, AMT)
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(192, 31, 196, 1)),
        Err(IpCategory::Reserved)
    );
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(192, 52, 193, 1)),
        Err(IpCategory::Reserved)
    );

    // CGNAT / Shared Address Space
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(100, 64, 0, 1)),
        Err(IpCategory::SharedAddressSpace)
    );
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(100, 127, 255, 254)),
        Err(IpCategory::SharedAddressSpace)
    );

    // Documentation
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(192, 0, 2, 1)),
        Err(IpCategory::Documentation)
    );
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(198, 51, 100, 1)),
        Err(IpCategory::Documentation)
    );
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(203, 0, 113, 1)),
        Err(IpCategory::Documentation)
    );

    // Benchmarking
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(198, 18, 0, 1)),
        Err(IpCategory::Benchmarking)
    );
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(198, 19, 0, 1)),
        Err(IpCategory::Benchmarking)
    );

    // Multicast and Broadcast / Reserved
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(224, 0, 0, 1)),
        Err(IpCategory::Multicast)
    );
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(239, 255, 255, 250)),
        Err(IpCategory::Multicast)
    );
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(240, 0, 0, 1)),
        Err(IpCategory::Reserved)
    );
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(255, 255, 255, 255)),
        Err(IpCategory::Reserved)
    );

    // Public IPv4 addresses
    assert!(classify_ipv4(Ipv4Addr::new(93, 184, 216, 34)).is_ok());
    assert!(classify_ipv4(Ipv4Addr::new(8, 8, 8, 8)).is_ok());
    assert!(classify_ipv4(Ipv4Addr::new(1, 1, 1, 1)).is_ok());
    assert!(classify_ipv4(Ipv4Addr::new(142, 250, 190, 46)).is_ok());
}

#[test]
fn test_ipv6_classification() {
    // Loopback & Unspecified
    assert_eq!(
        classify_ipv6(Ipv6Addr::LOCALHOST),
        Err(IpCategory::Loopback)
    );
    assert_eq!(
        classify_ipv6(Ipv6Addr::UNSPECIFIED),
        Err(IpCategory::Unspecified)
    );

    // ULA and AWS/cloud IMDS IPv6 (fd00:ec2::254)
    assert_eq!(
        classify_ipv6(Ipv6Addr::from_str("fd00:ec2::254").unwrap()),
        Err(IpCategory::CloudMetadata)
    );
    assert_eq!(
        classify_ipv6(Ipv6Addr::from_str("fc00::1").unwrap()),
        Err(IpCategory::UniqueLocal)
    );
    assert_eq!(
        classify_ipv6(Ipv6Addr::from_str("fd12:3456:789a::1").unwrap()),
        Err(IpCategory::UniqueLocal)
    );

    // Link-local
    assert_eq!(
        classify_ipv6(Ipv6Addr::from_str("fe80::1").unwrap()),
        Err(IpCategory::LinkLocal)
    );

    // Multicast
    assert_eq!(
        classify_ipv6(Ipv6Addr::from_str("ff02::1").unwrap()),
        Err(IpCategory::Multicast)
    );

    // Documentation and benchmarking / reserved
    assert_eq!(
        classify_ipv6(Ipv6Addr::from_str("2001:db8::1").unwrap()),
        Err(IpCategory::Documentation)
    );
    assert_eq!(
        classify_ipv6(Ipv6Addr::from_str("2001:2::1").unwrap()),
        Err(IpCategory::Benchmarking)
    );
    assert_eq!(
        classify_ipv6(Ipv6Addr::from_str("2001:10::1").unwrap()),
        Err(IpCategory::Reserved)
    );
    assert_eq!(
        classify_ipv6(Ipv6Addr::from_str("2001:20::1").unwrap()),
        Err(IpCategory::Reserved)
    );

    // NAT64 well-known prefix embedding private/loopback/metadata IPv4
    assert_eq!(
        classify_ipv6(Ipv6Addr::from_str("64:ff9b::127.0.0.1").unwrap()),
        Err(IpCategory::Nat64NonPublic)
    );
    assert_eq!(
        classify_ipv6(Ipv6Addr::from_str("64:ff9b::10.0.0.1").unwrap()),
        Err(IpCategory::Nat64NonPublic)
    );
    assert_eq!(
        classify_ipv6(Ipv6Addr::from_str("64:ff9b::169.254.169.254").unwrap()),
        Err(IpCategory::Nat64NonPublic)
    );

    // NAT64 local-use
    assert_eq!(
        classify_ipv6(Ipv6Addr::from_str("64:ff9b:1::1").unwrap()),
        Err(IpCategory::Nat64NonPublic)
    );

    // IPv4-mapped IPv6
    assert_eq!(
        classify_ipv6(Ipv6Addr::from_str("::ffff:127.0.0.1").unwrap()),
        Err(IpCategory::Ipv4MappedNonPublic)
    );
    assert_eq!(
        classify_ipv6(Ipv6Addr::from_str("::ffff:169.254.169.254").unwrap()),
        Err(IpCategory::Ipv4MappedNonPublic)
    );

    // 6to4 embedding loopback
    assert_eq!(
        classify_ipv6(Ipv6Addr::from_str("2002:7f00:0001::1").unwrap()),
        Err(IpCategory::Reserved)
    );

    // Public global unicast IPv6
    assert!(
        classify_ipv6(Ipv6Addr::from_str("2606:2800:220:1:248:1893:25c8:1946").unwrap()).is_ok()
    );
    assert!(classify_ipv6(Ipv6Addr::from_str("2001:4860:4860::8888").unwrap()).is_ok());
}

#[test]
fn test_url_validation_rules() {
    let policy = UrlPolicy::default();

    // Valid HTTPS URL
    let url = Url::parse("https://example.com/path?foo=bar").unwrap();
    assert!(validate_url(&url, &policy).is_ok());

    // HTTP rejected under https_only
    let url = Url::parse("http://example.com/path").unwrap();
    assert!(matches!(
        validate_url(&url, &policy),
        Err(NetError::UnsupportedScheme { .. })
    ));

    // Arbitrary schemes rejected
    for bad_scheme in &[
        "file:///etc/passwd",
        "gopher://example.com",
        "ftp://example.com",
    ] {
        let url = Url::parse(bad_scheme).unwrap();
        assert!(matches!(
            validate_url(&url, &policy),
            Err(NetError::UnsupportedScheme { .. })
        ));
    }

    // Credentials rejected
    let url = Url::parse("https://user:pass@example.com/secret").unwrap();
    assert!(matches!(
        validate_url(&url, &policy),
        Err(NetError::CredentialsNotAllowed)
    ));
}

#[test]
fn test_redirect_validation_rules() {
    let policy = UrlPolicy::default();
    let current = Url::parse("https://example.com/initial").unwrap();

    // Relative redirect
    let next = validate_redirect(&current, "/sub/page", &policy).unwrap();
    assert_eq!(next.as_str(), "https://example.com/sub/page");

    // Cross-domain HTTPS redirect
    let next = validate_redirect(&current, "https://other.com/api", &policy).unwrap();
    assert_eq!(next.as_str(), "https://other.com/api");

    // HTTPS -> HTTP downgrade refused!
    let err = validate_redirect(&current, "http://example.com/insecure", &policy);
    assert!(matches!(err, Err(NetError::DowngradeRefused(_))));

    // Empty location header refused
    assert!(matches!(
        validate_redirect(&current, "", &policy),
        Err(NetError::InvalidRedirect(_))
    ));
}

#[test]
fn test_resolve_and_pin_public_rejects_mixed_or_private() {
    let resolver = MockResolver::new();

    // Host resolving to private IP rejected
    resolver.insert(
        "internal.corp",
        vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 443)],
    );
    let timeout = Duration::from_secs(5);
    let err = resolve_and_pin_public(&resolver, "internal.corp", 443, false, timeout);
    assert!(matches!(err, Err(NetError::NonPublicAddress { .. })));

    // Host resolving to metadata IP rejected
    resolver.insert(
        "metadata.aws",
        vec![SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254)),
            443,
        )],
    );
    let err = resolve_and_pin_public(&resolver, "metadata.aws", 443, false, timeout);
    assert!(matches!(err, Err(NetError::NonPublicAddress { .. })));

    // Host resolving to mixed public + private IP: WHOLE ANSWER REJECTED
    resolver.insert(
        "mixed.attacker.com",
        vec![
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)), 443), // public
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 443),     // loopback
        ],
    );
    let err = resolve_and_pin_public(&resolver, "mixed.attacker.com", 443, false, timeout);
    assert!(matches!(err, Err(NetError::NonPublicAddress { .. })));

    // Pure public IP resolves and pins
    resolver.insert(
        "safe.example.com",
        vec![SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)),
            443,
        )],
    );
    let pinned =
        resolve_and_pin_public(&resolver, "safe.example.com", 443, false, timeout).unwrap();
    assert_eq!(pinned.ip(), IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)));
}

#[test]
fn test_client_mock_end_to_end() {
    let resolver = Arc::new(MockResolver::new());
    resolver.insert(
        "api.example.com",
        vec![SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)),
            443,
        )],
    );

    let transport = Arc::new(MockTransport::fixed(HttpResponse {
        status: 200,
        headers: vec![("content-type".to_owned(), "text/plain".to_owned())],
        body: b"hello public world".to_vec(),
    }));

    let client = Client::builder()
        .resolver(resolver)
        .transport(transport)
        .build()
        .unwrap();

    let resp = client
        .get(Url::parse("https://api.example.com/test").unwrap())
        .unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body, b"hello public world");
}

#[test]
fn test_client_redirect_to_private_ip_is_blocked() {
    let resolver = Arc::new(MockResolver::new());
    // Initial public host
    resolver.insert(
        "start.example.com",
        vec![SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)),
            443,
        )],
    );
    // Attacker redirect target host
    resolver.insert(
        "internal.attacker.com",
        vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)), 443)],
    );

    let transport = Arc::new(MockTransport::new(|req, _| {
        if req.host() == "start.example.com" {
            Ok(HttpResponse {
                status: 302,
                headers: vec![(
                    "location".to_owned(),
                    "https://internal.attacker.com/admin".to_owned(),
                )],
                body: Vec::new(),
            })
        } else {
            panic!("transport should never be called for non-public IP");
        }
    }));

    let client = Client::builder()
        .resolver(resolver)
        .transport(transport)
        .build()
        .unwrap();

    let err = client
        .get(Url::parse("https://start.example.com/entry").unwrap())
        .unwrap_err();

    assert!(matches!(err, NetError::NonPublicAddress { .. }));
}

#[test]
fn test_client_redirect_loop_capped() {
    let resolver = Arc::new(MockResolver::new());
    resolver.insert(
        "loop.example.com",
        vec![SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)),
            443,
        )],
    );

    let transport = Arc::new(MockTransport::new(|_, _| {
        Ok(HttpResponse {
            status: 302,
            headers: vec![(
                "location".to_owned(),
                "https://loop.example.com/".to_owned(),
            )],
            body: Vec::new(),
        })
    }));

    let client = Client::builder()
        .resolver(resolver)
        .transport(transport)
        .max_redirects(3)
        .build()
        .unwrap();

    let err = client
        .get(Url::parse("https://loop.example.com/start").unwrap())
        .unwrap_err();

    assert!(matches!(err, NetError::TooManyRedirects { max: 3 }));
}

#[test]
fn test_client_post_and_redirect_rewrites() {
    let resolver = Arc::new(MockResolver::new());
    resolver.insert(
        "post.example.com",
        vec![SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)),
            443,
        )],
    );

    let transport = Arc::new(MockTransport::new(|req, _| {
        if req.method == Method::Post {
            assert_eq!(req.body.as_deref(), Some(b"request body".as_slice()));
            Ok(HttpResponse {
                status: 303, // See Other
                headers: vec![(
                    "location".to_owned(),
                    "https://post.example.com/result".to_owned(),
                )],
                body: Vec::new(),
            })
        } else {
            assert_eq!(req.method, Method::Get);
            assert!(req.body.is_none());
            Ok(HttpResponse {
                status: 200,
                headers: Vec::new(),
                body: b"success".to_vec(),
            })
        }
    }));

    let client = Client::builder()
        .resolver(resolver)
        .transport(transport)
        .build()
        .unwrap();

    let resp = client
        .post(
            Url::parse("https://post.example.com/submit").unwrap(),
            b"request body".to_vec(),
            "application/octet-stream",
        )
        .unwrap();

    assert_eq!(resp.status, 200);
    assert_eq!(resp.body, b"success");
}

#[test]
fn test_client_timeout_handling() {
    let resolver = Arc::new(MockResolver::new());
    resolver.insert(
        "slow.example.com",
        vec![SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)),
            443,
        )],
    );

    let transport = Arc::new(MockTransport::new(|_, _| {
        std::thread::sleep(Duration::from_millis(50));
        Ok(HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: b"slow".to_vec(),
        })
    }));

    let client = Client::builder()
        .resolver(resolver)
        .transport(transport)
        .total_timeout(Duration::from_millis(10))
        .build()
        .unwrap();

    let err = client
        .get(Url::parse("https://slow.example.com/path").unwrap())
        .unwrap_err();

    assert!(matches!(err, NetError::Timeout));
}

#[test]
fn test_classify_and_is_public_ip() {
    assert!(is_public_ip(IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))));
    assert!(classify_ip(IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))).is_ok());

    assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
    assert!(classify_ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))).is_err());
}

#[test]
fn test_real_transport_oversize_line() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let server_handle = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = socket.read(&mut buf);
        // Send a status line exceeding 8 KiB without \n
        let long_line = "A".repeat(9000);
        let _ = socket.write_all(format!("HTTP/1.1 200 OK {long_line}\r\n\r\n").as_bytes());
    });

    let transport = DefaultTransport::new().unwrap();
    let url = Url::parse(&format!("http://localhost:{}/path", addr.port())).unwrap();
    let request = HttpRequest::get(url);
    let timeouts = Timeouts::default();
    let deadline = Instant::now() + Duration::from_secs(5);

    let err = transport
        .execute(&request, addr, &timeouts, 1024 * 1024, deadline)
        .unwrap_err();

    assert!(matches!(err, NetError::LineTooLong { limit: 8192 }));
    server_handle.join().unwrap();
}

#[test]
fn test_real_transport_giant_chunk() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let server_handle = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = socket.read(&mut buf);
        // Send chunked response with huge chunk size
        let _ = socket.write_all(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nffffffffffffffff\r\n",
        );
    });

    let transport = DefaultTransport::new().unwrap();
    let url = Url::parse(&format!("http://localhost:{}/path", addr.port())).unwrap();
    let request = HttpRequest::get(url);
    let timeouts = Timeouts::default();
    let deadline = Instant::now() + Duration::from_secs(5);

    let err = transport
        .execute(&request, addr, &timeouts, 64 * 1024, deadline)
        .unwrap_err();

    assert!(matches!(err, NetError::ResponseTooLarge { limit: 65536 }));
    server_handle.join().unwrap();
}

#[test]
fn test_real_transport_slowloris_drip_feed() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let server_handle = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = socket.read(&mut buf);
        // Send 1 byte every 40ms indefinitely
        for _ in 0..100 {
            if socket.write_all(b"H").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(40));
        }
    });

    let transport = DefaultTransport::new().unwrap();
    let url = Url::parse(&format!("http://localhost:{}/path", addr.port())).unwrap();
    let request = HttpRequest::get(url);
    let timeouts = Timeouts {
        connect: Duration::from_millis(100),
        read: Duration::from_millis(50),
        write: Duration::from_millis(50),
    };
    let deadline = Instant::now() + Duration::from_millis(80);

    let start = Instant::now();
    let err = transport
        .execute(&request, addr, &timeouts, 1024 * 1024, deadline)
        .unwrap_err();

    assert!(matches!(err, NetError::Timeout));
    assert!(start.elapsed() < Duration::from_millis(500));
    server_handle.join().unwrap();
}

#[test]
fn test_real_transport_untrusted_tls_cert() {
    // Generate self-signed TLS cert with rcgen
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let server_handle = std::thread::spawn(move || {
        let (tcp_stream, _) = listener.accept().unwrap();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let server_config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.cert.der().clone()],
                rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der())
                    .into(),
            )
            .unwrap();
        let conn = rustls::ServerConnection::new(Arc::new(server_config)).unwrap();
        let mut stream = rustls::StreamOwned::new(conn, tcp_stream);
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
    });

    // Default transport uses OS native roots, which will NOT trust this throwaway cert
    let transport = DefaultTransport::new().unwrap();
    let url = Url::parse(&format!("https://localhost:{}/path", addr.port())).unwrap();
    let request = HttpRequest::get(url);
    let timeouts = Timeouts::default();
    let deadline = Instant::now() + Duration::from_secs(5);

    let err = transport
        .execute(&request, addr, &timeouts, 1024 * 1024, deadline)
        .unwrap_err();

    assert!(
        matches!(err, NetError::Tls(_))
            || matches!(err, NetError::Io(ref io_err) if io_err.to_string().contains("cert")),
        "unexpected error: {err:?}"
    );
    server_handle.join().unwrap();
}

#[test]
fn test_real_transport_pinned_addr_and_headers() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let server_handle = std::thread::spawn(move || {
        let (mut socket, client_addr) = listener.accept().unwrap();
        assert_eq!(client_addr.ip(), IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
        let mut buf = [0u8; 2048];
        let n = socket.read(&mut buf).unwrap();
        let req_text = String::from_utf8_lossy(&buf[..n]);
        assert!(req_text.contains("GET /test-path HTTP/1.1\r\n"));
        // Proves pinned addr is used for socket connect while URL host is preserved in Host header
        assert!(req_text.contains("Host: pinned-target.unresolvable.invalid:9999\r\n"));
        assert!(req_text.contains("User-Agent: ashift/"));
        assert!(req_text.contains("x-custom-test: test-val\r\n"));
        let _ = socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\n\r\nsuccess");
    });

    let transport = DefaultTransport::new().unwrap();
    // Use an unresolvable hostname to prove the transport connects to pinned_addr without DNS lookup
    let url = Url::parse("http://pinned-target.unresolvable.invalid:9999/test-path").unwrap();
    let mut request = HttpRequest::get(url);
    request
        .headers
        .push(("x-custom-test".to_owned(), "test-val".to_owned()));
    let timeouts = Timeouts::default();
    let deadline = Instant::now() + Duration::from_secs(5);

    let resp = transport
        .execute(&request, addr, &timeouts, 1024 * 1024, deadline)
        .unwrap();

    assert_eq!(resp.status, 200);
    assert_eq!(resp.body, b"success");
    server_handle.join().unwrap();
}

#[test]
fn test_transport_header_crlf_injection_rejected() {
    let url = Url::parse("http://example.com/").unwrap();
    let mut request = HttpRequest::get(url);
    request
        .headers
        .push(("X-Bad\r\nInjected".to_owned(), "value".to_owned()));

    let transport = DefaultTransport::new().unwrap();
    let timeouts = Timeouts::default();
    let deadline = Instant::now() + Duration::from_secs(5);
    let dummy_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 80);

    let err = transport
        .execute(&request, dummy_addr, &timeouts, 1024, deadline)
        .unwrap_err();
    // Must strictly be InvalidHeader; failing with ConnectFailed would mean the check did not run upfront
    assert!(
        matches!(err, NetError::InvalidHeader(_)),
        "expected InvalidHeader error, got {err:?}"
    );
}

#[test]
fn test_ipv6_literal_host_resolution_and_headers() {
    let url = Url::parse("http://[::1]:8080/test-ipv6").unwrap();
    let request = HttpRequest::get(url);

    // host() must return IPv6 address without square brackets for getaddrinfo / socket resolution
    assert_eq!(request.host(), "::1");
    // host_header() must preserve brackets for HTTP/1.1 Host header
    assert_eq!(request.host_header(), "[::1]:8080");

    let resolver = MockResolver::new();
    let timeout = Duration::from_secs(2);
    let addrs = resolver.resolve(&request.host(), 8080, timeout).unwrap();
    assert_eq!(addrs.len(), 1);
    assert_eq!(addrs[0].ip(), "::1".parse::<IpAddr>().unwrap());
    assert_eq!(addrs[0].port(), 8080);
}

#[test]
fn test_cross_origin_307_post_body_dropped() {
    let resolver = Arc::new(MockResolver::new());
    resolver.insert(
        "origin1.example.com",
        vec![SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)),
            443,
        )],
    );
    resolver.insert(
        "origin2.example.com",
        vec![SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(93, 184, 216, 35)),
            443,
        )],
    );

    let seen_requests = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen_clone = Arc::clone(&seen_requests);

    let transport = Arc::new(MockTransport::new(move |req, _| {
        seen_clone
            .lock()
            .unwrap()
            .push((req.method, req.url.clone(), req.body.clone()));
        if req.url.host_str() == Some("origin1.example.com") {
            Ok(HttpResponse {
                status: 307,
                headers: vec![(
                    "location".to_owned(),
                    "https://origin2.example.com/target".to_owned(),
                )],
                body: Vec::new(),
            })
        } else {
            Ok(HttpResponse {
                status: 200,
                headers: Vec::new(),
                body: b"target-reached".to_vec(),
            })
        }
    }));

    let client = Client::builder()
        .resolver(resolver)
        .transport(transport)
        .build()
        .unwrap();

    let url = Url::parse("https://origin1.example.com/init").unwrap();
    let resp = client
        .post(url, b"sensitive-post-body".to_vec(), "application/json")
        .unwrap();
    assert_eq!(resp.status, 200);

    let reqs = seen_requests.lock().unwrap();
    assert_eq!(reqs.len(), 2);
    // Initial request is POST with body
    assert_eq!(reqs[0].0, Method::Post);
    assert_eq!(reqs[0].2, Some(b"sensitive-post-body".to_vec()));

    // Redirected hop is cross-origin: method switched to GET and body dropped!
    assert_eq!(reqs[1].0, Method::Get);
    assert_eq!(reqs[1].2, None);
}

#[test]
fn test_loopback_bounded_trailers_success_and_overflow() {
    // 1. Valid trailer within bounds
    {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut req_buf = [0u8; 1024];
            let _ = stream.read(&mut req_buf);
            let resp = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\ntest\r\n0\r\nX-Trailer: valid-trailer\r\n\r\n";
            let _ = stream.write_all(resp.as_bytes());
        });

        let client = Client::builder()
            .allow_loopback(true)
            .https_only(false)
            .allowed_ports(vec![port])
            .build()
            .unwrap();

        let url = Url::parse(&format!("http://127.0.0.1:{port}/trailer-ok")).unwrap();
        let resp = client
            .get(url)
            .expect("chunked response with trailer should succeed");
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"test");
        handle.join().unwrap();
    }

    // 2. Trailer overflowing MAX_HEADERS_BYTES (64 KiB)
    {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut req_buf = [0u8; 1024];
            let _ = stream.read(&mut req_buf);
            let chunk_head =
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\ntest\r\n0\r\n";
            let _ = stream.write_all(chunk_head.as_bytes());
            let trailer_line = format!("X-Large-Trailer: {}\r\n", "A".repeat(1000));
            for _ in 0..70 {
                if stream.write_all(trailer_line.as_bytes()).is_err() {
                    break;
                }
            }
            let _ = stream.write_all(b"\r\n");
        });

        let client = Client::builder()
            .allow_loopback(true)
            .https_only(false)
            .allowed_ports(vec![port])
            .build()
            .unwrap();

        let url = Url::parse(&format!("http://127.0.0.1:{port}/trailer-overflow")).unwrap();
        let err = client.get(url).expect_err("overflowing trailers must fail");
        match err {
            NetError::HeadersTooLarge { limit } => {
                assert_eq!(limit, 64 * 1024);
            }
            other => panic!("expected HeadersTooLarge error, got {other:?}"),
        }
        handle.join().unwrap();
    }
}

#[derive(Debug)]
struct SleepingResolver(Duration);
impl Resolver for SleepingResolver {
    fn resolve(
        &self,
        _host: &str,
        _port: u16,
        timeout: Duration,
    ) -> Result<Vec<SocketAddr>, NetError> {
        if self.0 > timeout {
            std::thread::sleep(timeout);
            return Err(NetError::Timeout);
        }
        std::thread::sleep(self.0);
        Ok(vec![SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)),
            _port,
        )])
    }
}

#[test]
fn test_dns_resolver_budget_sleep_timeout() {
    let resolver = Arc::new(SleepingResolver(Duration::from_millis(200)));
    let client = Client::builder()
        .resolver(resolver)
        .connect_timeout(Duration::from_millis(30))
        .build()
        .unwrap();

    let url = Url::parse("https://example.com/test-dns-timeout").unwrap();
    let err = client.get(url).expect_err("DNS timeout should fail");
    assert!(
        matches!(err, NetError::Timeout),
        "expected NetError::Timeout, got {err:?}"
    );
}

#[test]
fn test_port_policy_with_allow_loopback() {
    let client = Client::builder()
        .allow_loopback(true)
        .https_only(false)
        .build()
        .unwrap();

    let url = Url::parse("http://127.0.0.1:8080/forbidden-port").unwrap();
    let err = client
        .get(url)
        .expect_err("non-standard port must be rejected");
    match err {
        NetError::PortNotAllowed { port } => assert_eq!(port, 8080),
        other => panic!("expected PortNotAllowed, got {other:?}"),
    }
}

#[test]
fn test_sni_server_name_assertion() {
    let url = Url::parse("https://tsa.example.com/query").unwrap();
    let req = HttpRequest::get(url);
    let sni = req.sni_name().expect("sni name should resolve");
    match sni {
        rustls::pki_types::ServerName::DnsName(dns) => {
            assert_eq!(dns.as_ref(), "tsa.example.com");
        }
        _ => panic!("expected DnsName"),
    }

    let ip_url = Url::parse("https://127.0.0.1/query").unwrap();
    let ip_req = HttpRequest::get(ip_url);
    let ip_sni = ip_req.sni_name().expect("ip sni name should resolve");
    match ip_sni {
        rustls::pki_types::ServerName::IpAddress(ip) => match ip {
            rustls::pki_types::IpAddr::V4(v4) => assert_eq!(v4, [127, 0, 0, 1].into()),
            _ => panic!("expected V4"),
        },
        _ => panic!("expected IpAddress"),
    }
}

#[test]
fn test_sni_client_hello_wire_assertion() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let (tx, rx) = std::sync::mpsc::channel();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf).unwrap();
        tx.send(buf[..n].to_vec()).unwrap();
    });

    let resolver = Arc::new(MockResolver::new());
    resolver.insert(
        "tsa.test.local",
        vec![SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            port,
        )],
    );

    let client = Client::builder()
        .resolver(resolver)
        .allow_loopback(true)
        .allowed_ports(vec![port])
        .build()
        .unwrap();

    let url = Url::parse(&format!("https://tsa.test.local:{port}/query")).unwrap();
    let _ = client.get(url);
    handle.join().unwrap();

    let wire_bytes = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("received ClientHello");
    assert!(wire_bytes.len() > 5);
    assert_eq!(wire_bytes[0], 0x16); // Handshake
    assert_eq!(wire_bytes[1], 0x03); // Major version 3

    let wire_str = String::from_utf8_lossy(&wire_bytes);
    assert!(
        wire_str.contains("tsa.test.local"),
        "ClientHello wire bytes must contain SNI host"
    );
}

#[test]
fn test_tsa_allowlist_matching() {
    use crate::tools::sign::matches_tsa_allowlist;

    let target_good = Url::parse("https://tsa.example.com/tsa").unwrap();
    let target_evil = Url::parse("https://tsa.example.com.evil.com/tsa").unwrap();
    let target_sub = Url::parse("https://sub.tsa.example.com/tsa").unwrap();
    let target_http = Url::parse("http://tsa.example.com/tsa").unwrap();
    let target_port8443 = Url::parse("https://tsa.example.com:8443/tsa").unwrap();

    let allowlist = "https://tsa.example.com";
    assert!(matches_tsa_allowlist(&target_good, allowlist));
    assert!(!matches_tsa_allowlist(&target_evil, allowlist));
    assert!(!matches_tsa_allowlist(&target_sub, allowlist));
    assert!(!matches_tsa_allowlist(&target_http, allowlist));
    assert!(!matches_tsa_allowlist(&target_port8443, allowlist));

    let multi = "https://tsa1.example.com, https://tsa.example.com, https://tsa2.example.com";
    assert!(matches_tsa_allowlist(&target_good, multi));
    assert!(!matches_tsa_allowlist(&target_evil, multi));

    let path_allow = "https://tsa.example.com/tsa";
    let other_path = Url::parse("https://tsa.example.com/admin").unwrap();
    assert!(matches_tsa_allowlist(&target_good, path_allow));
    assert!(!matches_tsa_allowlist(&other_path, path_allow));
}

#[test]
fn test_cloud_default_deny_tsa_timestamp() {
    use crate::tools::sign::check_tsa_policy;

    let target = Url::parse("https://tsa.example.com/tsa").unwrap();

    // 1. Cloud mode without allowlist -> denied
    let err =
        check_tsa_policy(&target, None, true).expect_err("cloud mode without allowlist must fail");
    assert!(
        err.to_string()
            .contains("TSA requests are disabled in cloud unless an allowlist is configured")
    );

    // 2. Non-cloud mode without allowlist -> permitted
    assert!(check_tsa_policy(&target, None, false).is_ok());

    // 3. Cloud mode with matching allowlist -> permitted
    assert!(check_tsa_policy(&target, Some("https://tsa.example.com"), true).is_ok());

    // 4. Cloud mode with non-matching allowlist -> denied
    let err2 = check_tsa_policy(&target, Some("https://other.example.com"), true)
        .expect_err("mismatched allowlist must fail");
    assert!(
        err2.to_string()
            .contains("TSA URL is not in the allowed TSA list")
    );
}
