use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use url::Url;

use super::{
    client::Client,
    error::NetError,
    ip::{IpCategory, classify_ip, classify_ipv4, classify_ipv6, is_public_ip},
    resolver::{MockResolver, resolve_and_pin_public},
    transport::{HttpResponse, Method, MockTransport},
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

    // Link-local and cloud metadata (169.254.169.254)
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(169, 254, 1, 1)),
        Err(IpCategory::LinkLocal)
    );
    assert_eq!(
        classify_ipv4(Ipv4Addr::new(169, 254, 169, 254)),
        Err(IpCategory::CloudMetadata)
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

    // Documentation
    assert_eq!(
        classify_ipv6(Ipv6Addr::from_str("2001:db8::1").unwrap()),
        Err(IpCategory::Documentation)
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
        Err(NetError::CredentialsNotAllowed(_))
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
    let err = resolve_and_pin_public(&resolver, "internal.corp", 443, false);
    assert!(matches!(err, Err(NetError::NonPublicAddress { .. })));

    // Host resolving to metadata IP rejected
    resolver.insert(
        "metadata.aws",
        vec![SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254)),
            443,
        )],
    );
    let err = resolve_and_pin_public(&resolver, "metadata.aws", 443, false);
    assert!(matches!(err, Err(NetError::NonPublicAddress { .. })));

    // Host resolving to mixed public + private IP: WHOLE ANSWER REJECTED
    resolver.insert(
        "mixed.attacker.com",
        vec![
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)), 443), // public
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 443),     // loopback
        ],
    );
    let err = resolve_and_pin_public(&resolver, "mixed.attacker.com", 443, false);
    assert!(matches!(err, Err(NetError::NonPublicAddress { .. })));

    // Pure public IP resolves and pins
    resolver.insert(
        "safe.example.com",
        vec![SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)),
            443,
        )],
    );
    let pinned = resolve_and_pin_public(&resolver, "safe.example.com", 443, false).unwrap();
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
