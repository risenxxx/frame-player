//! Finding a renderer: the SSDP search — multicast from every interface, the
//! unicast sweep beside it — and the picker's discovery round, which reads each
//! LOCATION into a device the moment it is heard.

use std::sync::Mutex;
use std::time::Duration;

use super::control::{http, protocol_info, summarize_sink};
use super::description::{describe, host_of};
use super::{DlnaDeviceInfo, SSDP_ADDR};

/// One SSDP search round, **sent from every LAN interface separately**.
///
/// The obvious version binds `0.0.0.0` and lets the stack pick, which is how
/// this shipped and why it found nothing on Windows: the outgoing interface for
/// a multicast datagram is chosen by the routing table, and a machine with
/// Hyper-V, WSL or a VPN has virtual adapters that win it — the query leaves
/// into a switch nobody is listening on. macOS with one real interface never
/// showed it. `IP_MULTICAST_IF` is the control that actually decides, and
/// neither std nor tokio exposes it, hence socket2.
///
/// Replies are ordinary unicast datagrams back to the sending socket (unlike
/// mDNS, where answers are multicast too), so each interface collects its own.
///
/// A unicast sweep runs beside it — `hints` and every LAN host asked directly
/// (see [`crate::lan_sweep`]) — for the router that drops multicast between the
/// machine's segment and the television's.
///
/// **Each LOCATION goes out on `found` the moment it is heard**, not in a list
/// at the end of the round: what a caller does with one (an HTTP description
/// fetch) must not wait for the slowest part of the search, and a duplicate is
/// the receiver's to drop. Returns when the round is over.
async fn ssdp_search(
    timeout: Duration,
    hints: &[std::net::Ipv4Addr],
    found: tokio::sync::mpsc::UnboundedSender<String>,
) {
    let mut unicast =
        tauri::async_runtime::spawn(ssdp_sweep(timeout, hints.to_vec(), found.clone()));
    let sockets = ssdp_sockets();
    if sockets.is_empty() {
        eprintln!("[dlna] no usable network interface for SSDP");
    }
    if crate::logs::verbose() {
        let names: Vec<String> = sockets.iter().map(|(ip, _)| ip.to_string()).collect();
        eprintln!("[dlna] searching from {}", names.join(", "));
    }

    let mut tasks = Vec::new();
    for (ip, sock) in sockets {
        let found = found.clone();
        tasks.push(tauri::async_runtime::spawn(async move {
            for st in [
                "urn:schemas-upnp-org:device:MediaRenderer:1",
                "upnp:rootdevice",
                "ssdp:all",
            ] {
                let msg = format!(
                    "M-SEARCH * HTTP/1.1\r\nHOST: {SSDP_ADDR}\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: {st}\r\n\r\n"
                );
                if let Err(e) = sock.send_to(msg.as_bytes(), SSDP_ADDR).await {
                    eprintln!("[dlna] M-SEARCH from {ip} failed: {e}");
                    return;
                }
            }
            let mut heard: Vec<String> = Vec::new();
            let mut buf = vec![0u8; 8192];
            let deadline = tokio::time::Instant::now() + timeout;
            loop {
                let left = deadline.saturating_duration_since(tokio::time::Instant::now());
                if left.is_zero() {
                    break;
                }
                let Ok(Ok((n, from))) = tokio::time::timeout(left, sock.recv_from(&mut buf)).await
                else {
                    break;
                };
                let text = String::from_utf8_lossy(&buf[..n]);
                if let Some(loc) = header(&text, "location") {
                    if !heard.contains(&loc) {
                        if crate::logs::verbose() {
                            eprintln!("[dlna] {from} -> {loc}");
                        }
                        heard.push(loc.clone());
                        let _ = found.send(loc);
                    }
                }
            }
        }));
    }
    drop(found);

    for task in tasks {
        let _ = task.await;
    }
    // The sweep is a fallback and must not be able to hold back what the
    // multicast search already heard: when it stalled on a refused send, a
    // renderer that had answered never reached the picker. Its own round is
    // the send plus `timeout`, so twice that is generous.
    if tokio::time::timeout(timeout * 2, &mut unicast).await.is_err() {
        unicast.abort();
        eprintln!("[dlna] unicast sweep overran its round, dropped");
    }
}

/// [`ssdp_search`] for a caller that wants the round's whole list at once.
pub(super) async fn ssdp_locations(timeout: Duration, hints: &[std::net::Ipv4Addr]) -> Vec<String> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    ssdp_search(timeout, hints, tx).await;
    let mut locations: Vec<String> = Vec::new();
    while let Ok(loc) = rx.try_recv() {
        if !locations.contains(&loc) {
            locations.push(loc);
        }
    }
    locations
}

/// Unicast `M-SEARCH` to every sweep target. UPnP 1.1 has a device answer a
/// search addressed to it exactly as it answers the multicast one; a 1.0
/// device may stay silent, which costs nothing but its absence here.
async fn ssdp_sweep(
    timeout: Duration,
    hints: Vec<std::net::Ipv4Addr>,
    found: tokio::sync::mpsc::UnboundedSender<String>,
) {
    let Some(sock) = crate::lan_sweep::socket().await else {
        return;
    };
    let targets = crate::lan_sweep::sweep_targets(&hints);
    crate::lan_sweep::send_all(&sock, &targets, 1900, |host| {
        format!(
            "M-SEARCH * HTTP/1.1\r\nHOST: {host}:1900\r\nMAN: \"ssdp:discover\"\r\nST: urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n"
        )
        .into_bytes()
    })
    .await;
    let mut heard: Vec<String> = Vec::new();
    let mut buf = vec![0u8; 8192];
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            break;
        }
        let Ok(Ok((n, from))) = tokio::time::timeout(left, sock.recv_from(&mut buf)).await else {
            break;
        };
        let text = String::from_utf8_lossy(&buf[..n]);
        if let Some(loc) = header(&text, "location") {
            if !heard.contains(&loc) {
                if crate::logs::verbose() {
                    eprintln!("[dlna] unicast {from} -> {loc}");
                }
                heard.push(loc.clone());
                let _ = found.send(loc);
            }
        }
    }
}

/// One bound socket per usable IPv4 interface: loopback and link-local (APIPA,
/// a Wi-Fi adapter with no lease) are skipped because nothing answers there and
/// each costs the full search timeout.
pub(crate) fn ssdp_sockets() -> Vec<(std::net::Ipv4Addr, tokio::net::UdpSocket)> {
    let mut out = Vec::new();
    for iface in if_addrs::get_if_addrs().unwrap_or_default() {
        let std::net::IpAddr::V4(ip) = iface.ip() else {
            continue;
        };
        if ip.is_loopback() || ip.is_link_local() {
            continue;
        }
        let Ok(sock) = socket2::Socket::new(
            socket2::Domain::IPV4,
            socket2::Type::DGRAM,
            Some(socket2::Protocol::UDP),
        ) else {
            continue;
        };
        let bound = sock
            .set_reuse_address(true)
            .and_then(|()| sock.bind(&std::net::SocketAddrV4::new(ip, 0).into()))
            .and_then(|()| sock.set_multicast_if_v4(&ip))
            .and_then(|()| sock.set_multicast_ttl_v4(4))
            .and_then(|()| sock.set_nonblocking(true));
        if bound.is_err() {
            continue;
        }
        if let Ok(sock) = tokio::net::UdpSocket::from_std(sock.into()) {
            out.push((ip, sock));
        }
    }
    out
}

/// Case-insensitive HTTP-style header lookup, value trimmed.
pub(crate) fn header(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (k, v) = line.split_once(':')?;
        (k.trim().eq_ignore_ascii_case(name)).then(|| v.trim().to_string())
    })
}

/// One round of discovery. Devices that cannot be pushed to are dropped here
/// rather than shown and refused later.
///
/// **Every LOCATION is asked at once, and each renderer reaches `live` the
/// moment it is read.** A round used to be the whole search and then each
/// description and `GetProtocolInfo` in turn, 5 s of timeout apiece, published
/// at the end — so one LOCATION that never answered (a NAS advertising an
/// interface the LAN cannot reach is the measured case; a sleeping device is
/// another) held back every renderer behind it, and the picker restarted the
/// round before it could finish. Now the slowest device costs only itself.
///
/// Returns the round's whole list, which replaces `live` once the round is
/// over: that is how a device that stopped answering leaves.
pub(super) async fn collect_renderers(
    timeout: Duration,
    hints: &[std::net::Ipv4Addr],
    live: &Mutex<Vec<DlnaDeviceInfo>>,
) -> Vec<DlnaDeviceInfo> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let search = ssdp_search(timeout, hints, tx);
    tokio::pin!(search);
    let mut searching = true;
    let mut asked: Vec<String> = Vec::new();
    let mut reads = tokio::task::JoinSet::new();
    let mut out: Vec<DlnaDeviceInfo> = Vec::new();
    loop {
        tokio::select! {
            () = &mut search, if searching => searching = false,
            Some(location) = rx.recv() => {
                if !asked.contains(&location) {
                    asked.push(location.clone());
                    reads.spawn(renderer_at(location));
                }
            }
            Some(read) = reads.join_next(), if !reads.is_empty() => {
                let Ok(Some(device)) = read else { continue };
                // The multicast and the unicast search can both hear one
                // renderer, and a multi-homed one answers from each address.
                if out.iter().any(|d| d.id == device.id) {
                    continue;
                }
                let mut shown = live.lock().unwrap_or_else(|p| p.into_inner());
                match shown.iter_mut().find(|d| d.id == device.id) {
                    Some(old) => *old = device.clone(),
                    None => shown.push(device.clone()),
                }
                drop(shown);
                out.push(device);
            }
            else => break,
        }
    }
    out
}

/// Read one LOCATION into a renderer the picker can offer, or `None` for a
/// device that is unreachable, unreadable or not a renderer.
async fn renderer_at(location: String) -> Option<DlnaDeviceInfo> {
    let client = http();
    let Some(r) = describe(client, &location).await else {
        if crate::logs::verbose() {
            eprintln!("[dlna] {location}: no description");
        }
        return None;
    };
    let control = r.avtransport.clone()?;
    let mimes = match &r.connection_manager {
        Some(cm) => protocol_info(client, cm)
            .await
            .map(|sink| summarize_sink(&sink).keys().cloned().collect())
            .unwrap_or_default(),
        None => Vec::new(),
    };
    let ip = host_of(&location);
    let id = if r.udn.is_empty() { location.clone() } else { r.udn.clone() };
    Some(DlnaDeviceInfo {
        id,
        name: r.friendly_name,
        model: r.model,
        ip,
        control_url: control,
        rendering_url: r.rendering_control,
        mimes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_are_case_insensitive() {
        let reply = "HTTP/1.1 200 OK\r\nLOCATION: http://x/y.xml\r\nST: upnp:rootdevice\r\n";
        assert_eq!(header(reply, "location"), Some("http://x/y.xml".into()));
        assert_eq!(header(reply, "nope"), None);
    }
}
