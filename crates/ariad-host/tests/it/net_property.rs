//! Generative property tests for SSRF address filtering and URL/redirect validation.

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use ariad_host::net::{
    Client, HttpResponse, IpCategory, MockResolver, MockTransport, NetError, UrlPolicy,
    classify_ip, classify_ipv4, classify_ipv6, is_public_ip, validate_url,
};
use url::Url;

/// Simple deterministic linear congruential generator for property testing.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        (self.0 >> 32) as u32
    }

    fn next_u8(&mut self) -> u8 {
        (self.next_u32() & 0xFF) as u8
    }

    fn next_bool(&mut self) -> bool {
        (self.next_u32() & 1) == 1
    }

    fn range(&mut self, min: u32, max: u32) -> u32 {
        if min >= max {
            return min;
        }
        min + (self.next_u32() % (max - min + 1))
    }
}

#[test]
fn property_ipv4_private_and_metadata_never_public() {
    let mut rng = Rng::new(0x1337_c001_dead_beef);

    for _ in 0..10_000 {
        let b = rng.next_u8();
        let c = rng.next_u8();
        let d = rng.next_u8();

        // 10.x.x.x is always private
        let ip_10 = Ipv4Addr::new(10, b, c, d);
        assert_eq!(classify_ipv4(ip_10), Err(IpCategory::Private));
        assert!(!is_public_ip(IpAddr::V4(ip_10)));

        // 127.x.x.x is always loopback
        let ip_127 = Ipv4Addr::new(127, b, c, d);
        assert_eq!(classify_ipv4(ip_127), Err(IpCategory::Loopback));
        assert!(!is_public_ip(IpAddr::V4(ip_127)));

        // 169.254.x.x is always link-local or metadata
        let ip_169 = Ipv4Addr::new(169, 254, b, c);
        assert!(matches!(
            classify_ipv4(ip_169),
            Err(IpCategory::LinkLocal | IpCategory::CloudMetadata)
        ));
        assert!(!is_public_ip(IpAddr::V4(ip_169)));

        // 192.168.x.x is always private
        let ip_192 = Ipv4Addr::new(192, 168, b, c);
        assert_eq!(classify_ipv4(ip_192), Err(IpCategory::Private));
        assert!(!is_public_ip(IpAddr::V4(ip_192)));

        // 172.16-31.x.x is always private
        let octet2 = rng.range(16, 31) as u8;
        let ip_172 = Ipv4Addr::new(172, octet2, b, c);
        assert_eq!(classify_ipv4(ip_172), Err(IpCategory::Private));
        assert!(!is_public_ip(IpAddr::V4(ip_172)));

        // 100.64-127.x.x is always CGNAT
        let octet2_cgnat = rng.range(64, 127) as u8;
        let ip_cgnat = Ipv4Addr::new(100, octet2_cgnat, b, c);
        assert_eq!(classify_ipv4(ip_cgnat), Err(IpCategory::SharedAddressSpace));
        assert!(!is_public_ip(IpAddr::V4(ip_cgnat)));
    }
}

#[test]
fn property_ipv6_embedded_ipv4_preserves_ssrf_safety() {
    let mut rng = Rng::new(0xca11_ab1e_55aa);

    for _ in 0..5_000 {
        let b = rng.next_u8();
        let c = rng.next_u8();

        // NAT64 embedding private 10.x.x.x
        let v6_nat64_priv = Ipv6Addr::new(
            0x0064,
            0xff9b,
            0,
            0,
            0,
            0,
            (10 << 8) | (b as u16),
            (c as u16) << 8 | 1,
        );
        assert_eq!(
            classify_ipv6(v6_nat64_priv),
            Err(IpCategory::Nat64NonPublic)
        );
        assert!(!is_public_ip(IpAddr::V6(v6_nat64_priv)));

        // IPv4-mapped embedding loopback 127.x.x.x
        let v6_mapped_loop = Ipv6Addr::new(
            0,
            0,
            0,
            0,
            0,
            0xffff,
            (127 << 8) | (b as u16),
            (c as u16) << 8 | 1,
        );
        assert_eq!(
            classify_ipv6(v6_mapped_loop),
            Err(IpCategory::Ipv4MappedNonPublic)
        );
        assert!(!is_public_ip(IpAddr::V6(v6_mapped_loop)));

        // Unique Local Address (fc00::/7)
        let prefix = if rng.next_bool() { 0xfc00 } else { 0xfd00 } | (rng.next_u8() as u16);
        let ula = Ipv6Addr::new(prefix, rng.next_u8() as u16, b as u16, c as u16, 0, 0, 0, 1);
        assert!(matches!(
            classify_ip(IpAddr::V6(ula)),
            Err(IpCategory::UniqueLocal | IpCategory::CloudMetadata)
        ));
    }
}

#[test]
fn property_url_validation_invariants() {
    let policy = UrlPolicy::default();
    let mut rng = Rng::new(0xfeed_cafe_dead);

    let schemes = [
        "https", "http", "ftp", "file", "gopher", "data", "blob", "ws", "wss",
    ];
    let hosts = [
        "example.com",
        "api.sub.domain.org",
        "127.0.0.1",
        "169.254.169.254",
        "localhost",
        "[::1]",
    ];

    for _ in 0..2_000 {
        let scheme = schemes[rng.range(0, schemes.len() as u32 - 1) as usize];
        let host = hosts[rng.range(0, hosts.len() as u32 - 1) as usize];
        let has_creds = rng.next_bool();
        let port = if rng.next_bool() {
            format!(":{}", rng.range(1, 65535))
        } else {
            String::new()
        };

        let raw = if has_creds {
            format!("{scheme}://user:pass@{host}{port}/path?q=1")
        } else {
            format!("{scheme}://{host}{port}/path?q=1")
        };

        if let Ok(parsed) = Url::parse(&raw) {
            let res = validate_url(&parsed, &policy);
            let has_non_standard_port = parsed.port().is_some_and(|p| p != 443);
            if scheme != "https" {
                assert!(
                    res.is_err(),
                    "non-https scheme {scheme} must be rejected under default policy"
                );
            } else if has_creds {
                assert!(res.is_err(), "credentials must be rejected");
            } else if has_non_standard_port {
                assert!(
                    res.is_err(),
                    "non-standard port must be rejected under default policy"
                );
            } else {
                assert!(
                    res.is_ok(),
                    "valid HTTPS without credentials should pass URL policy"
                );
            }
        }
    }
}

#[test]
fn property_redirect_chain_cannot_exceed_cap_or_downgrade() {
    let mut rng = Rng::new(0x7777_8888_9999);

    for _ in 0..20 {
        let max_redirects = rng.range(1, 5);
        let chain_length = rng.range(0, 10);
        let target_downgrades = rng.next_bool();

        let resolver = Arc::new(MockResolver::new());
        // Map all hop domains to public IP 93.184.216.34
        for i in 0..=chain_length {
            resolver.insert(
                format!("hop{i}.example.com"),
                vec![SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)),
                    443,
                )],
            );
        }

        let transport = Arc::new(MockTransport::new(move |req, _| {
            let host = req.host();
            if let Some(num_str) = host
                .strip_prefix("hop")
                .and_then(|s| s.strip_suffix(".example.com"))
            {
                let current_hop: u32 = num_str.parse().unwrap_or(0);
                if current_hop < chain_length {
                    let next_hop = current_hop + 1;
                    let target_scheme = if target_downgrades && next_hop == chain_length {
                        "http"
                    } else {
                        "https"
                    };
                    Ok(HttpResponse {
                        status: 302,
                        headers: vec![(
                            "location".to_owned(),
                            format!("{target_scheme}://hop{next_hop}.example.com/step"),
                        )],
                        body: Vec::new(),
                    })
                } else {
                    Ok(HttpResponse {
                        status: 200,
                        headers: Vec::new(),
                        body: b"destination reached".to_vec(),
                    })
                }
            } else {
                Ok(HttpResponse {
                    status: 200,
                    headers: Vec::new(),
                    body: Vec::new(),
                })
            }
        }));

        let client = Client::builder()
            .resolver(resolver)
            .transport(transport)
            .max_redirects(max_redirects)
            .total_timeout(Duration::from_secs(5))
            .build()
            .unwrap();

        let start_url = Url::parse("https://hop0.example.com/start").unwrap();
        let res = client.get(start_url);

        if target_downgrades && chain_length > 0 && chain_length <= max_redirects {
            assert!(
                matches!(res, Err(NetError::DowngradeRefused(_))),
                "downgrade must be refused"
            );
        } else if chain_length > max_redirects {
            assert!(
                matches!(res, Err(NetError::TooManyRedirects { .. })),
                "chain exceeding max_redirects must fail with TooManyRedirects"
            );
        } else {
            assert!(
                res.is_ok(),
                "compliant redirect chain within cap should succeed"
            );
        }
    }
}
