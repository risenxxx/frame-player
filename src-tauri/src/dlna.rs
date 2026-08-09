//! DLNA/UPnP AV reconnaissance: is there a MediaRenderer on this network, and
//! what does it say it can play?
//!
//! Why this exists at all, with Cast already shipped: a DLNA renderer is the
//! **TV's own player**, not a browser pipeline, so its format list is the set
//! the television actually decodes — routinely MKV, HEVC and E-AC-3, i.e. the
//! releases our prepare rung exists to repack. And unlike Cast it answers a
//! real capability question: `ConnectionManager::GetProtocolInfo` returns the
//! Sink list, which is the API whose absence forced the per-cell probing in
//! casting.md.
//!
//! **The branch proved out and this is a shipping transport** — the second half
//! of the file is the command surface the frontend drives. What survives from
//! the reconnaissance is the parsing: there is still no XML crate, because what
//! is wanted out of these documents is a dozen leaf elements and a service list,
//! and a real parser is a dependency plus a licensing entry for that. The cost
//! of the decision is paid in [`open_tag_end`] and [`unescape`], which handle
//! the shapes vendors actually emit — attributes, namespace prefixes, empty
//! elements, entities — and in the fixtures under `mod tests`, which are the
//! only place a device we do not own can be reproduced. If the string scanning
//! ever has to grow a third special case, that is the signal to take the
//! dependency instead.
//!
//! Everything here is written for the general renderer rather than for the one
//! television it was measured on: the MIME spelling, the `Seek` unit and the
//! resolution of relative control URLs are all negotiated or derived from what
//! the device said, not assumed.
//!
//! Run it against the LAN with `FP_DLNA_PROBE=1 npm run tauri:macos` — it has to
//! run inside the app rather than from `cargo test`, because on macOS 15+ a
//! process without Local Network permission has its multicast silently dropped
//! and every answer here would be a false negative.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::Duration;

pub(crate) const SSDP_ADDR: &str = "239.255.255.250:1900";
const AVTRANSPORT: &str = "urn:schemas-upnp-org:service:AVTransport:1";
const CONNECTION_MANAGER: &str = "urn:schemas-upnp-org:service:ConnectionManager:1";
const RENDERING_CONTROL: &str = "urn:schemas-upnp-org:service:RenderingControl:1";

#[derive(Debug, Default)]
pub struct Renderer {
    /// The device's own UUID. Not the Cast id of the same television —
    /// measured on the LG, the two are different UUIDs, so the transports of
    /// one physical device cannot be joined by identifier and are merged by
    /// address instead (see the frontend's device list).
    pub udn: String,
    pub location: String,
    pub friendly_name: String,
    pub manufacturer: String,
    pub model: String,
    pub device_type: String,
    /// Absolute control URL of the AVTransport service — the one that takes
    /// `SetAVTransportURI` + `Play`. Absent means "announces itself but cannot
    /// be pushed to", which is the case worth knowing about early.
    pub avtransport: Option<String>,
    pub connection_manager: Option<String>,
    pub rendering_control: Option<String>,
    /// The AVTransport service description, which is where a renderer states
    /// which seek modes it will honour — `Seek` answering OK is not the same
    /// as `Seek` being supported.
    pub avtransport_scpd: Option<String>,
}

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
async fn ssdp_search(timeout: Duration) -> Vec<String> {
    let sockets = ssdp_sockets();
    if sockets.is_empty() {
        eprintln!("[dlna] no usable network interface for SSDP");
        return Vec::new();
    }
    if crate::cast::cast_debug() {
        let names: Vec<String> = sockets.iter().map(|(ip, _)| ip.to_string()).collect();
        eprintln!("[dlna] searching from {}", names.join(", "));
    }

    let mut tasks = Vec::new();
    for (ip, sock) in sockets {
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
                    return Vec::new();
                }
            }
            let mut found: Vec<String> = Vec::new();
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
                    if !found.contains(&loc) {
                        if crate::cast::cast_debug() {
                            eprintln!("[dlna] {from} -> {loc}");
                        }
                        found.push(loc);
                    }
                }
            }
            found
        }));
    }

    let mut locations: Vec<String> = Vec::new();
    for task in tasks {
        for loc in task.await.unwrap_or_default() {
            if !locations.contains(&loc) {
                locations.push(loc);
            }
        }
    }
    locations
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

/// Just past the `>` of the first opening `<name …>`.
///
/// **Matching on the literal `<name>` is what makes a parser vendor-specific.**
/// Three shapes are legal and all of them appear in the field: an attribute
/// (`<friendlyName xml:lang="en">`), a namespace prefix (`<av:CurrentTransportState>`
/// — the UPnP schema says out-arguments are unqualified and not every renderer
/// obeys), and the empty element `<friendlyName/>`. So the tag name is compared
/// as a *local* name against everything up to the first whitespace, and an empty
/// element is skipped rather than mistaken for content.
///
/// The delimiter check is what keeps `deviceType` from answering a search for
/// `device`; the old `format!("<{name}>")` got that right by accident and a
/// bare `starts_with` would not.
fn open_tag_end(xml: &str, name: &str) -> Option<usize> {
    let mut from = 0usize;
    while let Some(rel) = xml[from..].find('<') {
        let lt = from + rel;
        from = lt + 1;
        let Some(gt_rel) = xml[lt + 1..].find('>') else {
            break;
        };
        let gt = lt + 1 + gt_rel;
        let inner = &xml[lt + 1..gt];
        // Closing tags, the declaration, comments and doctypes.
        if inner.starts_with(['/', '?', '!']) {
            continue;
        }
        // An empty element has no content to return; keep looking rather than
        // reporting the tag absent, in case a real one follows.
        if inner.ends_with('/') {
            continue;
        }
        let head = inner.split_whitespace().next().unwrap_or("");
        if head.rsplit(':').next().unwrap_or(head) == name {
            return Some(gt + 1);
        }
    }
    None
}

/// Where the first matching `</name>` begins, by local name.
fn close_tag_start(xml: &str, name: &str) -> Option<usize> {
    let mut from = 0usize;
    while let Some(rel) = xml[from..].find("</") {
        let lt = from + rel;
        from = lt + 2;
        let Some(gt_rel) = xml[lt + 2..].find('>') else {
            break;
        };
        let inner = xml[lt + 2..lt + 2 + gt_rel].trim();
        if inner.rsplit(':').next().unwrap_or(inner) == name {
            return Some(lt);
        }
    }
    None
}

/// The text between the first `<tag>` and its `</tag>`, if any. Still raw —
/// call [`unescape`] on anything destined for a screen.
pub(crate) fn tag<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let start = open_tag_end(xml, name)?;
    let end = close_tag_start(&xml[start..], name)? + start;
    Some(xml[start..end].trim())
}

/// Every `<name …>` in the document, each as a slice running from just past its
/// opening tag to the end. The caller narrows further; this only replaces
/// `split("<name>")`, which cannot see an attribute or a prefix.
pub(crate) fn chunks<'a>(xml: &'a str, name: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = open_tag_end(&xml[from..], name) {
        from += rel;
        out.push(&xml[from..]);
    }
    out
}

/// XML text, decoded. Without this a television called "Living Room &amp; Kitchen"
/// is listed under its markup — the picker is the one place these strings are
/// read by a person.
pub(crate) fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        // Bounded: an unescaped `&` in somebody's device name must not swallow
        // the rest of the string looking for a semicolon that never comes.
        let semi = after.find(';').filter(|&n| n > 0 && n <= 10);
        let decoded = semi.and_then(|n| match &after[..n] {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            num => num
                .strip_prefix('#')
                .and_then(|n| match n.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => n.parse::<u32>().ok(),
                })
                .and_then(char::from_u32),
        });
        match (decoded, semi) {
            (Some(c), Some(n)) => {
                out.push(c);
                rest = &after[n + 1..];
            }
            _ => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Escape for an XML **text node**. Attribute values additionally need `"`,
/// which [`escape_attr`] adds; nothing here may escape more than that, because
/// the result is escaped a second time on its way into `CurrentURIMetaData` and
/// every extra entity survives both rounds.
fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn escape_attr(s: &str) -> String {
    escape_text(s).replace('"', "&quot;")
}

/// One client for every UPnP conversation, built once.
///
/// **reqwest sends no `User-Agent` at all by default** — the trap the tracker
/// announce already paid for, where a WAF answered 403 without one and 200 with
/// a single letter. Renderers are the same kind of surface: some sniff the
/// header, and the UPnP spec asks for `<OS>/<version> UPnP/1.0 <product>/<version>`
/// besides. Sharing one client also keeps the connection pool alive across the
/// poll's several SOAP calls a second, instead of a TCP handshake per action.
pub(crate) fn http() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(format!(
                "{}/1.0 UPnP/1.0 FramePlayer/{}",
                std::env::consts::OS,
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .unwrap_or_default()
    })
}

/// Resolve a description's relative `controlURL` against its base — the
/// device's `URLBase` when it declares one, its LOCATION otherwise.
///
/// **A path-relative URL resolves against the base's directory, not its
/// origin.** This used to force every relative URL to the root, which for a
/// description served from `/dmr/description.xml` turns `upnp/control` into
/// `/upnp/control` — a 404 on any device that writes its control URLs that way.
/// Absolute-path URLs (`/upnp/control/...`) are what most vendors emit and are
/// unaffected; the test below pins both forms.
pub(crate) fn absolute(base: &str, url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        return url.to_string();
    }
    let Some(scheme_end) = base.find("://") else {
        return url.to_string();
    };
    let after = &base[scheme_end + 3..];
    let host_end = after.find('/').map(|i| i + scheme_end + 3).unwrap_or(base.len());
    let origin = &base[..host_end];
    // A scheme-relative URL keeps the base's scheme and nothing else.
    if let Some(rest) = url.strip_prefix("//") {
        return format!("{}//{rest}", &base[..scheme_end + 1]);
    }
    if url.starts_with('/') {
        return format!("{origin}{url}");
    }
    // Everything after the origin, up to and including the last separator.
    let dir_end = base[host_end..]
        .rfind('/')
        .map(|i| host_end + i + 1)
        .unwrap_or(host_end);
    format!("{}{}{url}", &base[..dir_end], if dir_end == host_end { "/" } else { "" })
}

/// The host out of a `http://host:port/path` LOCATION, IPv6 literals included.
///
/// Splitting on `:` and taking the first field reads `http://[fe80::1]:8080/x`
/// as the host `[fe80`, which then fails to parse as an address and reports the
/// device as unreachable rather than as one we do not serve.
pub(crate) fn host_of(location: &str) -> String {
    let Some(after) = location.split("://").nth(1) else {
        return String::new();
    };
    let hostport = after.split('/').next().unwrap_or("");
    match hostport.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or("").to_string(),
        None => hostport.split(':').next().unwrap_or("").to_string(),
    }
}

/// The part of a description that belongs to the MediaRenderer.
///
/// **A description may hold more than one device.** The spec lets a root device
/// embed others in a `<deviceList>`, and combined server+renderer boxes do
/// exactly that — so walking every `<service>` in the document and keeping the
/// last match per type binds our control URLs to whichever device happens to
/// come last. A MediaServer has a ConnectionManager too, and its
/// `GetProtocolInfo` answers with the formats it can *serve*, which is a
/// different question from what the screen can decode.
///
/// A device's own `serviceList` precedes its `deviceList` in a well-formed
/// description, so cutting the chunk at the nested list leaves exactly this
/// device's services and identity.
///
/// Narrowing only happens on a positive identification: with no MediaRenderer
/// anywhere the whole document is returned, which is what this always did.
/// Note that the opposite rule holds in upnp.rs — an InternetGatewayDevice
/// keeps its WAN services two levels down, so there the whole-document walk is
/// the correct reading and must stay.
fn renderer_device(xml: &str) -> &str {
    for chunk in chunks(xml, "device") {
        let own = match chunk.find("<deviceList") {
            Some(i) => &chunk[..i],
            None => chunk,
        };
        if tag(own, "deviceType").is_some_and(|t| t.contains("MediaRenderer")) {
            return own;
        }
    }
    xml
}

/// Read a device description. Kept separate from the fetch so every vendor
/// shape this has to survive can be pinned by a test.
fn parse_description(location: &str, xml: &str) -> Renderer {
    // **`URLBase` outranks the LOCATION for relative URLs.** UPnP 1.1 deprecates
    // it and plenty of shipping devices still send it, sometimes pointing at a
    // different port than the one the description came from — resolve against
    // the location there and every control URL is silently wrong.
    let base = tag(xml, "URLBase")
        .map(unescape)
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| location.to_string());
    let scope = renderer_device(xml);

    let udn = tag(scope, "UDN").unwrap_or("").trim_start_matches("uuid:").to_string();
    let mut r = Renderer {
        udn,
        location: location.to_string(),
        friendly_name: tag(scope, "friendlyName").map(unescape).unwrap_or_else(|| "?".into()),
        manufacturer: tag(scope, "manufacturer").map(unescape).unwrap_or_else(|| "?".into()),
        model: tag(scope, "modelName").map(unescape).unwrap_or_else(|| "?".into()),
        device_type: tag(scope, "deviceType").unwrap_or("?").to_string(),
        ..Default::default()
    };
    let services = tag(scope, "serviceList").unwrap_or(scope);
    for chunk in chunks(services, "service") {
        let Some(service_type) = tag(chunk, "serviceType") else {
            continue;
        };
        let Some(control) = tag(chunk, "controlURL") else {
            continue;
        };
        let url = absolute(&base, &unescape(control));
        // `contains` rather than an equality test on purpose: AVTransport:1,
        // :2 and :3 are all live, and a renderer that answers on a later
        // version is not one we should refuse to talk to.
        if service_type.contains("AVTransport") {
            r.avtransport = Some(url);
            r.avtransport_scpd = tag(chunk, "SCPDURL").map(|u| absolute(&base, &unescape(u)));
        } else if service_type.contains("ConnectionManager") {
            r.connection_manager = Some(url);
        } else if service_type.contains("RenderingControl") {
            r.rendering_control = Some(url);
        }
    }
    r
}

/// Fetch and read a device description.
async fn describe(client: &reqwest::Client, location: &str) -> Option<Renderer> {
    let xml = client
        .get(location)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()?;
    Some(parse_description(location, &xml))
}

/// `GetProtocolInfo` — the renderer's own list of what it will accept, which is
/// the whole reason this transport is interesting. Returns the raw `Sink` value
/// (a long comma-separated list of `http-get:*:<mime>:<flags>`).
async fn protocol_info(client: &reqwest::Client, control_url: &str) -> Option<String> {
    let body = format!(
        r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:GetProtocolInfo xmlns:u="{CONNECTION_MANAGER}"/></s:Body></s:Envelope>"#
    );
    let response = client
        .post(control_url)
        .header("Content-Type", "text/xml; charset=\"utf-8\"")
        .header("SOAPAction", format!("\"{CONNECTION_MANAGER}#GetProtocolInfo\""))
        .timeout(Duration::from_secs(5))
        .body(body)
        .send()
        .await
        .ok()?;
    let text = response.text().await.ok()?;
    tag(&text, "Sink").map(|s| s.to_string())
}

/// One SOAP call against a service, returning the response body.
async fn soap(
    client: &reqwest::Client,
    control_url: &str,
    service: &str,
    action: &str,
    args: &str,
) -> Result<String, String> {
    let body = format!(
        r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:{action} xmlns:u="{service}"><InstanceID>0</InstanceID>{args}</u:{action}></s:Body></s:Envelope>"#
    );
    let response = client
        .post(control_url)
        .header("Content-Type", "text/xml; charset=\"utf-8\"")
        .header("SOAPAction", format!("\"{service}#{action}\""))
        .timeout(Duration::from_secs(8))
        .body(body)
        .send()
        .await
        .map_err(|e| format!("{action}: {e}"))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        // A renderer's refusal is a SOAP fault with a UPnP error code inside,
        // and the code is the whole message: 701 is "transition not available",
        // 714 "illegal MIME type", 716 "resource not found".
        let code = tag(&text, "errorCode").unwrap_or("?");
        let desc = tag(&text, "errorDescription").unwrap_or("");
        return Err(format!("{action}: HTTP {status}, UPnP error {code} {desc}"));
    }
    Ok(text)
}

/// `H:MM:SS.mmm`, the only duration format DIDL-Lite takes.
fn didl_duration(seconds: f64) -> String {
    let total = seconds.max(0.0);
    let h = (total / 3600.0).floor() as u64;
    let m = ((total % 3600.0) / 60.0).floor() as u64;
    let s = total % 60.0;
    format!("{h}:{m:02}:{s:06.3}")
}

/// Duration in seconds, straight from the container header.
fn media_duration(path: &std::path::Path) -> Option<f64> {
    let ictx = ffmpeg_the_third::format::input(path).ok()?;
    let d = ictx.duration();
    (d > 0).then(|| d as f64 / ffmpeg_the_third::ffi::AV_TIME_BASE as f64)
}

/// The DIDL-Lite that goes with the URI — and **this is where a renderer
/// decides whether the stream may be seeked**, before it has fetched a single
/// byte. Measured on the LG: with a bare `<res protocolInfo="http-get:*:mime:*">`
/// the TV's own on-screen transport grays its seek buttons out from the start
/// and answers a sender `Seek` with "not available", no matter what the HTTP
/// responses later advertise. The three things it reads are the DLNA flags in
/// the fourth protocolInfo field, `size`, and `duration`.
///
/// **The escaping is two rounds and the inner one was missing.** The DIDL is a
/// document in its own right, embedded as *text* inside `CurrentURIMetaData`,
/// so a title has to be escaped for the DIDL and the whole DIDL escaped again
/// for the SOAP envelope. Escaping once means the receiver unescapes once and
/// its DIDL parser then meets a bare `&` — which is not valid XML, so any film
/// with an ampersand in its name ("Tom & Jerry" is not an edge case) produced
/// metadata that a strict renderer rejects outright and a lenient one silently
/// mangles. The `<`/`>` half is the same story for a name in angle brackets.
fn didl(url: &str, mime: &str, title: &str, size: u64, duration: Option<f64>) -> String {
    let flags = "DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000";
    let duration_attr = duration
        .map(|d| format!(r#" duration="{}""#, didl_duration(d)))
        .unwrap_or_default();
    let item = format!(
        r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/"><item id="0" parentID="-1" restricted="1"><dc:title>{}</dc:title><upnp:class>object.item.videoItem</upnp:class><res protocolInfo="{}" size="{size}"{duration_attr}>{}</res></item></DIDL-Lite>"#,
        escape_text(title),
        escape_attr(&format!("http-get:*:{mime}:{flags}")),
        escape_text(url),
    );
    escape_text(&item)
}

/// Print what the network has. Called at startup only under `FP_DLNA_PROBE=1`.
pub async fn probe() {
    eprintln!("[dlna] searching for UPnP devices…");
    let locations = ssdp_search(Duration::from_secs(6)).await;
    if locations.is_empty() {
        eprintln!("[dlna] nothing answered M-SEARCH — no UPnP devices, or multicast is being dropped");
        return;
    }

    let client = http();
    let mut renderers = 0;
    for location in &locations {
        let Some(r) = describe(client, location).await else {
            eprintln!("[dlna] {location}: description unreadable");
            continue;
        };
        eprintln!(
            "[dlna] {} — {} {} ({}) at {}",
            r.friendly_name, r.manufacturer, r.model, r.device_type, r.location
        );
        eprintln!(
            "[dlna]     AVTransport: {}",
            r.avtransport.as_deref().unwrap_or("(none — cannot be pushed to)")
        );
        if r.avtransport.is_some() {
            renderers += 1;
        }
        if let Some(cm) = &r.connection_manager {
            match protocol_info(client, cm).await {
                Some(sink) => {
                    let formats = summarize_sink(&sink);
                    eprintln!("[dlna]     accepts {} distinct MIME types:", formats.len());
                    for (mime, count) in formats {
                        eprintln!("[dlna]       {mime} ({count} profile(s))");
                    }
                }
                None => eprintln!("[dlna]     GetProtocolInfo: no answer"),
            }
        }
    }
    eprintln!("[dlna] done: {} renderer(s) that accept a pushed URL", renderers);
}

/// The Sink list is hundreds of `http-get:*:<mime>:DLNA.ORG_PN=...` entries;
/// what matters for the ladder is which MIME types appear at all.
fn summarize_sink(sink: &str) -> BTreeMap<String, usize> {
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    for entry in sink.split(',') {
        let parts: Vec<&str> = entry.split(':').collect();
        if parts.len() >= 3 && !parts[2].is_empty() {
            *out.entry(parts[2].to_string()).or_default() += 1;
        }
    }
    out
}

/// The MIME spellings one container is advertised under, in the order we would
/// rather send them.
///
/// **There is no single right answer per container, and the device is the one
/// who decides.** Renderers announce what they take in `GetProtocolInfo`, and
/// the spellings differ by vendor — Matroska appears as `video/x-matroska` and
/// as `video/x-mkv`, AVI has four live forms, MPEG-TS three. Sending a name the
/// device does not list earns UPnP 714 "illegal MIME type" from a strict one.
///
/// Kept in step with `DLNA_MIME` in `src/lib/cast.svelte.ts`, which answers the
/// neighbouring question — whether to *offer* the device at all. A divergence
/// costs a worse guess rather than a failure: [`negotiate_mime`] falls back to
/// the first spelling here, so the frontend saying yes and this table saying
/// nothing can no longer produce `application/octet-stream` on the wire.
fn mime_candidates(ext: &str) -> &'static [&'static str] {
    match ext {
        "mkv" => &["video/x-matroska", "video/x-mkv", "video/mkv"],
        "mp4" | "m4v" => &["video/mp4"],
        "mov" => &["video/quicktime", "video/mp4"],
        "avi" => &["video/avi", "video/x-msvideo", "video/x-ms-avi", "video/msvideo"],
        "ts" | "m2ts" | "mts" => &[
            "video/mp2t",
            "video/mp2ts",
            "video/vnd.dlna.mpeg-tts",
            "video/x-mpegts",
        ],
        "mpg" | "mpeg" | "mpe" | "vob" => &["video/mpeg"],
        "webm" => &["video/webm"],
        "wmv" => &["video/x-ms-wmv"],
        "asf" => &["video/x-ms-asf"],
        "flv" => &["video/x-flv"],
        "3gp" => &["video/3gpp"],
        "ogv" => &["video/ogg"],
        "mp3" => &["audio/mpeg"],
        "m4a" | "aac" => &["audio/mp4", "audio/aac"],
        "flac" => &["audio/flac", "audio/x-flac"],
        "ogg" | "oga" | "opus" => &["audio/ogg"],
        "wav" => &["audio/wav", "audio/x-wav"],
        _ => &[],
    }
}

/// The extension of whatever the URL or path ends in, lower-cased, with any
/// query or fragment removed.
fn extension_of(source: &str) -> String {
    let path = source.split(['?', '#']).next().unwrap_or(source);
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() && ext.len() <= 5 => ext.to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// The spelling to put on the wire: what the renderer advertised if it
/// advertised any of them, our own preference otherwise.
///
/// The fallback is the interesting half. `cast_mime` answers
/// `application/octet-stream` for every container Cast never carries — AVI,
/// MPEG-TS, WMV, MPEG — while the frontend was perfectly willing to offer such
/// a file to a renderer that lists `video/x-msvideo`. So the load went out
/// naming a type no device accepts, and the failure looked like the television
/// refusing the file.
fn negotiate_mime(advertised: &[String], ext: &str, wanted: &str) -> String {
    let candidates = mime_candidates(ext);
    if let Some(m) = candidates
        .iter()
        .find(|c| advertised.iter().any(|a| a.eq_ignore_ascii_case(c)))
    {
        return (*m).to_string();
    }
    if candidates.is_empty() || candidates.iter().any(|c| c.eq_ignore_ascii_case(wanted)) {
        return wanted.to_string();
    }
    candidates[0].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_urls_resolve_against_the_location() {
        let loc = "http://192.0.2.48:9197/dmr/description.xml";
        assert_eq!(absolute(loc, "/upnp/control/AVTransport1"), "http://192.0.2.48:9197/upnp/control/AVTransport1");
        // Path-relative resolves against the description's *directory*. This
        // case used to assert `/upnp/control`, which is what the code did and
        // not what RFC 3986 says — a device writing its control URLs this way
        // was being sent to a path that does not exist. The test documented the
        // bug rather than the contract.
        assert_eq!(absolute(loc, "upnp/control"), "http://192.0.2.48:9197/dmr/upnp/control");
        assert_eq!(absolute("http://192.0.2.48:9197/", "ctl"), "http://192.0.2.48:9197/ctl");
        assert_eq!(absolute("http://192.0.2.48:9197", "ctl"), "http://192.0.2.48:9197/ctl");
        assert_eq!(absolute(loc, "//other:80/x"), "http://other:80/x");
        assert_eq!(absolute(loc, "http://other/x"), "http://other/x");
    }

    #[test]
    fn tags_survive_attributes_prefixes_and_empty_elements() {
        // Every one of these is legal and at least one vendor emits it.
        assert_eq!(tag(r#"<friendlyName xml:lang="en">TV</friendlyName>"#, "friendlyName"), Some("TV"));
        assert_eq!(tag("<av:CurrentTransportState>PLAYING</av:CurrentTransportState>", "CurrentTransportState"), Some("PLAYING"));
        assert_eq!(tag("<x>a</x><Sink>b</Sink>", "Sink"), Some("b"));
        // An empty element carries nothing; the next real one wins.
        assert_eq!(tag("<UDN/><UDN>uuid:1</UDN>", "UDN"), Some("uuid:1"));
        assert_eq!(tag("<UDN/>", "UDN"), None);
        // The delimiter check: a longer name must not answer a shorter search.
        assert_eq!(tag("<deviceType>x</deviceType>", "device"), None);
        assert_eq!(tag("<modelNumber>1</modelNumber>", "modelName"), None);
        // Declarations, comments and doctypes are not elements.
        assert_eq!(tag(r#"<?xml version="1.0"?><!-- c --><name>n</name>"#, "name"), Some("n"));
        // A prefixed close tag against an unprefixed open one, and vice versa.
        assert_eq!(tag("<name>n</u:name>", "name"), Some("n"));
    }

    #[test]
    fn chunks_are_one_per_opening_tag() {
        let xml = "<service><a>1</a></service><service ><a>2</a></service>";
        let c = chunks(xml, "service");
        assert_eq!(c.len(), 2);
        assert_eq!(tag(c[0], "a"), Some("1"));
        assert_eq!(tag(c[1], "a"), Some("2"));
        assert!(chunks("<serviceList></serviceList>", "service").is_empty());
    }

    #[test]
    fn entities_are_decoded() {
        assert_eq!(unescape("Living Room &amp; Kitchen"), "Living Room & Kitchen");
        assert_eq!(unescape("&lt;a&gt; &quot;b&quot; &apos;c&apos;"), "<a> \"b\" 'c'");
        assert_eq!(unescape("&#65;&#x42;"), "AB");
        assert_eq!(unescape("plain"), "plain");
        // An unescaped ampersand is left alone rather than eating the rest.
        assert_eq!(unescape("R&D and more"), "R&D and more");
        assert_eq!(unescape("&notanentity;"), "&notanentity;");
    }

    /// A combined MediaServer + MediaRenderer, which is what the whole-document
    /// walk got wrong: the server's ConnectionManager came last and won.
    const COMBINED: &str = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <URLBase>http://192.0.2.9:2870/</URLBase>
  <device>
    <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
    <friendlyName>Salon &amp; Kitchen</friendlyName>
    <manufacturer>ACME</manufacturer>
    <modelName>Q80</modelName>
    <UDN>uuid:renderer-1</UDN>
    <serviceList>
      <service>
        <serviceType>urn:schemas-upnp-org:service:AVTransport:3</serviceType>
        <controlURL>/rcr/AVTransport/ctl</controlURL>
        <SCPDURL>/rcr/AVTransport/scpd.xml</SCPDURL>
      </service>
      <service>
        <serviceType>urn:schemas-upnp-org:service:ConnectionManager:1</serviceType>
        <controlURL>/rcr/CM/ctl</controlURL>
      </service>
    </serviceList>
    <deviceList>
      <device>
        <deviceType>urn:schemas-upnp-org:device:MediaServer:1</deviceType>
        <UDN>uuid:server-1</UDN>
        <serviceList>
          <service>
            <serviceType>urn:schemas-upnp-org:service:ConnectionManager:1</serviceType>
            <controlURL>/srv/CM/ctl</controlURL>
          </service>
        </serviceList>
      </device>
    </deviceList>
  </device>
</root>"#;

    #[test]
    fn services_come_from_the_renderer_not_its_embedded_server() {
        let r = parse_description("http://192.0.2.9:9197/desc.xml", COMBINED);
        assert_eq!(r.udn, "renderer-1");
        // Decoded, not shown as markup.
        assert_eq!(r.friendly_name, "Salon & Kitchen");
        // URLBase outranks the LOCATION: note the port is 2870, not 9197.
        assert_eq!(r.avtransport.as_deref(), Some("http://192.0.2.9:2870/rcr/AVTransport/ctl"));
        assert_eq!(r.connection_manager.as_deref(), Some("http://192.0.2.9:2870/rcr/CM/ctl"));
        assert_eq!(r.avtransport_scpd.as_deref(), Some("http://192.0.2.9:2870/rcr/AVTransport/scpd.xml"));
    }

    #[test]
    fn a_plain_renderer_is_unaffected_by_the_narrowing() {
        let xml = r#"<root><device>
            <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
            <friendlyName>Plain</friendlyName><UDN>uuid:p</UDN>
            <serviceList><service>
              <serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>
              <controlURL>/AVTransport/control</controlURL>
            </service></serviceList></device></root>"#;
        let r = parse_description("http://192.0.2.5:7676/dmr", xml);
        assert_eq!(r.avtransport.as_deref(), Some("http://192.0.2.5:7676/AVTransport/control"));
    }

    /// The shape the narrowing actually exists for: the **root** device is the
    /// MediaServer and the renderer is embedded under it. Reading the document
    /// as a whole takes the server's identity and the server's `serviceList`,
    /// which has no AVTransport in it — so `collect_renderers` drops the
    /// television entirely and it never reaches the picker.
    ///
    /// Worth knowing why the `COMBINED` fixture above does not catch this on its
    /// own: there the renderer comes first, so the first `<serviceList>` in the
    /// document is already the right one and the bug hides. Order is the whole
    /// difference, which is exactly the kind of thing one device vendor gets
    /// right and the next does not.
    #[test]
    fn a_renderer_embedded_under_a_server_is_still_found() {
        let xml = r#"<root><device>
            <deviceType>urn:schemas-upnp-org:device:MediaServer:1</deviceType>
            <friendlyName>Box (server)</friendlyName><UDN>uuid:server-1</UDN>
            <serviceList><service>
              <serviceType>urn:schemas-upnp-org:service:ContentDirectory:1</serviceType>
              <controlURL>/srv/CD/ctl</controlURL>
            </service></serviceList>
            <deviceList><device>
              <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
              <friendlyName>Box (screen)</friendlyName><UDN>uuid:renderer-2</UDN>
              <serviceList><service>
                <serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>
                <controlURL>/rcr/AVT/ctl</controlURL>
              </service></serviceList>
            </device></deviceList></device></root>"#;
        let r = parse_description("http://192.0.2.7:80/d.xml", xml);
        assert_eq!(r.avtransport.as_deref(), Some("http://192.0.2.7:80/rcr/AVT/ctl"));
        assert_eq!(r.udn, "renderer-2");
        assert_eq!(r.friendly_name, "Box (screen)");
        assert!(r.device_type.contains("MediaRenderer"), "{}", r.device_type);
    }

    /// No device declares itself a MediaRenderer: the whole document stays in
    /// scope, which is what this did before the narrowing existed. Being wrong
    /// in the other direction would silently drop such a device from the picker.
    #[test]
    fn an_unrecognised_device_keeps_the_old_whole_document_scope() {
        let xml = r#"<root><device>
            <deviceType>urn:vendor-com:device:Box:1</deviceType>
            <UDN>uuid:v</UDN>
            <deviceList><device><serviceList><service>
              <serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>
              <controlURL>/x/ctl</controlURL>
            </service></serviceList></device></deviceList></device></root>"#;
        let r = parse_description("http://192.0.2.6:80/d.xml", xml);
        assert_eq!(r.avtransport.as_deref(), Some("http://192.0.2.6:80/x/ctl"));
    }

    #[test]
    fn hosts_survive_ipv6_literals() {
        assert_eq!(host_of("http://192.0.2.48:9197/dmr/d.xml"), "192.0.2.48");
        assert_eq!(host_of("http://192.0.2.48/d.xml"), "192.0.2.48");
        assert_eq!(host_of("http://[2001:db8::1]:8080/d.xml"), "2001:db8::1");
        assert_eq!(host_of("nonsense"), "");
    }

    #[test]
    fn mime_follows_what_the_renderer_advertises() {
        let advertised: Vec<String> = ["video/x-mkv", "video/x-msvideo", "video/mp4"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // The device's own spelling wins over our preferred one.
        assert_eq!(negotiate_mime(&advertised, "mkv", "video/x-matroska"), "video/x-mkv");
        // The regression this exists for: AVI reached the wire as octet-stream.
        assert_eq!(negotiate_mime(&advertised, "avi", "application/octet-stream"), "video/x-msvideo");
        // Nothing advertised for the container: our first spelling, never the
        // octet-stream fallback, which no renderer accepts.
        assert_eq!(negotiate_mime(&[], "avi", "application/octet-stream"), "video/avi");
        // A container we have no opinion about keeps the caller's answer.
        assert_eq!(negotiate_mime(&[], "xyz", "application/octet-stream"), "application/octet-stream");
        // Case is not a difference.
        let shouty = vec!["VIDEO/X-MATROSKA".to_string()];
        assert_eq!(negotiate_mime(&shouty, "mkv", "video/x-matroska"), "video/x-matroska");
    }

    #[test]
    fn extensions_ignore_the_query_and_the_directory() {
        assert_eq!(extension_of("/a/b/The.Movie.2024.MKV"), "mkv");
        assert_eq!(extension_of("http://h:1/c/tok/ep1.mkv"), "mkv");
        assert_eq!(extension_of("http://h:1/c/tok/ep1.mkv?x=1#y"), "mkv");
        assert_eq!(extension_of(r"E:\Films\a.avi"), "avi");
        assert_eq!(extension_of("/a/no-extension"), "");
        // A dotted directory name must not be read as an extension.
        assert_eq!(extension_of("/a.b/movie"), "");
    }

    #[test]
    fn didl_escapes_twice_so_the_receiver_sees_valid_xml() {
        let x = didl("http://h/f.mkv", "video/x-matroska", "Tom & Jerry <2>", 42, None);
        // What the receiver un-escapes once and hands to its DIDL parser.
        let inner = x.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&");
        assert!(inner.contains("<dc:title>Tom &amp; Jerry &lt;2&gt;</dc:title>"), "{inner}");
        // Un-escaping a second time, as the DIDL parser does, gives the name back.
        let title = inner.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&");
        assert!(title.contains("<dc:title>Tom & Jerry <2></dc:title>"), "{title}");
    }

    #[test]
    fn sink_is_summarised_by_mime() {
        let sink = "http-get:*:video/mp4:DLNA.ORG_PN=AVC_MP4_MP_HD,\
                    http-get:*:video/mp4:DLNA.ORG_PN=AVC_MP4_HP_HD,\
                    http-get:*:video/x-matroska:*";
        let s = summarize_sink(sink);
        assert_eq!(s.get("video/mp4"), Some(&2));
        assert_eq!(s.get("video/x-matroska"), Some(&1));
    }

    #[test]
    fn didl_durations_are_h_mm_ss() {
        assert_eq!(didl_duration(0.0), "0:00:00.000");
        assert_eq!(didl_duration(3415.92), "0:56:55.920");
        assert_eq!(didl_duration(7265.5), "2:01:05.500");
    }

    #[test]
    fn didl_res_declares_seekability() {
        // The three things the renderer reads before it fetches anything.
        let x = didl("http://h/f.mkv", "video/x-matroska", "t", 42, Some(60.0));
        assert!(x.contains("DLNA.ORG_OP=01"));
        assert!(x.contains(r#"size=&quot;42&quot;"#) || x.contains("size=\"42\"") || x.contains("size="));
        assert!(x.contains("0:01:00.000"));
    }

    #[test]
    fn headers_are_case_insensitive() {
        let reply = "HTTP/1.1 200 OK\r\nLOCATION: http://x/y.xml\r\nST: upnp:rootdevice\r\n";
        assert_eq!(header(reply, "location"), Some("http://x/y.xml".into()));
        assert_eq!(header(reply, "nope"), None);
    }
}

// ---- The transport ---------------------------------------------------------
//
// DLNA is the third way this player reaches a television, and the one that
// asks the least of us: the renderer is the TV's **own** player, so a release
// it already decodes — measured: MKV, 4K HEVC Main-10 HDR10, E-AC-3 5.1 —
// plays untouched, with seeking, from the same Range server the Cast path uses.
// What it costs is that every device answers differently; hence the ladder is
// read from `GetProtocolInfo` rather than assumed.
//
// The command surface deliberately mirrors cast.rs (`*_connect`, `*_load`,
// `*_status`, `*_control`, `*_disconnect`) and reports the same status shape,
// so the frontend keeps one store and one casting screen and only chooses which
// command name to call.

/// One renderer as the picker shows it, and as `dlna_connect` takes it back.
#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
pub struct DlnaDeviceInfo {
    pub id: String,
    pub name: String,
    pub model: String,
    pub ip: String,
    pub control_url: String,
    pub rendering_url: Option<String>,
    /// MIME types the renderer says it accepts — the capability answer Cast
    /// never gives, and what the compatibility line in the picker is built on.
    pub mimes: Vec<String>,
}

#[derive(Default)]
struct DlnaState {
    /// Same vocabulary as cast.rs: connecting | loading | playing | paused |
    /// buffering | ended | stopped | error.
    state: String,
    error: Option<String>,
    time: f64,
    reported_at: Option<std::time::Instant>,
    duration: f64,
    volume: f64,
    volume_known: bool,
    /// Whether a non-zero level was ever reported — see the poll for why a
    /// perpetual zero is read as "no volume control here" rather than "muted".
    saw_volume: bool,
    /// A command was just sent and its optimistic state must survive the
    /// renderer's lagging reports until then. **This is the pause bug**: the LG
    /// answers `Pause` with a proper response and then keeps saying `PLAYING`
    /// for a poll or two (after a load it says `TRANSITIONING` for six), so the
    /// next status overwrote the optimism and the button sprang back — the same
    /// shape as the seekbar's `seekSettling`, and the same answer.
    settle_until: Option<std::time::Instant>,
    /// Where a seek was aimed while the renderer was paused, and until when to
    /// trust that over the renderer's own answer. **Measured: a paused renderer
    /// keeps reporting the position it had before the seek** until playback
    /// resumes, so believing it would drag the knob back to where the film was
    /// — the same "jumps and returns" the chapter list used to show, from the
    /// other end.
    seek_target: Option<(f64, std::time::Instant)>,
    /// Which `Seek` unit this renderer answered to, once one has. `None` means
    /// the ladder in [`seek_to`] has not been walked yet.
    seek_unit: Option<&'static str>,
    /// Whether the renderer has ever reported `PLAYING` this session. Until it
    /// has, a `TransportStatus` of `ERROR_OCCURRED` is a failed load; after it
    /// has, it is noise from a device that recovered.
    saw_playing: bool,
}

/// How long an optimistic state outranks the renderer's own report. Long
/// enough to cover this TV's observed lag, short enough that a command the
/// device actually refused shows up as one flicker rather than a stuck button.
const SETTLE: Duration = Duration::from_millis(2500);

/// How close to the duration counts as "the film finished" rather than "someone
/// pressed stop". A renderer reports the position it last decoded, which lags
/// the end by a beat, and the last seconds of a file are credits nobody stops
/// deliberately.
const END_SLACK: f64 = 8.0;

/// How long a paused seek's target outranks the renderer's own position. Long
/// enough to cover a viewer studying the frame before pressing play, bounded so
/// a renderer that never catches up cannot freeze the knob for good.
const PAUSED_SEEK_HOLD: Duration = Duration::from_secs(120);

/// How long to keep asking a renderer to start before calling the load failed.
/// Deliberately bounded well inside the frontend's own "fetched and never
/// played" timeout: once the television is preparing, judging whether anything
/// ever appears is that detector's job and not this one's.
const PLAY_BUDGET: Duration = Duration::from_secs(12);

/// Between `Play` attempts. A renderer that refuses because it is still
/// bringing its player up answers at once, so this pause is the whole cost of
/// a retry.
const PLAY_RETRY: Duration = Duration::from_millis(1200);

struct DlnaSession {
    device: DlnaDeviceInfo,
    state: Arc<Mutex<DlnaState>>,
    poll: tauri::async_runtime::JoinHandle<()>,
}

#[derive(Default)]
struct DlnaInner {
    /// Discovery runs only while the picker is open, exactly like the mDNS
    /// browse: nothing about this app is on the network until asked.
    discovery: Option<(tauri::async_runtime::JoinHandle<()>, Arc<Mutex<Vec<DlnaDeviceInfo>>>)>,
    session: Option<DlnaSession>,
}

#[derive(Default)]
pub struct DlnaService {
    inner: Mutex<DlnaInner>,
}

use std::sync::{Arc, Mutex};

/// Claim a state now and hold it against the renderer's lagging reports;
/// returns what it was, so a refused command can put it back.
fn arm(state: &Arc<Mutex<DlnaState>>, next: &str) -> String {
    let mut guard = state.lock().unwrap_or_else(|p| p.into_inner());
    let previous = guard.state.clone();
    guard.state = next.to_string();
    guard.settle_until = Some(std::time::Instant::now() + SETTLE);
    previous
}

fn set_state(state: &Arc<Mutex<DlnaState>>, f: impl FnOnce(&mut DlnaState)) {
    let mut guard = state.lock().unwrap_or_else(|p| p.into_inner());
    f(&mut guard);
}

/// `H:MM:SS` (what AVTransport speaks) from seconds, and back.
fn hms(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    format!("{}:{:02}:{:02}", total / 3600, (total % 3600) / 60, total % 60)
}

fn parse_hms(text: &str) -> Option<f64> {
    let mut secs = 0.0;
    for part in text.trim().split(':') {
        secs = secs * 60.0 + part.parse::<f64>().ok()?;
    }
    Some(secs)
}

/// The `Seek` units worth trying, in order of preference.
///
/// **`REL_TIME` is not universal.** It is what the measured television takes
/// and what most renderers declare, but `ABS_TIME` is the only mode some
/// devices implement — and a renderer that declares both may still refuse one
/// of them, which is why this is a runtime ladder rather than a reading of the
/// SCPD's `A_ARG_TYPE_SeekMode` list. The byte modes are deliberately absent:
/// `X_DLNA_REL_BYTE` would need an offset we cannot compute from a timestamp
/// without the container index the television is holding.
const SEEK_UNITS: [&str; 2] = ["REL_TIME", "ABS_TIME"];

/// Seek, learning which unit this device speaks and then keeping to it.
///
/// The cost of the ladder is one extra round trip on the first seek of a
/// session, and only on a device that refuses the preferred unit; the cost of
/// not having it is a seekbar that silently does nothing on every renderer
/// outside the one this was measured against.
async fn seek_to(
    client: &reqwest::Client,
    control: &str,
    state: &Arc<Mutex<DlnaState>>,
    target: f64,
) -> Result<(), String> {
    let known = state.lock().unwrap_or_else(|p| p.into_inner()).seek_unit;
    let units: Vec<&'static str> = match known {
        Some(u) => vec![u],
        None => SEEK_UNITS.to_vec(),
    };
    let mut last = None;
    for unit in units {
        let args = format!("<Unit>{unit}</Unit><Target>{}</Target>", hms(target));
        match soap(client, control, AVTRANSPORT, "Seek", &args).await {
            Ok(_) => {
                set_state(state, |s| s.seek_unit = Some(unit));
                return Ok(());
            }
            Err(e) => {
                eprintln!("[dlna] {e}");
                last = Some(e);
            }
        }
    }
    Err(last.unwrap_or_else(|| "Seek: no unit accepted".into()))
}

/// Collect renderers once. Devices that cannot be pushed to are dropped here
/// rather than shown and refused later.
async fn collect_renderers(timeout: Duration) -> Vec<DlnaDeviceInfo> {
    let client = http();
    let mut out = Vec::new();
    for location in ssdp_search(timeout).await {
        let Some(r) = describe(client, &location).await else {
            continue;
        };
        let Some(control) = r.avtransport.clone() else {
            continue;
        };
        let mimes = match &r.connection_manager {
            Some(cm) => protocol_info(client, cm)
                .await
                .map(|sink| summarize_sink(&sink).keys().cloned().collect())
                .unwrap_or_default(),
            None => Vec::new(),
        };
        let ip = host_of(&location);
        out.push(DlnaDeviceInfo {
            id: if r.udn.is_empty() { location.clone() } else { r.udn.clone() },
            name: r.friendly_name,
            model: r.model,
            ip,
            control_url: control,
            rendering_url: r.rendering_control,
            mimes,
        });
    }
    out
}

#[tauri::command]
pub fn dlna_discover_start(service: tauri::State<'_, Arc<DlnaService>>) -> Result<(), String> {
    let mut inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
    if inner.discovery.is_some() {
        return Ok(());
    }
    let list: Arc<Mutex<Vec<DlnaDeviceInfo>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = list.clone();
    // Re-searched rather than searched once: SSDP replies are UDP and a device
    // that missed the first M-SEARCH would otherwise never appear.
    let task = tauri::async_runtime::spawn(async move {
        loop {
            let found = collect_renderers(Duration::from_secs(3)).await;
            if !found.is_empty() {
                *sink.lock().unwrap_or_else(|p| p.into_inner()) = found;
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
    inner.discovery = Some((task, list));
    Ok(())
}

#[tauri::command]
pub fn dlna_discover_stop(service: tauri::State<'_, Arc<DlnaService>>) {
    let mut inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
    if let Some((task, _)) = inner.discovery.take() {
        task.abort();
    }
}

#[tauri::command]
pub fn dlna_devices(service: tauri::State<'_, Arc<DlnaService>>) -> Vec<DlnaDeviceInfo> {
    let inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
    inner
        .discovery
        .as_ref()
        .map(|(_, list)| list.lock().unwrap_or_else(|p| p.into_inner()).clone())
        .unwrap_or_default()
}

/// Same shape as `CastStatus` — the frontend reads one store whichever
/// transport is live.
#[derive(serde::Serialize, Default)]
pub struct DlnaStatus {
    pub state: String,
    pub error: Option<String>,
    pub time: f64,
    pub duration: f64,
    pub volume: f64,
    pub muted: bool,
    pub volume_known: bool,
    pub volume_fixed: bool,
    pub fetches: u64,
    pub device: Option<String>,
}

#[tauri::command]
pub fn dlna_connect(
    service: tauri::State<'_, Arc<DlnaService>>,
    device: DlnaDeviceInfo,
) -> Result<(), String> {
    let mut inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(old) = inner.session.take() {
        old.poll.abort();
    }
    let state = Arc::new(Mutex::new(DlnaState {
        state: "connecting".into(),
        volume: 1.0,
        ..Default::default()
    }));
    let poll_state = state.clone();
    let control = device.control_url.clone();
    let rendering = device.rendering_url.clone();
    // The renderer is polled rather than subscribed to: GENA eventing would
    // mean running a callback HTTP server and renewing subscriptions, for a
    // position that arrives once a second either way.
    let poll = tauri::async_runtime::spawn(async move {
        let client = http();
        loop {
            tokio::time::sleep(Duration::from_millis(900)).await;
            let loaded = {
                let s = poll_state.lock().unwrap_or_else(|p| p.into_inner());
                !matches!(s.state.as_str(), "connecting" | "stopped" | "error")
            };
            if !loaded {
                continue;
            }
            if let Ok(info) = soap(client, &control, AVTRANSPORT, "GetTransportInfo", "").await {
                if crate::cast::cast_debug() {
                    eprintln!("[dlna] GetTransportInfo -> {}", info.replace('\n', " "));
                }
                let transport = tag(&info, "CurrentTransportState").unwrap_or("").to_string();
                // **A renderer that failed says so, and we were not listening.**
                // `CurrentTransportStatus` goes to `ERROR_OCCURRED` when the
                // device gave up on the stream — which it reports in the same
                // breath as a transport state of `STOPPED` or even `PLAYING`,
                // so the state alone cannot tell a failure from somebody
                // pressing stop. Read before the first `PLAYING` it is a load
                // that died, and saying so beats waiting out the frontend's
                // "fetched and never played" timeout; read afterwards it is a
                // device that stumbled and recovered, and acting on it would
                // tear down a session that is still watchable.
                let failed = tag(&info, "CurrentTransportStatus")
                    .is_some_and(|s| s.eq_ignore_ascii_case("ERROR_OCCURRED"));
                set_state(&poll_state, |s| {
                    if transport == "PLAYING" {
                        s.saw_playing = true;
                    }
                    if failed && !s.saw_playing {
                        s.state = "error".into();
                        s.error = Some("load_failed".into());
                        s.settle_until = None;
                        return;
                    }
                    if s.settle_until.map(|t| t > std::time::Instant::now()).unwrap_or(false) {
                        return;
                    }
                    s.settle_until = None;
                    s.state = match transport.as_str() {
                        "PLAYING" => "playing".into(),
                        "PAUSED_PLAYBACK" | "PAUSED_RECORDING" => "paused".into(),
                        "TRANSITIONING" => "buffering".into(),
                        // A renderer that reaches the end simply stops, so
                        // "finished" and "stopped from the TV" arrive as one
                        // state — and they must not stay one, or pressing stop
                        // on the remote would advance the queue to the next
                        // episode. The position decides: within the last few
                        // seconds of a known duration it is the end of a film,
                        // anything earlier is somebody stopping it.
                        "STOPPED" | "NO_MEDIA_PRESENT" => {
                            let finished = s.duration > 0.0 && s.time >= s.duration - END_SLACK;
                            if finished { "ended".into() } else { "stopped".into() }
                        }
                        _ => s.state.clone(),
                    };
                });
            }
            if let Ok(pos) = soap(client, &control, AVTRANSPORT, "GetPositionInfo", "").await {
                // **`RelTime` is optional and plenty of renderers answer it with
                // the literal `NOT_IMPLEMENTED`**, which parses as nothing and
                // leaves the seekbar frozen at zero for the whole film. `AbsTime`
                // carries the same number on a single-track stream, so it is the
                // fallback rather than a second feature.
                let time = tag(&pos, "RelTime")
                    .and_then(parse_hms)
                    .or_else(|| tag(&pos, "AbsTime").and_then(parse_hms));
                let dur = tag(&pos, "TrackDuration").and_then(parse_hms);
                set_state(&poll_state, |s| {
                    if let Some(t) = time {
                        // A pending paused seek outranks a report that has not
                        // caught up with it; the moment one lands near the
                        // target — or the deadline passes — the renderer is
                        // believed again.
                        let stale = match s.seek_target {
                            Some((target, until)) => {
                                if (t - target).abs() < 3.0 || std::time::Instant::now() > until {
                                    s.seek_target = None;
                                    false
                                } else {
                                    true
                                }
                            }
                            None => false,
                        };
                        if !stale {
                            s.time = t;
                            s.reported_at = Some(std::time::Instant::now());
                        }
                    }
                    if let Some(d) = dur {
                        if d > 0.0 {
                            s.duration = d;
                        }
                    }
                });
            }
            if let Some(rc) = &rendering {
                // **A renderer having RenderingControl does not mean it will
                // let us use it.** Measured on the LG: `GetVolume` answers UPnP
                // **606 "Action not authorized"**, i.e. the service is present
                // and closed to third-party senders. So the reading is
                // authoritative in both directions — a failure marks the volume
                // *unknown*, which disables the slider and routes keys and the
                // wheel to the existing "volume lives on the TV's own remote"
                // explanation, exactly as a Cast device with `controlType:
                // fixed` does. Leaving a stale value there would show a slider
                // that moves and changes nothing.
                let args = "<Channel>Master</Channel>";
                let reading = match soap(client, rc, RENDERING_CONTROL, "GetVolume", args).await {
                    Ok(v) => {
                        if crate::cast::cast_debug() {
                            eprintln!("[dlna] GetVolume -> {}", v.replace('\n', " "));
                        }
                        tag(&v, "CurrentVolume").and_then(|s| s.parse::<f64>().ok())
                    }
                    Err(e) => {
                        if crate::cast::cast_debug() {
                            eprintln!("[dlna] {e}");
                        }
                        None
                    }
                };
                // **A reading of zero from a television that is audibly not
                // silent is not a volume.** Measured on the LG: `GetVolume`
                // answers 200 with `<CurrentVolume>0</CurrentVolume>` while the
                // set plays at almost full, and the same action asked from
                // curl comes back UPnP **606 "Action not authorized"** — the
                // service is there and its Master channel is not wired to the
                // speakers. So a level is trusted once a non-zero one has been
                // seen, and until then the slider stays disabled and keys and
                // wheel give the existing "volume lives on the TV's own remote"
                // answer. A device that is genuinely at zero costs one such
                // message; the alternative is a slider that moves and does
                // nothing, or worse, one that jumps the room to full.
                set_state(&poll_state, |s| match reading {
                    Some(level) => {
                        if level > 0.0 {
                            s.saw_volume = true;
                        }
                        s.volume = (level / 100.0).clamp(0.0, 1.0);
                        s.volume_known = s.saw_volume;
                    }
                    None => s.volume_known = false,
                });
            }
        }
    });
    inner.session = Some(DlnaSession { device, state, poll });
    Ok(())
}

#[tauri::command]
pub async fn dlna_load(
    service: tauri::State<'_, Arc<DlnaService>>,
    cast_service: tauri::State<'_, Arc<crate::cast::CastService>>,
    path: String,
    position: f64,
    title: Option<String>,
    hidden: bool,
) -> Result<(), String> {
    let (ip, advertised) = {
        let inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
        let device = &inner.session.as_ref().ok_or("not connected")?.device;
        (device.ip.clone(), device.mimes.clone())
    };
    let file = std::path::PathBuf::from(&path);
    if !file.is_file() {
        return Err("no such file".into());
    }
    let ip: std::net::IpAddr = ip.parse().map_err(|_| "bad device address".to_string())?;
    let url = crate::cast::serve_one_file(&cast_service, ip, &file, hidden).await?;
    // `cast_mime_for` answers for the Cast ladder, where anything but MP4 and
    // WebM is prepared first — so it says `application/octet-stream` for the
    // containers only this transport ever carries. Ask the renderer instead.
    let mime = negotiate_mime(
        &advertised,
        &extension_of(&path),
        crate::cast::cast_mime_for(&file),
    );
    let size = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
    let duration = media_duration(&file);
    load_url(&service, url, mime, size, duration, position, title).await
}

/// The same load for a URL somebody else registered — the torrent stream, where
/// there is no file to open and the size and duration come from the torrent and
/// from mpv instead of from a container header.
#[tauri::command]
pub async fn dlna_load_url(
    service: tauri::State<'_, Arc<DlnaService>>,
    url: String,
    mime: String,
    size: u64,
    duration: f64,
    position: f64,
    title: Option<String>,
) -> Result<(), String> {
    let duration = (duration > 0.0).then_some(duration);
    load_url(&service, url, mime, size, duration, position, title).await
}

async fn load_url(
    service: &Arc<DlnaService>,
    url: String,
    mime: String,
    size: u64,
    duration: Option<f64>,
    position: f64,
    title: Option<String>,
) -> Result<(), String> {
    let (control, state, advertised) = {
        let inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
        let session = inner.session.as_ref().ok_or("not connected")?;
        (
            session.device.control_url.clone(),
            session.state.clone(),
            session.device.mimes.clone(),
        )
    };
    // The torrent path arrives with a MIME the frontend picked from its own
    // table; the renderer's list still gets the last word on how to spell it.
    let mime = negotiate_mime(&advertised, &extension_of(&url), &mime);
    set_state(&state, |s| {
        s.state = "loading".into();
        s.error = None;
        s.time = position;
        s.reported_at = None;
        if let Some(d) = duration {
            s.duration = d;
        }
    });

    let client = http();
    // **Stop before Set, always.** A renderer that is already playing refuses
    // a new URI outright — measured: UPnP **701 "Transition not available"** —
    // and it will be playing more often than not: the previous session, a
    // session this player did not end cleanly, or another sender on the same
    // television. The Stop is best-effort by design; on an idle renderer it is
    // a no-op, and its failure says nothing about whether the load will work.
    let _ = soap(client, &control, AVTRANSPORT, "Stop", "").await;
    let name = title.unwrap_or_else(|| "Frame Player".into());
    // The URL is percent-encoded by the server that issued it, so nothing in it
    // needs escaping today; escaping anyway is what keeps that a property of
    // this call rather than of whoever registered the file.
    let args = format!(
        "<CurrentURI>{}</CurrentURI><CurrentURIMetaData>{}</CurrentURIMetaData>",
        escape_text(&url),
        didl(&url, &mime, &name, size, duration)
    );
    if let Err(e) = soap(client, &control, AVTRANSPORT, "SetAVTransportURI", &args).await {
        eprintln!("[dlna] {e}");
        set_state(&state, |s| {
            s.state = "error".into();
            s.error = Some("load_failed".into());
        });
        return Err(e);
    }
    // **Accepting a URI is not being ready for a command, and a refusal is not
    // a verdict.** Setting the URI is what makes the TV bring its player up,
    // and until that is done the renderer answers `Play` with a dropped
    // connection or with UPnP 701 "transition not available".
    //
    // The trap is that the refusal arrives while the television is *already
    // fetching the file*: measured on the LG, `SetAVTransportURI` alone starts
    // it, so counting refusals means giving up on a load that is under way.
    // Measured on a 10 GB 2160p MKV: the set answered `TRANSITIONING` from the
    // first poll to the last, served a HEAD, a read from byte 0 and then the
    // Matroska cues from the 10 GB mark ~18 s in — while the old six-attempt
    // budget expired around seven seconds and reported "the TV could not open
    // this file". Casting the same file again succeeded at once, against a
    // player that was by then already up.
    //
    // So the renderer is asked what it is doing rather than counted at. It is
    // started if it answers `Play`, if it reports `PLAYING` whoever caused it,
    // or if it reports `TRANSITIONING` — that is a television preparing, not
    // one refusing. Only "still not started when the budget runs out" is a
    // failed load.
    let mut playing = false;
    let mut starting = false;
    let mut refusal: Option<String> = None;
    let deadline = std::time::Instant::now() + PLAY_BUDGET;
    loop {
        match soap(client, &control, AVTRANSPORT, "Play", "<Speed>1</Speed>").await {
            Ok(_) => {
                playing = true;
                break;
            }
            // Printed unconditionally, like a failed Cast LOAD: the renderer
            // names its own reason, and collapsing every one of them into
            // `load_failed` is what made this failure take a wire log to tell
            // apart from a file the television cannot decode.
            Err(e) => {
                eprintln!("[dlna] {e}");
                refusal = Some(e);
            }
        }
        if let Ok(info) = soap(client, &control, AVTRANSPORT, "GetTransportInfo", "").await {
            match tag(&info, "CurrentTransportState").unwrap_or("") {
                "PLAYING" => {
                    playing = true;
                    break;
                }
                "TRANSITIONING" => starting = true,
                _ => {}
            }
            // The device saying outright that it could not take the stream —
            // no reason to spend the rest of the budget asking again.
            if tag(&info, "CurrentTransportStatus")
                .is_some_and(|s| s.eq_ignore_ascii_case("ERROR_OCCURRED"))
            {
                refusal = Some("renderer reported ERROR_OCCURRED".into());
                break;
            }
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(PLAY_RETRY).await;
    }
    if !playing && !starting {
        set_state(&state, |s| {
            s.state = "error".into();
            s.error = Some("load_failed".into());
        });
        return Err(refusal.unwrap_or_else(|| "renderer never accepted Play".into()));
    }
    if position > 1.0 {
        // Best-effort, and worth saying why it may do nothing: a renderer that
        // is only `TRANSITIONING` has not read the container's index yet, so it
        // has no way to turn a time into an offset and answers accordingly.
        // Losing the resume point is a far smaller failure than refusing the
        // load over it, which is why this stays a nudge — but a silent one is
        // indistinguishable from a television that ignores seeks altogether.
        //
        // Deliberately *not* where the seek unit gets learned: a refusal here
        // usually means "too early", not "wrong unit", and recording ABS_TIME
        // from it would pin the whole session to the fallback. `seek_to` only
        // remembers what succeeded, so a failure at this point leaves the
        // ladder untouched for the viewer's first real seek.
        let _ = seek_to(client, &control, &state, position).await;
    }
    // **Not "playing" — "buffering".** A renderer accepting `Play` says nothing
    // about it managing to decode what it fetched, and claiming playback here
    // blinded every detector downstream: the frontend marks the session as
    // having played, and a television that takes the file and shows nothing
    // then looks exactly like one that is watching it. The poll reports the
    // truth a second later (TRANSITIONING → PLAYING), and until it does, the
    // session is honestly still starting.
    set_state(&state, |s| s.state = "buffering".into());
    Ok(())
}

#[tauri::command]
pub fn dlna_status(
    service: tauri::State<'_, Arc<DlnaService>>,
    cast_service: tauri::State<'_, Arc<crate::cast::CastService>>,
) -> DlnaStatus {
    let inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
    let Some(session) = inner.session.as_ref() else {
        return DlnaStatus {
            state: "idle".into(),
            ..Default::default()
        };
    };
    let s = session.state.lock().unwrap_or_else(|p| p.into_inner());
    // Extrapolated between the renderer's one-second reports, exactly as the
    // Cast status is, or the seekbar steps once a second.
    let time = match (&s.reported_at, s.state.as_str()) {
        (Some(at), "playing") => s.time + at.elapsed().as_secs_f64(),
        _ => s.time,
    };
    DlnaStatus {
        state: s.state.clone(),
        error: s.error.clone(),
        time,
        duration: s.duration,
        volume: s.volume,
        muted: false,
        volume_known: s.volume_known,
        // A renderer with no RenderingControl cannot take a volume; one that has
        // it is adjustable, and DLNA has no "fixed" declaration to read.
        volume_fixed: session.device.rendering_url.is_none(),
        fetches: crate::cast::server_hits(&cast_service),
        device: Some(session.device.name.clone()),
    }
}

#[tauri::command]
pub async fn dlna_control(
    service: tauri::State<'_, Arc<DlnaService>>,
    action: String,
    value: Option<f64>,
) -> Result<(), String> {
    let (control, rendering, state) = {
        let inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
        let session = inner.session.as_ref().ok_or("not connected")?;
        (
            session.device.control_url.clone(),
            session.device.rendering_url.clone(),
            session.state.clone(),
        )
    };
    let client = http();
    match action.as_str() {
        // **The optimism has to be armed before the round trip, not after it.**
        // A SOAP call to the television takes a few hundred milliseconds, and
        // the frontend polls twice a second: set the state afterwards and a
        // poll lands in the gap, reads the state the command is in the middle
        // of changing, and the button flips to the new icon, back to the old
        // one and forward again. Arming first closes the window; a refusal
        // rolls the state back, which is the only reason the previous value is
        // captured at all.
        "play" | "pause" => {
            let playing = action == "play";
            let previous = arm(&state, if playing { "playing" } else { "paused" });
            let call = if playing {
                soap(client, &control, AVTRANSPORT, "Play", "<Speed>1</Speed>").await
            } else {
                soap(client, &control, AVTRANSPORT, "Pause", "").await
            };
            if let Err(e) = call {
                set_state(&state, |s| {
                    s.state = previous;
                    s.settle_until = None;
                });
                return Err(e);
            }
        }
        "seek" => {
            let target = value.unwrap_or(0.0).max(0.0);
            // Same rule as above, and for the knob rather than the icon: the
            // position is claimed before the call, or a poll in the gap drags
            // it back to where the film was.
            let was_paused = {
                let s = state.lock().unwrap_or_else(|p| p.into_inner());
                s.state == "paused"
            };
            set_state(&state, |s| {
                s.time = target;
                s.reported_at = Some(std::time::Instant::now());
                s.settle_until = Some(std::time::Instant::now() + SETTLE);
            });
            if was_paused {
                // **This renderer refuses to seek while paused** — measured,
                // UPnP 501 "Action Failed" — so the standard controller remedy
                // applies: resume, seek, pause again. Verified end to end
                // against the television: the position moves, the state comes
                // back paused and the TV fetches the new byte range. It costs a
                // fraction of a second of playback, which is the honest price
                // of a seek on a device that has no other way to do one; the
                // alternative is a seekbar that refuses to work whenever the
                // film is paused, which is when a viewer most often reaches for
                // it.
                let _ = soap(client, &control, AVTRANSPORT, "Play", "<Speed>1</Speed>").await;
                tokio::time::sleep(Duration::from_millis(150)).await;
                let seeked = seek_to(client, &control, &state, target).await;
                tokio::time::sleep(Duration::from_millis(150)).await;
                let _ = soap(client, &control, AVTRANSPORT, "Pause", "").await;
                set_state(&state, |s| {
                    s.state = "paused".into();
                    s.settle_until = Some(std::time::Instant::now() + SETTLE);
                    s.time = target;
                    s.reported_at = Some(std::time::Instant::now());
                    s.seek_target = Some((target, std::time::Instant::now() + PAUSED_SEEK_HOLD));
                });
                seeked?;
            } else {
                seek_to(client, &control, &state, target).await?;
            }
        }
        "volume" => {
            let rc = rendering.ok_or("device has no volume control")?;
            let level = (value.unwrap_or(0.0).clamp(0.0, 1.0) * 100.0).round() as u32;
            let args = format!("<Channel>Master</Channel><DesiredVolume>{level}</DesiredVolume>");
            if let Err(e) = soap(client, &rc, RENDERING_CONTROL, "SetVolume", &args).await {
                // Refused (606 on the measured television): stop claiming the control exists,
                // so the slider disables itself on the next poll instead of
                // moving without effect.
                set_state(&state, |s| s.volume_known = false);
                return Err(e);
            }
            set_state(&state, |s| s.volume = level as f64 / 100.0);
        }
        "mute" => {
            let rc = rendering.ok_or("device has no volume control")?;
            let on = value.unwrap_or(0.0) > 0.5;
            let args = format!(
                "<Channel>Master</Channel><DesiredMute>{}</DesiredMute>",
                if on { 1 } else { 0 }
            );
            soap(client, &rc, RENDERING_CONTROL, "SetMute", &args).await?;
        }
        _ => return Err(format!("unknown action {action}")),
    }
    Ok(())
}

/// Stop the renderer and tear the session down; returns the last position, so
/// the handback to mpv lands where the television was.
#[tauri::command]
pub async fn dlna_disconnect(
    service: tauri::State<'_, Arc<DlnaService>>,
    cast_service: tauri::State<'_, Arc<crate::cast::CastService>>,
) -> Result<f64, String> {
    // The LAN server is shared with the Cast transport and was left running
    // when a DLNA session ended — the token outliving its session is the one
    // thing this server promises not to do.
    crate::cast::release_server(&cast_service);
    let session = {
        let mut inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.session.take()
    };
    let Some(session) = session else {
        return Ok(0.0);
    };
    session.poll.abort();
    let last = {
        let s = session.state.lock().unwrap_or_else(|p| p.into_inner());
        s.time
    };
    let client = http();
    let _ = soap(client, &session.device.control_url, AVTRANSPORT, "Stop", "").await;
    Ok(last)
}

/// Drive the real command surface without the GUI: discover, connect, load,
/// watch, disconnect. Off unless `FP_DLNA_PLAY=<file>` is set (with an optional
/// `FP_DLNA_TARGET=<ip>`), and deliberately calling the same functions the
/// frontend invokes rather than a parallel copy — a self-test of a private
/// re-implementation proves nothing about what ships.
pub async fn selftest(app: &tauri::AppHandle, target: Option<String>, path: String) {
    use tauri::Manager as _;

    let service = app.state::<Arc<DlnaService>>();
    let cast_service = app.state::<Arc<crate::cast::CastService>>();
    if let Err(e) = dlna_discover_start(service.clone()) {
        eprintln!("[dlna] discovery failed: {e}");
        return;
    }
    tokio::time::sleep(Duration::from_secs(5)).await;
    let devices = dlna_devices(service.clone());
    dlna_discover_stop(service.clone());
    let Some(device) = devices
        .into_iter()
        .find(|d| match &target {
            Some(ip) => &d.ip == ip,
            None => d.mimes.iter().any(|m| m.starts_with("video/")),
        })
    else {
        eprintln!("[dlna] no matching renderer");
        return;
    };
    eprintln!("[dlna] selftest on {} ({})", device.name, device.ip);
    let target_ip = device.ip.clone();
    // The description URL, which is the control URL minus its service path —
    // the device list keeps only the latter, and re-deriving it here keeps the
    // selftest honest about what the command actually receives.
    let location_for_report = device
        .control_url
        .split_once("/AVTransport/")
        .map(|(origin, _)| format!("{origin}/"))
        .unwrap_or_else(|| device.control_url.clone());
    if let Err(e) = dlna_connect(service.clone(), device) {
        eprintln!("[dlna] connect failed: {e}");
        return;
    }
    if let Err(e) = dlna_load(
        service.clone(),
        cast_service.clone(),
        path,
        0.0,
        Some("Frame Player".into()),
        false,
    )
    .await
    {
        eprintln!("[dlna] load failed: {e}");
        return;
    }
    for tick in 0..14 {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        if tick == 4 {
            match dlna_control(service.clone(), "seek".into(), Some(600.0)).await {
                Ok(()) => eprintln!("[dlna] seek to 10:00 sent"),
                Err(e) => eprintln!("[dlna] seek failed: {e}"),
            }
        }
        if tick == 8 {
            match dlna_control(service.clone(), "pause".into(), None).await {
                Ok(()) => eprintln!("[dlna] pause sent"),
                Err(e) => eprintln!("[dlna] pause failed: {e}"),
            }
        }
        if tick == 9 {
            // Seeking while paused is the case the renderer refuses outright
            // (UPnP 501) and `dlna_control` works around, so the selftest
            // exercises it here rather than the plain seek above.
            match dlna_control(service.clone(), "seek".into(), Some(900.0)).await {
                Ok(()) => eprintln!("[dlna] seek-while-paused ok"),
                Err(e) => eprintln!("[dlna] seek-while-paused failed: {e}"),
            }
        }
        if tick == 10 {
            let _ = dlna_control(service.clone(), "play".into(), None).await;
            eprintln!("[dlna] play sent");
        }
        let s = dlna_status(service.clone(), cast_service.clone());
        eprintln!(
            "[dlna] state={} t={:.1}/{:.1} vol={:.2} known={} fetches={}",
            s.state, s.time, s.duration, s.volume, s.volume_known, s.fetches
        );
    }
    // Print the diagnosis on the way out: the selftest is the only place it can
    // be exercised without the GUI, and a report nobody has read once is a
    // report that says what its author assumed.
    for l in cast_diagnose(
        target_ip.clone(),
        Some(8009),
        Some(location_for_report.clone()),
    )
    .await
    {
        eprintln!("[diag] {:<18} {:<8} {}", l.id, l.state, l.detail);
    }
    match dlna_disconnect(service.clone(), cast_service.clone()).await {
        Ok(last) => eprintln!("[dlna] disconnected at {last:.1}s"),
        Err(e) => eprintln!("[dlna] disconnect failed: {e}"),
    }
}

// ---- Diagnosis --------------------------------------------------------------

/// One line of a device report.
///
/// **`id` rather than a title**: this report is shown to a viewer, in a
/// localised window, and Rust cannot reach the dictionary — the same split the
/// macOS menu lives with. So the check names itself with a stable id the
/// frontend translates, and `detail` carries what only this side knows: the
/// addresses, the lists, the device's own error text. Data, not prose.
#[derive(serde::Serialize)]
pub struct CheckLine {
    pub id: String,
    /// ok | warn | fail | info — the report colors its rule from this.
    pub state: String,
    pub detail: String,
}

fn line(id: &str, state: &str, detail: impl Into<String>) -> CheckLine {
    CheckLine {
        id: id.to_string(),
        state: state.to_string(),
        detail: detail.into(),
    }
}

/// Everything this player can learn about a device without playing anything.
///
/// The reason it exists: when a cast fails on someone else's television, the
/// difference between "the network cannot reach it", "the receiver refused the
/// file" and "the device does not do this at all" is invisible from the outside
/// and decides everything. Every check here is one we already make somewhere in
/// the normal flow — this is those checks, run on demand, with their answers
/// written down instead of consumed.
///
/// It deliberately stops short of playing: launching an app on a television to
/// answer a diagnostic question is a surprise, and the fetch test that needs it
/// is a separate, announced step.
#[tauri::command]
pub async fn cast_diagnose(
    ip: String,
    cast_port: Option<u16>,
    dlna_location: Option<String>,
) -> Vec<CheckLine> {
    let mut out = Vec::new();
    let Ok(addr): Result<IpAddr, _> = ip.parse() else {
        out.push(line("address", "fail", ip.clone()));
        return out;
    };

    // Which of our own interfaces would serve this device, and therefore which
    // one the television has to be able to reach back on.
    match crate::cast::lan_ip_for_device(addr) {
        Some(local) => out.push(line("subnet", "ok", local.to_string())),
        None => out.push(line("subnet", "fail", String::new())),
    }

    if let Some(port) = cast_port {
        let t = std::time::Instant::now();
        match tokio::time::timeout(
            Duration::from_secs(5),
            tokio::net::TcpStream::connect((addr, port)),
        )
        .await
        {
            Ok(Ok(_)) => out.push(line("cast_port", "ok", format!("{addr}:{port} · {} ms", t.elapsed().as_millis()))),
            Ok(Err(e)) => out.push(line("cast_port", "fail", format!("{addr}:{port} · {e}"))),
            Err(_) => out.push(line("cast_port", "timeout", format!("{addr}:{port}"))),
        }
        match crate::cast::probe_receiver(addr, port).await {
            Ok(summary) => out.push(line("cast_handshake", "ok", summary)),
            Err(e) => out.push(line("cast_handshake", "fail", e)),
        }
    } else {
        out.push(line("cast_absent", "info", String::new()));
    }

    let Some(location) = dlna_location else {
        out.push(line("dlna_absent", "info", String::new()));
        return out;
    };

    let client = http();
    let Some(renderer) = describe(client, &location).await else {
        out.push(line("dlna_description", "fail", location.clone()));
        return out;
    };
    out.push(line("dlna_renderer", "ok", format!("{} · {} {}", renderer.friendly_name, renderer.manufacturer, renderer.model)));
    match &renderer.avtransport {
        Some(url) => out.push(line("avtransport", "ok", url.clone())),
        None => out.push(line("avtransport", "fail", String::new())),
    }

    if let Some(cm) = &renderer.connection_manager {
        match protocol_info(client, cm).await {
            Some(sink) => {
                let mimes = summarize_sink(&sink);
                let video: Vec<&String> = mimes.keys().filter(|m| m.starts_with("video/")).collect();
                out.push(line(
                    "formats",
                    if video.is_empty() { "warn" } else { "ok" },
                    format!(
                        "{} · {}",
                        mimes.len(),
                        video.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                    ),
                ));
            }
            None => out.push(line("formats", "warn", String::new())),
        }
    }

    // The two capabilities users notice missing, both read from the device's own
    // description rather than discovered by failing.
    if let Some(scpd) = &renderer.avtransport_scpd {
        if let Ok(r) = client.get(scpd).timeout(Duration::from_secs(5)).send().await {
            let xml = r.text().await.unwrap_or_default();
            let actions: Vec<String> = xml
                .split("<action>")
                .skip(1)
                .filter_map(|c| tag(c, "name").map(|s| s.to_string()))
                .collect();
            let seekable = actions.iter().any(|a| a == "Seek");
            out.push(line(
                "seeking",
                if seekable { "ok" } else { "warn" },
                actions.join(", "),
            ));
        }
    }
    if let Some(rc) = &renderer.rendering_control {
        let args = "<Channel>Master</Channel>";
        match soap(client, rc, RENDERING_CONTROL, "GetVolume", args).await {
            Ok(v) => {
                let level = tag(&v, "CurrentVolume").unwrap_or("?").to_string();
                out.push(line(
                    "volume",
                    if level == "0" { "warn" } else { "ok" },
                    level,
                ));
            }
            Err(e) => out.push(line("volume", "warn", e)),
        }
    }
    out
}
