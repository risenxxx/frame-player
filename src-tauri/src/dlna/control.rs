//! Control: the one HTTP client every UPnP conversation goes through, a SOAP
//! action against a service, and `GetProtocolInfo` — the renderer's own list
//! of what it will accept.

use std::collections::BTreeMap;
use std::time::Duration;

use super::xml::tag;
use super::CONNECTION_MANAGER;

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

/// `GetProtocolInfo` — the renderer's own list of what it will accept, which is
/// the whole reason this transport is interesting. Returns the raw `Sink` value
/// (a long comma-separated list of `http-get:*:<mime>:<flags>`).
pub(super) async fn protocol_info(client: &reqwest::Client, control_url: &str) -> Option<String> {
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
pub(super) async fn soap(
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

/// The Sink list is hundreds of `http-get:*:<mime>:DLNA.ORG_PN=...` entries;
/// what matters for the ladder is which MIME types appear at all.
pub(super) fn summarize_sink(sink: &str) -> BTreeMap<String, usize> {
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    for entry in sink.split(',') {
        let parts: Vec<&str> = entry.split(':').collect();
        if parts.len() >= 3 && !parts[2].is_empty() {
            *out.entry(parts[2].to_string()).or_default() += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sink_is_summarised_by_mime() {
        let sink = "http-get:*:video/mp4:DLNA.ORG_PN=AVC_MP4_MP_HD,\
                    http-get:*:video/mp4:DLNA.ORG_PN=AVC_MP4_HP_HD,\
                    http-get:*:video/x-matroska:*";
        let s = summarize_sink(sink);
        assert_eq!(s.get("video/mp4"), Some(&2));
        assert_eq!(s.get("video/x-matroska"), Some(&1));
    }
}
