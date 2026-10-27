//! The discovery round against loopback. A module of its own at
//! `dlna::discovery_tests`, the path the casting rules cite it by.

use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// One LOCATION that never answers must not hold back a renderer that does.
///
/// An SSDP responder in the group on every interface the search uses names
/// two LOCATIONs on loopback: a listener that accepts and never replies,
/// and a renderer that answers at once. Off by default because a round
/// also sweeps the real LAN once:
///
/// `FP_TEST_DLNA_SLOW=1 cargo test --lib dlna::discovery_tests::slow_location_smoke -- --nocapture`
#[tokio::test(flavor = "multi_thread")]
async fn slow_location_smoke() {
    if std::env::var("FP_TEST_DLNA_SLOW").is_err() {
        return;
    }
    // The LOCATION that never answers.
    let mute = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mute_port = mute.local_addr().unwrap().port();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((s, _)) = mute.accept().await {
            held.push(s);
        }
    });
    // The renderer that does.
    let http = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = http.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = http.accept().await {
            tokio::spawn(async move {
                let mut req = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    let Ok(n) = s.read(&mut buf).await else { return };
                    if n == 0 {
                        return;
                    }
                    req.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&req).to_string();
                    let Some(end) = text.find("\r\n\r\n") else { continue };
                    let length = text[..end]
                        .lines()
                        .find_map(|l| {
                            let (k, v) = l.split_once(':')?;
                            k.eq_ignore_ascii_case("content-length").then(|| v.trim().parse().ok())?
                        })
                        .unwrap_or(0usize);
                    if req.len() >= end + 4 + length {
                        break;
                    }
                }
                let body = if req.starts_with(b"GET") {
                    "<?xml version=\"1.0\"?><root xmlns=\"urn:schemas-upnp-org:device-1-0\"><device>\
                     <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>\
                     <friendlyName>Loopback renderer</friendlyName><modelName>Test</modelName>\
                     <UDN>uuid:loopback-renderer</UDN><serviceList>\
                     <service><serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>\
                     <controlURL>/avt</controlURL></service>\
                     <service><serviceType>urn:schemas-upnp-org:service:ConnectionManager:1</serviceType>\
                     <controlURL>/cm</controlURL></service>\
                     </serviceList></device></root>"
                } else {
                    "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\">\
                     <s:Body><u:GetProtocolInfoResponse xmlns:u=\"urn:schemas-upnp-org:service:ConnectionManager:1\">\
                     <Source></Source><Sink>http-get:*:video/x-matroska:*,http-get:*:video/mp4:*</Sink>\
                     </u:GetProtocolInfoResponse></s:Body></s:Envelope>"
                };
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(reply.as_bytes()).await;
            });
        }
    });
    // The SSDP responder: the dead LOCATION first, so a sequential round
    // would sit on it before reaching the live one.
    let ssdp = {
        let sock = socket2::Socket::new(
            socket2::Domain::IPV4,
            socket2::Type::DGRAM,
            Some(socket2::Protocol::UDP),
        )
        .unwrap();
        sock.set_reuse_address(true).unwrap();
        // Unix only: there SO_REUSEADDR alone does not let a second socket
        // bind a multicast port; on Windows it does, and the call does not exist.
        #[cfg(unix)]
        sock.set_reuse_port(true).unwrap();
        sock.bind(&std::net::SocketAddrV4::new(std::net::Ipv4Addr::UNSPECIFIED, 1900).into())
            .unwrap();
        for (ip, _) in ssdp_sockets() {
            let _ = sock.join_multicast_v4(&"239.255.255.250".parse().unwrap(), &ip);
        }
        sock.set_nonblocking(true).unwrap();
        tokio::net::UdpSocket::from_std(sock.into()).unwrap()
    };
    tokio::spawn(async move {
        let mut buf = [0u8; 2048];
        while let Ok((n, from)) = ssdp.recv_from(&mut buf).await {
            if !buf[..n].starts_with(b"M-SEARCH") {
                continue;
            }
            for (location, usn) in [
                (format!("http://127.0.0.1:{mute_port}/desc.xml"), "uuid:loopback-mute"),
                (format!("http://127.0.0.1:{port}/desc.xml"), "uuid:loopback-renderer"),
            ] {
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nEXT:\r\nLOCATION: {location}\r\n\
                     ST: urn:schemas-upnp-org:device:MediaRenderer:1\r\nUSN: {usn}\r\n\r\n"
                );
                let _ = ssdp.send_to(reply.as_bytes(), from).await;
            }
        }
    });

    let live = Mutex::new(Vec::new());
    let start = std::time::Instant::now();
    let ours = |list: &[DlnaDeviceInfo]| list.iter().any(|d| d.id == "loopback-renderer");
    let hints = ["127.0.0.1".parse().unwrap()];
    let (round, shown_after) = tokio::join!(
        collect_renderers(Duration::from_secs(3), &hints, &live),
        async {
            while !ours(&live.lock().unwrap()) {
                if start.elapsed() > Duration::from_secs(20) {
                    return Duration::MAX;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            start.elapsed()
        }
    );
    let round_took = start.elapsed();
    println!("shown after {shown_after:?}, round over after {round_took:?}");
    assert!(ours(&round), "the round lost the renderer");
    let device = round.iter().find(|d| d.id == "loopback-renderer").unwrap();
    assert!(device.mimes.iter().any(|m| m == "video/x-matroska"));
    // The sweep's send is well under a second; the mute LOCATION costs 5 s.
    assert!(shown_after < Duration::from_secs(2), "held back: {shown_after:?}");
}
