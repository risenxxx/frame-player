//! Finding a television when multicast does not reach it.
//!
//! Both discoveries are multicast — mDNS for Cast, SSDP for DLNA — and a good
//! many routers do not carry multicast between their segments: a laptop on a
//! cable or on 5 GHz and a television on 2.4 GHz can share a subnet and route
//! unicast between each other without trouble, while every group datagram is
//! dropped at the bridge (IGMP snooping, multicast-to-unicast conversion on one
//! band, mesh backhaul). Everything *after* discovery is unicast — the CASTV2
//! connection, the SOAP calls, the television fetching from our server — so the
//! only thing that fails is being found, and the panel says nobody is there.
//!
//! So each address is also asked directly, in the way each protocol answers:
//!
//! - **Cast: `:8009` open, then `GET :8008/setup/eureka_info`** for the name
//!   (cast.rs). The obvious route, an mDNS query sent straight to the device's
//!   own address, was measured first and is dead: a Cast soundbar that answered
//!   ping and served `eureka_info` stayed silent to it from an ephemeral port,
//!   from 5353, with and without the QU bit — Cast's responder listens on the
//!   group and nowhere else. (A router on the same network did answer it, which
//!   is what makes that a property of the device and not of the probe.)
//! - **DLNA: unicast `M-SEARCH`** to `host:1900` (UPnP Device Architecture 1.1
//!   §1.3.2) — measured answered by a UPnP 1.1 router; a 1.0 renderer may stay
//!   silent, which costs nothing but its absence here.
//!
//! Asked of which addresses: the ones a television was seen at before (the
//! frontend remembers them), then every host of each LAN subnet. A /24 is 254
//! connection attempts or datagrams — a round of about six seconds — and the sweep stays browse-only — nothing is
//! announced, nothing is played to. It does **not** get past a network that
//! refuses unicast between clients too (a guest network, AP isolation): there
//! the television could not fetch the file from us either, and nothing on this
//! side can help.
//!
//! **A precaution, not a measured fix.** The report that prompted it turned out
//! to be a television on another network; no router that drops multicast
//! between bands has been tested against yet.

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

/// Subnets wider than this are not swept whole — only the /24 around our own
/// address. 1022 hosts is still a trivial burst; a /16 is 65 534 and would read
/// as a scan to anything watching the network.
const WIDEST_PREFIX: u8 = 22;

/// Datagrams per burst before a short pause. Every address that has not been
/// talked to needs an ARP resolution first, and the kernel holds only one or
/// three packets per pending entry — pacing keeps a sweep from being dropped
/// on the floor by its own neighbour table.
const BURST: usize = 32;
const BURST_PAUSE: Duration = Duration::from_millis(4);

/// The LAN interfaces a sweep may cover: IPv4, private, not loopback or
/// link-local, and not a tunnel or a virtual switch (by interface name). A
/// tunnel's address is often private too, and sweeping somebody's corporate
/// network from a video player is not a thing to do by accident; a VM bridge
/// (three of them on the machine this was written on) holds no television.
fn lan_interfaces() -> Vec<(Ipv4Addr, u8)> {
    let mut out = Vec::new();
    for iface in if_addrs::get_if_addrs().unwrap_or_default() {
        let if_addrs::IfAddr::V4(v4) = &iface.addr else {
            continue;
        };
        if v4.ip.is_loopback() || v4.ip.is_link_local() || !v4.ip.is_private() {
            continue;
        }
        if is_tunnel_name(&iface.name) {
            continue;
        }
        out.push((v4.ip, v4.prefixlen));
    }
    out
}

fn is_tunnel_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["utun", "tun", "tap", "wg", "ppp", "ipsec", "tailscale", "zt", "bridge", "vmnet", "vboxnet", "docker", "veth", "vethernet"]
        .iter()
        .any(|p| name.starts_with(p))
}

/// Every address a sweep asks, in order: the remembered ones first (they are
/// the likely answer and the cheapest to confirm), then each subnet's hosts.
/// Our own addresses, network and broadcast addresses are never included, and
/// nothing appears twice. Pure, so the arithmetic is testable.
pub(crate) fn targets(ifaces: &[(Ipv4Addr, u8)], hints: &[Ipv4Addr]) -> Vec<Ipv4Addr> {
    let own: Vec<Ipv4Addr> = ifaces.iter().map(|(ip, _)| *ip).collect();
    let mut out: Vec<Ipv4Addr> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for ip in hints {
        if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() || own.contains(ip) {
            continue;
        }
        if seen.insert(*ip) {
            out.push(*ip);
        }
    }
    for (ip, prefix) in ifaces {
        if *prefix > 30 {
            continue;
        }
        let prefix = if *prefix < WIDEST_PREFIX { 24 } else { *prefix };
        let mask = u32::MAX << (32 - prefix);
        let net = u32::from(*ip) & mask;
        let broadcast = net | !mask;
        for host in (net + 1)..broadcast {
            let host = Ipv4Addr::from(host);
            if own.contains(&host) {
                continue;
            }
            if seen.insert(host) {
                out.push(host);
            }
        }
    }
    out
}

/// The sweep's address list for this machine right now.
pub(crate) fn sweep_targets(hints: &[Ipv4Addr]) -> Vec<Ipv4Addr> {
    targets(&lan_interfaces(), hints)
}

/// Parse the frontend's remembered addresses, dropping anything that is not a
/// plain IPv4 literal.
pub(crate) fn parse_hints(hints: &[String]) -> Vec<Ipv4Addr> {
    hints.iter().filter_map(|h| h.parse().ok()).collect()
}

/// An unbound-port socket for a sweep. Bound to the wildcard address on
/// purpose: unlike the multicast searches, a unicast datagram's interface is
/// the routing table's to choose, and the routing table is right about it.
pub(crate) async fn socket() -> Option<tokio::net::UdpSocket> {
    tokio::net::UdpSocket::bind("0.0.0.0:0").await.ok()
}

/// Send `payload(host)` to every target on `port`, paced. Send errors are
/// ignored: a host that does not exist is most of the sweep, and some stacks
/// report that per datagram (`EHOSTDOWN`, `ENOBUFS`) rather than silently.
pub(crate) async fn send_all(
    sock: &tokio::net::UdpSocket,
    targets: &[Ipv4Addr],
    port: u16,
    payload: impl Fn(Ipv4Addr) -> Vec<u8>,
) {
    for (i, host) in targets.iter().enumerate() {
        let _ = sock.send_to(&payload(*host), SocketAddr::from((*host, port))).await;
        if (i + 1) % BURST == 0 {
            tokio::time::sleep(BURST_PAUSE).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> Ipv4Addr {
        s.parse().unwrap()
    }

    #[test]
    fn a_24_is_every_host_but_ours_with_hints_first() {
        let t = targets(&[(ip("192.168.1.10"), 24)], &[ip("192.168.1.77"), ip("10.0.0.5")]);
        assert_eq!(t[0], ip("192.168.1.77"));
        assert_eq!(t[1], ip("10.0.0.5"));
        assert_eq!(t.len(), 2 + 254 - 2); // the hint is not repeated, our own address is skipped
        assert!(!t.contains(&ip("192.168.1.10")));
        assert!(!t.contains(&ip("192.168.1.0")));
        assert!(!t.contains(&ip("192.168.1.255")));
    }

    #[test]
    fn a_wide_subnet_is_narrowed_to_our_24() {
        let t = targets(&[(ip("10.1.2.3"), 16)], &[]);
        assert_eq!(t.len(), 253);
        assert!(t.iter().all(|h| h.octets()[..3] == [10, 1, 2]));
    }

    #[test]
    fn a_22_is_swept_whole_and_a_point_to_point_link_not_at_all() {
        assert_eq!(targets(&[(ip("172.16.4.1"), 22)], &[]).len(), 1021);
        assert!(targets(&[(ip("172.16.4.1"), 32)], &[]).is_empty());
    }

    #[test]
    fn our_own_address_is_never_a_hint() {
        let t = targets(&[(ip("192.168.0.2"), 30)], &[ip("192.168.0.2")]);
        assert_eq!(t, vec![ip("192.168.0.1")]);
    }
}
