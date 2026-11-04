//! IP address classification and public-IP verification for SSRF prevention.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Reason an IP address is categorized as non-public.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum IpCategory {
    #[error("loopback address")]
    Loopback,
    #[error("unspecified address")]
    Unspecified,
    #[error("private address")]
    Private,
    #[error("link-local address")]
    LinkLocal,
    #[error("multicast address")]
    Multicast,
    #[error("cloud metadata service address")]
    CloudMetadata,
    #[error("shared address space (CGNAT)")]
    SharedAddressSpace,
    #[error("documentation address")]
    Documentation,
    #[error("benchmarking network address")]
    Benchmarking,
    #[error("reserved address")]
    Reserved,
    #[error("unique local address (ULA)")]
    UniqueLocal,
    #[error("IPv4-mapped address with non-public inner IPv4")]
    Ipv4MappedNonPublic,
    #[error("NAT64 address with non-public inner IPv4")]
    Nat64NonPublic,
    #[error("non-global unicast IPv6 address")]
    NonGlobalUnicast,
}

/// Classifies an IP address, returning `Ok(())` if it is public or `Err(IpCategory)` if non-public.
pub fn classify_ip(ip: IpAddr) -> Result<(), IpCategory> {
    match ip {
        IpAddr::V4(v4) => classify_ipv4(v4),
        IpAddr::V6(v6) => classify_ipv6(v6),
    }
}

/// Returns `true` if the IP address is globally routable and public (not loopback, private, link-local,
/// multicast, unspecified, cloud metadata, NAT64 non-public, or reserved).
pub fn is_public_ip(ip: IpAddr) -> bool {
    classify_ip(ip).is_ok()
}

/// Classifies an IPv4 address.
pub fn classify_ipv4(v4: Ipv4Addr) -> Result<(), IpCategory> {
    let octets = v4.octets();

    // 0.0.0.0/8 - Unspecified / current network
    if octets[0] == 0 {
        return Err(IpCategory::Unspecified);
    }

    // 127.0.0.0/8 - Loopback
    if octets[0] == 127 {
        return Err(IpCategory::Loopback);
    }

    // 10.0.0.0/8 - Private RFC 1918
    if octets[0] == 10 {
        return Err(IpCategory::Private);
    }

    // 100.64.0.0/10 - Shared Address Space RFC 6598 (CGNAT)
    if octets[0] == 100 && (octets[1] & 0b1100_0000 == 64) {
        return Err(IpCategory::SharedAddressSpace);
    }

    // 169.254.0.0/16 - Link-local RFC 3927 (includes 169.254.169.254 cloud metadata)
    if octets[0] == 169 && octets[1] == 254 {
        if octets[2] == 169 && octets[3] == 254 {
            return Err(IpCategory::CloudMetadata);
        }
        return Err(IpCategory::LinkLocal);
    }

    // 172.16.0.0/12 - Private RFC 1918
    if octets[0] == 172 && (octets[1] >= 16 && octets[1] <= 31) {
        return Err(IpCategory::Private);
    }

    // 192.0.0.0/24 - IETF Protocol Assignments RFC 6890
    if octets[0] == 192 && octets[1] == 0 && octets[2] == 0 {
        return Err(IpCategory::Reserved);
    }

    // 192.0.2.0/24 - TEST-NET-1 Documentation RFC 5737
    if octets[0] == 192 && octets[1] == 0 && octets[2] == 2 {
        return Err(IpCategory::Documentation);
    }

    // 192.88.99.0/24 - 6to4 Relay Anycast RFC 7526 (deprecated)
    if octets[0] == 192 && octets[1] == 88 && octets[2] == 99 {
        return Err(IpCategory::Reserved);
    }

    // 192.168.0.0/16 - Private RFC 1918
    if octets[0] == 192 && octets[1] == 168 {
        return Err(IpCategory::Private);
    }

    // 198.18.0.0/15 - Benchmarking RFC 2544
    if octets[0] == 198 && (octets[1] == 18 || octets[1] == 19) {
        return Err(IpCategory::Benchmarking);
    }

    // 198.51.100.0/24 - TEST-NET-2 Documentation RFC 5737
    if octets[0] == 198 && octets[1] == 51 && octets[2] == 100 {
        return Err(IpCategory::Documentation);
    }

    // 203.0.113.0/24 - TEST-NET-3 Documentation RFC 5737
    if octets[0] == 203 && octets[1] == 0 && octets[2] == 113 {
        return Err(IpCategory::Documentation);
    }

    // 224.0.0.0/4 - Multicast RFC 5771
    if octets[0] >= 224 && octets[0] <= 239 {
        return Err(IpCategory::Multicast);
    }

    // 240.0.0.0/4 - Reserved / Future use RFC 1112 (includes 255.255.255.255 broadcast)
    if octets[0] >= 240 {
        return Err(IpCategory::Reserved);
    }

    Ok(())
}

/// Classifies an IPv6 address.
pub fn classify_ipv6(v6: Ipv6Addr) -> Result<(), IpCategory> {
    let octets = v6.octets();
    let segments = v6.segments();

    // ::/128 - Unspecified
    if v6.is_unspecified() {
        return Err(IpCategory::Unspecified);
    }

    // ::1/128 - Loopback
    if v6.is_loopback() {
        return Err(IpCategory::Loopback);
    }

    // ff00::/8 - Multicast RFC 4291
    if octets[0] == 0xff {
        return Err(IpCategory::Multicast);
    }

    // fc00::/7 - Unique Local Address RFC 4193 (includes fd00:ec2::254 cloud metadata)
    if octets[0] & 0xfe == 0xfc {
        // fd00:ec2::254 is AWS/cloud IPv6 IMDS metadata service
        if segments[0] == 0xfd00 && segments[1] == 0x0ec2 && segments[7] == 0x0254 {
            return Err(IpCategory::CloudMetadata);
        }
        return Err(IpCategory::UniqueLocal);
    }

    // fe80::/10 - Link-local unicast RFC 4291
    if octets[0] == 0xfe && (octets[1] & 0xc0 == 0x80) {
        return Err(IpCategory::LinkLocal);
    }

    // ::ffff:0:0/96 - IPv4-mapped IPv6 address RFC 4291
    if octets[0..10] == [0; 10] && octets[10] == 0xff && octets[11] == 0xff {
        let inner = Ipv4Addr::new(octets[12], octets[13], octets[14], octets[15]);
        return classify_ipv4(inner).map_err(|_| IpCategory::Ipv4MappedNonPublic);
    }

    // ::ffff:0:0:0/96 - IPv4-translated address RFC 2765
    if octets[0..10] == [0; 10] && octets[10] == 0 && octets[11] == 0 && octets[12..16] != [0; 4] {
        let inner = Ipv4Addr::new(octets[12], octets[13], octets[14], octets[15]);
        return classify_ipv4(inner).map_err(|_| IpCategory::Ipv4MappedNonPublic);
    }

    // 64:ff9b::/96 - Well-Known Prefix for NAT64 RFC 6052
    if segments[0] == 0x0064 && segments[1] == 0xff9b && segments[2..6] == [0; 4] {
        let inner = Ipv4Addr::new(octets[12], octets[13], octets[14], octets[15]);
        return classify_ipv4(inner).map_err(|_| IpCategory::Nat64NonPublic);
    }

    // 64:ff9b:1::/48 - Local-Use NAT64 RFC 8215
    if segments[0] == 0x0064 && segments[1] == 0xff9b && segments[2] == 0x0001 {
        return Err(IpCategory::Nat64NonPublic);
    }

    // 100::/64 - Discard prefix RFC 6666
    if segments[0] == 0x0100 && segments[1..4] == [0; 3] {
        return Err(IpCategory::Reserved);
    }

    // 2001:db8::/32 - Documentation RFC 3849
    if segments[0] == 0x2001 && segments[1] == 0x0db8 {
        return Err(IpCategory::Documentation);
    }

    // 2001:0000::/32 - Teredo RFC 4380 (embeds IPv4; reserved for tunneling)
    if segments[0] == 0x2001 && segments[1] == 0x0000 {
        return Err(IpCategory::Reserved);
    }

    // 2002::/16 - 6to4 RFC 3056 (embeds IPv4)
    if segments[0] == 0x2002 {
        let inner = Ipv4Addr::new(octets[2], octets[3], octets[4], octets[5]);
        return classify_ipv4(inner).map_err(|_| IpCategory::Reserved);
    }

    // All IANA-allocated public global unicast addresses reside in 2000::/3 (RFC 4291 §2.4)
    if (segments[0] & 0xe000) != 0x2000 {
        return Err(IpCategory::NonGlobalUnicast);
    }

    Ok(())
}
