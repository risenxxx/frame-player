//! The device description: which device in it is the renderer, what it is
//! called, and its control URLs resolved against the right base.

use std::time::Duration;

use super::xml::{chunks, tag, unescape};

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
pub(super) async fn describe(client: &reqwest::Client, location: &str) -> Option<Renderer> {
    let xml = client
        .get(location)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()?;
    let renderer = parse_description(location, &xml);
    // The name its owner gave it, whichever path read the description — the
    // picker, the device check, the recon probe. A report replaces it.
    crate::logs::remember("device", &renderer.friendly_name);
    Some(renderer)
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
}
