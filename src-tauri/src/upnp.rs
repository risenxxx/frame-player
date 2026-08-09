//! Asking the router whether the BitTorrent port is really open.
//!
//! **Why this exists at all is a measurement, not a feature request.** A rutracker
//! swarm was probed one peer at a time: of ~30 addresses the tracker handed out,
//! **20–22 never answered a SYN**, 5 completed a handshake, 2–3 refused the
//! connection outright and exactly **1** showed the mid-handshake cut that means
//! DPI. So the swarm is not dead and it is not being filtered — it is behind NAT,
//! and those peers can only ever be reached if *they* dial *us*. Everything else
//! that suggests itself was measured and is worth nothing here: public trackers
//! know 0–1 extra peers for a tracker-exclusive release, `numwant` is capped by
//! the tracker, `peers6` is not sent at all, and PEX is already running. Being
//! reachable is the only large lever left.
//!
//! librqbit will ask the router for a mapping (`enable_upnp_port_forwarding`),
//! but its forwarder is a `run_forever` task that reports to nobody — so a switch
//! wired only to that flag would say "on" whether or not a single packet ever
//! reached the router, which is the kind of setting that is worse than none. This
//! module closes that: it finds the gateway, asks it for **the mapping librqbit
//! created**, and answers with what the router itself said.
//!
//! Deliberately a read, never a write. The mapping is librqbit's to make and
//! renew (60 s lease, refreshed); a second writer would be two things fighting
//! over one port with no way to tell which won.
//!
//! On macOS this needs Local Network permission, exactly as casting does — and so
//! does librqbit's own forwarder, since both start with a multicast search. A
//! refused permission therefore reports "no router", which is the honest answer:
//! from here the two are indistinguishable, and the mapping did not happen either
//! way.

use std::time::Duration;

use serde::Serialize;

use crate::dlna::{absolute, chunks, header, http, ssdp_sockets, tag, SSDP_ADDR};

/// An Internet Gateway Device announces itself under one of these, and the
/// connection service that owns port mappings is one of the two below. Both
/// spellings are live in the field: PPP is what DSL routers expose.
const IGD_ST: &str = "urn:schemas-upnp-org:device:InternetGatewayDevice:1";
const WAN_IP: &str = "urn:schemas-upnp-org:service:WANIPConnection:1";
const WAN_PPP: &str = "urn:schemas-upnp-org:service:WANPPPConnection:1";

/// Short on purpose: this runs while a settings dialog is open, and a router
/// that has not answered a multicast in two seconds is not going to.
const SEARCH_TIMEOUT: Duration = Duration::from_secs(2);
const SOAP_TIMEOUT: Duration = Duration::from_secs(5);

/// What the router said, as an id the frontend translates plus whatever data
/// only this side can see.
///
/// Rust returns an id and never prose — the same rule the cast diagnosis
/// follows, and for the same reason: the sentence has to be localised and Rust
/// cannot reach the dictionary.
#[derive(Serialize, Default)]
pub struct PortStatus {
    /// `off` | `mapped` | `unmapped` | `no_router` | `no_port`
    pub state: String,
    /// The port librqbit is listening on, once there is a session.
    pub port: u16,
    /// Where the mapping points, or the router's own refusal — shown as-is
    /// beside the sentence, because on somebody else's router this is the only
    /// thing that distinguishes one failure from another.
    pub detail: Option<String>,
}

/// Ask the gateway whether `port` is forwarded to us.
pub async fn check(port: u16) -> PortStatus {
    if port == 0 {
        return PortStatus {
            state: "no_port".into(),
            ..Default::default()
        };
    }
    let client = http();
    let mut last_error = None;

    let gateways = search().await;
    // Logged because the two ways this comes back empty are indistinguishable
    // from the answer alone — a router with UPnP switched off, and macOS
    // dropping the multicast because Local Network permission was refused — and
    // the second one is not a fact about the network at all.
    eprintln!("[upnp] {} gateway(s) answered for port {port}", gateways.len());

    for location in gateways {
        let Some(xml) = fetch(client, &location).await else {
            continue;
        };
        // Services are found by scanning for `<service>` rather than parsed
        // properly, exactly as the renderer description is read in dlna.rs: the
        // one thing wanted out of this document is a control URL next to a
        // service type, and a real XML parser for that is a dependency to carry
        // for one field.
        //
        // **The whole document is the right scope here, and it is the wrong one
        // there.** An InternetGatewayDevice keeps the service that owns port
        // mappings two levels down — root → WANDevice → WANConnectionDevice —
        // so narrowing to the root device's own `serviceList`, which is what
        // `renderer_device` does for a MediaRenderer, would find nothing at all.
        // The two modules read the same shape of document for opposite reasons.
        for chunk in chunks(&xml, "service") {
            let Some(kind) = tag(chunk, "serviceType") else {
                continue;
            };
            if kind != WAN_IP && kind != WAN_PPP {
                continue;
            }
            let Some(control) = tag(chunk, "controlURL") else {
                continue;
            };
            let control = absolute(&location, control);
            match mapping(client, &control, kind, port).await {
                Ok(Some(client_addr)) => {
                    return PortStatus {
                        state: "mapped".into(),
                        port,
                        detail: Some(client_addr),
                    }
                }
                // The router answered and has no such mapping: it is there, it
                // understands the question, and librqbit's request did not take.
                Ok(None) => {
                    return PortStatus {
                        state: "unmapped".into(),
                        port,
                        detail: None,
                    }
                }
                Err(e) => last_error = Some(e),
            }
        }
    }

    PortStatus {
        state: "no_router".into(),
        port,
        detail: last_error,
    }
}

/// `Ok(Some(where it points))` for a live mapping, `Ok(None)` when the router
/// says there is none, `Err` when it would not answer the question.
async fn mapping(
    client: &reqwest::Client,
    control_url: &str,
    service: &str,
    port: u16,
) -> Result<Option<String>, String> {
    let body = format!(
        r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:GetSpecificPortMappingEntry xmlns:u="{service}"><NewRemoteHost></NewRemoteHost><NewExternalPort>{port}</NewExternalPort><NewProtocol>TCP</NewProtocol></u:GetSpecificPortMappingEntry></s:Body></s:Envelope>"#
    );
    let response = client
        .post(control_url)
        .header("Content-Type", "text/xml; charset=\"utf-8\"")
        .header(
            "SOAPAction",
            format!("\"{service}#GetSpecificPortMappingEntry\""),
        )
        .timeout(SOAP_TIMEOUT)
        .body(body)
        .send()
        .await
        .map_err(|e| format!("{e}"))?;

    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if status.is_success() {
        let host = tag(&text, "NewInternalClient").unwrap_or("?");
        let internal = tag(&text, "NewInternalPort").unwrap_or("?");
        return Ok(Some(format!("{host}:{internal}")));
    }

    // **714 is the answer, not an error.** `NoSuchEntryInArray` is how an IGD
    // says "nothing is mapped on that port", and treating it as a failure would
    // report a working router as an absent one — which is the difference between
    // "your router refuses" and "turn UPnP on in your router".
    let code = tag(&text, "errorCode").unwrap_or("");
    if code == "714" {
        return Ok(None);
    }
    let desc = tag(&text, "errorDescription").unwrap_or("");
    Err(format!("HTTP {status}, UPnP {code} {desc}").trim().to_string())
}

/// SSDP M-SEARCH for gateways, one socket per usable interface.
async fn search() -> Vec<String> {
    let mut tasks = Vec::new();
    for (_, sock) in ssdp_sockets() {
        tasks.push(tauri::async_runtime::spawn(async move {
            let msg = format!(
                "M-SEARCH * HTTP/1.1\r\nHOST: {SSDP_ADDR}\r\nMAN: \"ssdp:discover\"\r\nMX: 1\r\nST: {IGD_ST}\r\n\r\n"
            );
            if sock.send_to(msg.as_bytes(), SSDP_ADDR).await.is_err() {
                return Vec::new();
            }
            let mut found: Vec<String> = Vec::new();
            let mut buf = vec![0u8; 4096];
            let deadline = tokio::time::Instant::now() + SEARCH_TIMEOUT;
            loop {
                let left = deadline.saturating_duration_since(tokio::time::Instant::now());
                if left.is_zero() {
                    break;
                }
                let Ok(Ok((n, _))) = tokio::time::timeout(left, sock.recv_from(&mut buf)).await
                else {
                    break;
                };
                if let Some(loc) = header(&String::from_utf8_lossy(&buf[..n]), "location") {
                    if !found.contains(&loc) {
                        found.push(loc);
                    }
                }
            }
            found
        }));
    }

    let mut locations = Vec::new();
    for task in tasks {
        for loc in task.await.unwrap_or_default() {
            if !locations.contains(&loc) {
                locations.push(loc);
            }
        }
    }
    locations
}

async fn fetch(client: &reqwest::Client, location: &str) -> Option<String> {
    client
        .get(location)
        .timeout(SOAP_TIMEOUT)
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()
}
