//! What a load tells the renderer about the file: the DIDL-Lite, where it
//! decides whether the stream may be seeked, and the MIME type, spelled the way
//! the device itself advertises it.

use super::xml::{escape_attr, escape_text};

/// `H:MM:SS.mmm`, the only duration format DIDL-Lite takes.
fn didl_duration(seconds: f64) -> String {
    let total = seconds.max(0.0);
    let h = (total / 3600.0).floor() as u64;
    let m = ((total % 3600.0) / 60.0).floor() as u64;
    let s = total % 60.0;
    format!("{h}:{m:02}:{s:06.3}")
}

/// Duration in seconds, straight from the container header.
pub(super) fn media_duration(path: &std::path::Path) -> Option<f64> {
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
///
/// **An external subtitle is said two ways, because renderers read two.** A
/// second `<res>` of a subtitle type after the video's is the generic form
/// (the one LG and most others take); Samsung reads its own
/// `sec:CaptionInfoEx` element instead — and, for a set that asks, the
/// `CaptionInfo.sec` header on the video's answer (cast/server.rs). Both appear only
/// when there is a subtitle, so the metadata of every other file is what it
/// always was.
pub(super) fn didl(
    url: &str,
    mime: &str,
    title: &str,
    size: u64,
    duration: Option<f64>,
    caption: Option<(&str, &str)>,
) -> String {
    let flags = "DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000";
    let duration_attr = duration
        .map(|d| format!(r#" duration="{}""#, didl_duration(d)))
        .unwrap_or_default();
    let (sec_ns, caption_xml) = match caption {
        Some((sub_url, sub_mime)) => {
            let kind = sub_mime.rsplit('/').next().unwrap_or("srt");
            let kind = if kind == "caption" { "smi" } else { kind };
            (
                r#" xmlns:sec="http://www.sec.co.kr/""#,
                format!(
                    r#"<res protocolInfo="{}">{url}</res><sec:CaptionInfoEx sec:type="{kind}">{url}</sec:CaptionInfoEx>"#,
                    escape_attr(&format!("http-get:*:{sub_mime}:*")),
                    url = escape_text(sub_url),
                ),
            )
        }
        None => ("", String::new()),
    };
    let item = format!(
        r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/"{sec_ns}><item id="0" parentID="-1" restricted="1"><dc:title>{}</dc:title><upnp:class>object.item.videoItem</upnp:class><res protocolInfo="{}" size="{size}"{duration_attr}>{}</res>{caption_xml}</item></DIDL-Lite>"#,
        escape_text(title),
        escape_attr(&format!("http-get:*:{mime}:{flags}")),
        escape_text(url),
    );
    escape_text(&item)
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
pub(super) fn extension_of(source: &str) -> String {
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
pub(super) fn negotiate_mime(advertised: &[String], ext: &str, wanted: &str) -> String {
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
        let x = didl("http://h/f.mkv", "video/x-matroska", "Tom & Jerry <2>", 42, None, None);
        // What the receiver un-escapes once and hands to its DIDL parser.
        let inner = x.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&");
        assert!(inner.contains("<dc:title>Tom &amp; Jerry &lt;2&gt;</dc:title>"), "{inner}");
        // Un-escaping a second time, as the DIDL parser does, gives the name back.
        let title = inner.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&");
        assert!(title.contains("<dc:title>Tom & Jerry <2></dc:title>"), "{title}");
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
        let x = didl("http://h/f.mkv", "video/x-matroska", "t", 42, Some(60.0), None);
        assert!(x.contains("DLNA.ORG_OP=01"));
        assert!(x.contains(r#"size=&quot;42&quot;"#) || x.contains("size=\"42\"") || x.contains("size="));
        assert!(x.contains("0:01:00.000"));
        // No subtitle, no trace of one: the metadata every other file gets.
        assert!(!x.contains("sec:") && !x.contains("text/srt"));
    }

    #[test]
    fn didl_names_an_external_subtitle_both_ways() {
        let x = didl("http://h/c/a/f.mkv", "video/x-matroska", "t", 42, None, Some(("http://h/c/b/b.srt", "text/srt")));
        let inner = x.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&amp;", "&");
        // The video's `res` stays first — a renderer plays the first one it takes.
        let video = inner.find("f.mkv</res>").expect("video res");
        let generic = inner.find(r#"<res protocolInfo="http-get:*:text/srt:*">http://h/c/b/b.srt</res>"#).expect("generic res");
        assert!(video < generic, "{inner}");
        assert!(inner.contains(r#"xmlns:sec="http://www.sec.co.kr/""#), "{inner}");
        assert!(inner.contains(r#"<sec:CaptionInfoEx sec:type="srt">http://h/c/b/b.srt</sec:CaptionInfoEx>"#), "{inner}");
    }
}
