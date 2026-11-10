//! Which resolved addresses the gateway may connect to.
//!
//! The gateway sits in a normal Podman network. From there gvproxy forwards
//! `192.168.127.254` to `127.0.0.1` of the Windows host, and the LAN, the
//! router and the tailnet are reachable too (phase 0, 6.4 and 12.6). Only
//! public unicast addresses are therefore allowed, in every mode.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Whether `ip` is a public unicast address the gateway may connect to.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_v4(ip),
        IpAddr::V6(ip) => is_public_v6(ip),
    }
}

/// Blocked IPv4 ranges (IANA special-purpose registry plus multicast and
/// reserved space).
const BLOCKED_V4: &[(Ipv4Addr, u8)] = &[
    (Ipv4Addr::new(0, 0, 0, 0), 8),       // "this network"
    (Ipv4Addr::new(10, 0, 0, 0), 8),      // private
    (Ipv4Addr::new(100, 64, 0, 0), 10),   // CGNAT, Tailscale
    (Ipv4Addr::new(127, 0, 0, 0), 8),     // loopback
    (Ipv4Addr::new(169, 254, 0, 0), 16),  // link-local, cloud metadata
    (Ipv4Addr::new(172, 16, 0, 0), 12),   // private
    (Ipv4Addr::new(192, 0, 0, 0), 24),    // IETF protocol assignments
    (Ipv4Addr::new(192, 0, 2, 0), 24),    // documentation
    (Ipv4Addr::new(192, 88, 99, 0), 24),  // 6to4 relay
    (Ipv4Addr::new(192, 168, 0, 0), 16),  // private, gvproxy 192.168.127.0/24
    (Ipv4Addr::new(198, 18, 0, 0), 15),   // benchmarking
    (Ipv4Addr::new(198, 51, 100, 0), 24), // documentation
    (Ipv4Addr::new(203, 0, 113, 0), 24),  // documentation
    (Ipv4Addr::new(224, 0, 0, 0), 4),     // multicast
    (Ipv4Addr::new(240, 0, 0, 0), 4),     // reserved, broadcast
];

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let bits = u32::from(ip);
    !BLOCKED_V4.iter().any(|(net, len)| {
        let mask = u32::MAX << (32 - u32::from(*len));
        bits & mask == u32::from(*net) & mask
    })
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    // IPv4-mapped (::ffff:a.b.c.d) and NAT64 (64:ff9b::a.b.c.d) reach the
    // embedded IPv4 address, so that address decides.
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_public_v4(v4);
    }
    if segments[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        let [a, b] = segments[6].to_be_bytes();
        let [c, d] = segments[7].to_be_bytes();
        return is_public_v4(Ipv4Addr::new(a, b, c, d));
    }
    // Only global unicast (2000::/3) is public; that excludes loopback,
    // unique-local, link-local, multicast and the deprecated IPv4-compatible
    // form in one go.
    if segments[0] & 0xe000 != 0x2000 {
        return false;
    }
    let blocked = [
        // IETF protocol assignments incl. Teredo (2001::/23).
        segments[0] == 0x2001 && segments[1] < 0x0200,
        // Documentation (2001:db8::/32).
        segments[0] == 0x2001 && segments[1] == 0x0db8,
        // 6to4 (2002::/16) can tunnel to any IPv4 address, private ones too.
        segments[0] == 0x2002,
    ];
    !blocked.contains(&true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn public(text: &str) -> bool {
        is_public(text.parse().unwrap())
    }

    #[test]
    fn public_addresses() {
        for ip in [
            "1.1.1.1",
            "93.184.215.14",
            "140.82.121.4",
            "100.63.255.255",
            "100.128.0.0",
            "172.15.255.255",
            "172.32.0.0",
            "192.169.0.1",
            "223.255.255.255",
            "2606:4700:4700::1111",
            "2a00:1450:4001:80b::200e",
            "::ffff:1.1.1.1",
            "64:ff9b::1.1.1.1",
        ] {
            assert!(public(ip), "{ip} blocked");
        }
    }

    #[test]
    fn blocked_addresses() {
        for ip in [
            "0.0.0.0",
            "10.88.0.1",
            "100.64.0.1",
            "100.100.100.100",
            "127.0.0.1",
            "127.255.255.254",
            "169.254.169.254",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.127.254",
            "192.168.1.1",
            "192.0.2.1",
            "198.18.0.1",
            "224.0.0.1",
            "239.255.255.250",
            "255.255.255.255",
            "::",
            "::1",
            "::127.0.0.1",
            "::ffff:127.0.0.1",
            "::ffff:192.168.127.254",
            "64:ff9b::7f00:1",
            "64:ff9b::a00:1",
            "fc00::1",
            "fd7a:115c:a1e0::1",
            "fe80::1",
            "ff02::1",
            "2001:db8::1",
            "2001::1",
            "2002:7f00:1::1",
        ] {
            assert!(!public(ip), "{ip} allowed");
        }
    }
}
