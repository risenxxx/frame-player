//! Torrent RSS feeds: a link that names a *release* rather than one torrent.
//!
//! ## Why a feed is worth accepting at all
//!
//! BitTorrent cannot add a file to a torrent, so an uploader who adds an episode
//! publishes a different torrent with a different magnet — see
//! `torrent_relocate` for how the player carries a season across that. What it
//! could not do was *notice*: the new magnet lives on a tracker page, and the
//! viewer had to go and fetch it. Many trackers publish a feed per release whose
//! whole job is to answer that question, so a feed pasted instead of a magnet is
//! both a way in and a way to learn about the next episode.
//!
//! ## One skeleton, many dialects
//!
//! The container is RSS 2.0 almost everywhere, occasionally Atom, and that is
//! the only thing feeds agree on. Where the torrent is differs by site, and the
//! parser reads every place seen in the wild rather than one per site:
//!
//! - `<enclosure url type="application/x-bittorrent">` — the RSS way;
//! - `<link>` holding a magnet or a `.torrent` URL, or Atom's
//!   `<link rel="enclosure" href>`;
//! - a namespaced element: `nyaa:infoHash`, `torrent:magnetURI` /
//!   `torrent:infoHash` (EZTV), `showrss:info_hash`;
//! - Torznab's `<torznab:attr name="magneturl"|"infohash"|"size" value>`.
//!
//! The info hash is optional in every one of them. A 40-hex `<guid>` is read as
//! one (it is exactly what some sites put there, and no other guid is 40 hex by
//! accident), and an item that carries neither a hash nor anything to fetch is
//! left in the list with no source, which the frontend does not offer.
//!
//! Elements are matched by **local name**, so a namespace prefix a site chose
//! differently from the next one costs nothing. This is also why it is a real
//! parser (quick-xml, already in the tree) rather than the string scanning
//! `dlna.rs` gets away with: the number of shapes here is the reason that
//! module's own note says to take a dependency instead.
//!
//! ## What travels, and through what
//!
//! A feed URL names what somebody is watching exactly as a magnet does, so the
//! request goes **through the torrent proxy when there is one** — the rule
//! `announce_peers` set for the one tracker request the player makes itself.
//! The frontend never polls: a feed is read when it is pasted, when the start
//! screen lists the torrent it belongs to, and when the update button is
//! pressed. Anything more would be the resident client this player is not.

use std::time::Duration;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use serde::Serialize;

const TIMEOUT: Duration = Duration::from_secs(20);

/// A feed is a few kilobytes; a page this large is not one, and reading it
/// whole into memory to find that out is the one thing worth refusing.
const MAX_FEED_BYTES: usize = 8 * 1024 * 1024;

/// A season's `.torrent` is a few hundred kilobytes (one piece hash per 20
/// bytes). Anything past this is a page served under a torrent's name.
const MAX_TORRENT_BYTES: usize = 32 * 1024 * 1024;

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct FeedItem {
    pub title: String,
    /// Lower-case hex, 40 characters.
    pub info_hash: Option<String>,
    pub magnet: Option<String>,
    pub torrent_url: Option<String>,
    pub size: Option<u64>,
    /// Seconds since the epoch.
    pub published: Option<i64>,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Feed {
    pub title: Option<String>,
    pub items: Vec<FeedItem>,
}

#[derive(Serialize, Clone, Debug)]
pub struct FeedTorrent {
    pub info_hash: String,
    pub name: Option<String>,
}

fn client(proxy: &str) -> reqwest::Client {
    // A User-Agent, always: reqwest sends none by default, and the WAF in front
    // of a tracker refuses such a request outright — the trap the announce has
    // already paid for.
    let mut builder = reqwest::Client::builder()
        .user_agent(format!("FramePlayer/{}", env!("CARGO_PKG_VERSION")))
        .timeout(TIMEOUT);
    if let Some(p) = (!proxy.is_empty())
        .then(|| reqwest::Proxy::all(proxy).ok())
        .flatten()
    {
        builder = builder.proxy(p);
    }
    builder.build().unwrap_or_default()
}

fn check_url(url: &str) -> Result<reqwest::Url, String> {
    let parsed = reqwest::Url::parse(url.trim()).map_err(|_| "bad_url".to_string())?;
    match parsed.scheme() {
        "http" | "https" => Ok(parsed),
        _ => Err("bad_url".into()),
    }
}

async fn fetch(url: &str, proxy: &str, limit: usize) -> Result<Vec<u8>, String> {
    let url = check_url(url)?;
    let resp = client(proxy)
        .get(url)
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    if !resp.status().is_success() {
        return Err(format!("http_{}", resp.status().as_u16()));
    }
    if resp.content_length().is_some_and(|n| n as usize > limit) {
        return Err("too_large".into());
    }
    let bytes = resp.bytes().await.map_err(|e| format!("{e}"))?;
    if bytes.len() > limit {
        return Err("too_large".into());
    }
    Ok(bytes.to_vec())
}

/// Bytes to text by the XML declaration's own word.
///
/// Almost every feed is UTF-8, and the exception is not hypothetical: trackers
/// that grew up on windows-1251 still publish in it. The HTTP charset is not
/// consulted because a feed served as `application/xml` often carries none,
/// while the declaration is what an XML reader is required to honour anyway.
fn decode_body(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.trim_start_matches('\u{feff}').to_string();
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).to_lowercase();
    let label = head.find("encoding=").and_then(|i| {
        let rest = &head[i + 9..];
        let quote = rest.chars().next()?;
        let rest = &rest[1..];
        rest.find(quote).map(|end| rest[..end].to_string())
    });
    let enc = label
        .and_then(|l| encoding_rs::Encoding::for_label(l.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    enc.decode_with_bom_removal(bytes).0.into_owned()
}

/// The info hash out of anything that might carry one, as lower-case hex.
///
/// A magnet's `xt` may be base32 (32 characters) rather than hex, and both name
/// the same torrent — so both come back as hex, or an update check would see
/// two spellings of one hash as two torrents.
fn hash_from(text: &str) -> Option<String> {
    let s = text.trim();
    let candidate = match s.to_ascii_lowercase().find("urn:btih:") {
        Some(i) => s[i + 9..]
            .split(|c: char| !c.is_ascii_alphanumeric())
            .next()
            .unwrap_or(""),
        None => s,
    };
    if candidate.len() == 40 && candidate.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Some(candidate.to_ascii_lowercase());
    }
    if candidate.len() == 32 {
        return base32_to_hex(candidate);
    }
    None
}

fn base32_to_hex(s: &str) -> Option<String> {
    let mut bits: u64 = 0;
    let mut nbits = 0;
    let mut out = String::with_capacity(40);
    for c in s.bytes() {
        let v = match c.to_ascii_uppercase() {
            b @ b'A'..=b'Z' => b - b'A',
            b @ b'2'..=b'7' => b - b'2' + 26,
            _ => return None,
        };
        bits = (bits << 5) | v as u64;
        nbits += 5;
        if nbits >= 8 {
            nbits -= 8;
            out.push_str(&format!("{:02x}", (bits >> nbits) & 0xff));
        }
    }
    (out.len() == 40).then_some(out)
}

/// Whether a URL is plausibly a `.torrent` download.
///
/// Looser for an enclosure, which is by definition a file attached to the item
/// (`/api/torrents/<hash>/file` names no extension), than for a `<link>`, which
/// is far more often the tracker's HTML page about the release — and a page
/// fetched as a torrent is an error the viewer would read as a broken feed.
fn looks_like_torrent_url(url: &str, enclosure: bool) -> bool {
    let lower = url.to_ascii_lowercase();
    let path = lower.split(['?', '#']).next().unwrap_or("");
    let web = lower.starts_with("http://") || lower.starts_with("https://");
    web && (path.ends_with(".torrent")
        || (enclosure && lower.contains("torrent"))
        || path.contains("/download"))
}

/// `&amp;` and friends in an attribute value, which quick-xml hands over raw.
fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        match tail.find(';') {
            Some(end) if end <= 10 => {
                out.push_str(&entity(&tail[1..end]).unwrap_or_else(|| tail[..=end].to_string()));
                rest = &tail[end + 1..];
            }
            _ => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<String> {
    let c = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        n => {
            let code = if let Some(hex) = n.strip_prefix("#x").or_else(|| n.strip_prefix("#X")) {
                u32::from_str_radix(hex, 16).ok()?
            } else {
                n.strip_prefix('#')?.parse().ok()?
            };
            char::from_u32(code)?
        }
    };
    Some(c.to_string())
}

fn attr(e: &BytesStart, name: &[u8]) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        (a.key.local_name().as_ref() == name)
            .then(|| unescape(&String::from_utf8_lossy(&a.value)))
    })
}

fn local(e: &BytesStart) -> String {
    String::from_utf8_lossy(e.local_name().as_ref()).to_ascii_lowercase()
}

/// What an item/entry has collected so far, before the rules pick from it.
#[derive(Default)]
struct Draft {
    title: String,
    guid: Option<String>,
    links: Vec<String>,
    enclosures: Vec<(String, Option<String>, Option<u64>)>,
    hash: Option<String>,
    magnet: Option<String>,
    size: Option<u64>,
    published: Option<i64>,
}

impl Draft {
    fn finish(self) -> FeedItem {
        let mut magnet = self.magnet;
        let mut torrent_url = None;
        let mut size = self.size;

        // An enclosure that says it is a torrent beats one that only might be.
        let mut enclosures = self.enclosures;
        enclosures.sort_by_key(|(_, ty, _)| {
            !ty.as_deref().is_some_and(|t| t.contains("bittorrent"))
        });
        for (url, ty, len) in &enclosures {
            if url.starts_with("magnet:") {
                magnet.get_or_insert_with(|| url.clone());
            } else if torrent_url.is_none()
                && (ty.as_deref().is_some_and(|t| t.contains("bittorrent"))
                    || looks_like_torrent_url(url, true))
            {
                torrent_url = Some(url.clone());
                if size.is_none() {
                    size = *len;
                }
            }
        }
        for link in &self.links {
            if link.starts_with("magnet:") {
                magnet.get_or_insert_with(|| link.clone());
            } else if torrent_url.is_none() && looks_like_torrent_url(link, false) {
                torrent_url = Some(link.clone());
            }
        }

        let info_hash = self
            .hash
            .as_deref()
            .and_then(hash_from)
            .or_else(|| magnet.as_deref().and_then(hash_from))
            .or_else(|| {
                self.guid
                    .as_deref()
                    .filter(|g| g.trim().len() == 40)
                    .and_then(hash_from)
            });

        FeedItem {
            title: self.title.split_whitespace().collect::<Vec<_>>().join(" "),
            info_hash,
            magnet,
            torrent_url,
            size: size.filter(|&n| n > 0),
            published: self.published,
        }
    }
}

/// Read a feed. `Err("not_feed")` means the document was not RSS or Atom,
/// which the frontend takes as "this was an ordinary link after all".
pub fn parse_feed(text: &str) -> Result<Feed, String> {
    let mut reader = Reader::from_str(text);
    let mut feed = Feed::default();
    let mut root_seen = false;
    let mut draft: Option<Draft> = None;
    // The element whose text is being collected, and the text so far.
    let mut current: Option<String> = None;
    let mut buf = String::new();
    let mut depth_in_item = 0usize;

    loop {
        let event = match reader.read_event() {
            Ok(e) => e,
            Err(_) if root_seen => break,
            Err(_) => return Err("not_feed".into()),
        };
        match event {
            Event::Start(e) | Event::Empty(e)
                if !root_seen =>
            {
                let name = local(&e);
                if !matches!(name.as_str(), "rss" | "feed" | "rdf") {
                    return Err("not_feed".into());
                }
                root_seen = true;
            }
            Event::Start(e) => {
                let name = local(&e);
                if draft.is_none() {
                    if name == "item" || name == "entry" {
                        draft = Some(Draft::default());
                        depth_in_item = 0;
                    } else if name == "title" && feed.title.is_none() {
                        current = Some(name);
                        buf.clear();
                    }
                    continue;
                }
                depth_in_item += 1;
                let d = draft.as_mut().unwrap();
                if name == "link" {
                    if let Some(href) = attr(&e, b"href") {
                        push_atom_link(d, &e, href);
                        continue;
                    }
                }
                if name == "enclosure" {
                    push_enclosure(d, &e);
                }
                if name == "attr" {
                    push_torznab(d, &e);
                }
                current = Some(name);
                buf.clear();
            }
            Event::Empty(e) => {
                let Some(d) = draft.as_mut() else { continue };
                match local(&e).as_str() {
                    "enclosure" => push_enclosure(d, &e),
                    "link" => {
                        if let Some(href) = attr(&e, b"href") {
                            push_atom_link(d, &e, href);
                        }
                    }
                    "attr" => push_torznab(d, &e),
                    _ => {}
                }
            }
            Event::Text(t) => {
                if current.is_some() {
                    buf.push_str(&t.decode().unwrap_or_default());
                }
            }
            Event::CData(t) => {
                if current.is_some() {
                    buf.push_str(&t.decode().unwrap_or_default());
                }
            }
            Event::GeneralRef(r) => {
                if current.is_some() {
                    let name = r.decode().unwrap_or_default();
                    match r.resolve_char_ref() {
                        Ok(Some(c)) => buf.push(c),
                        _ => buf.push_str(&entity(&name).unwrap_or_default()),
                    }
                }
            }
            Event::End(e) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_ascii_lowercase();
                let Some(d) = draft.as_mut() else {
                    if current.as_deref() == Some(name.as_str()) {
                        let title = buf.trim().to_string();
                        if !title.is_empty() {
                            feed.title = Some(title);
                        }
                        current = None;
                    }
                    continue;
                };
                if depth_in_item == 0 && (name == "item" || name == "entry") {
                    feed.items.push(draft.take().unwrap().finish());
                    current = None;
                    continue;
                }
                depth_in_item = depth_in_item.saturating_sub(1);
                if current.as_deref() == Some(name.as_str()) {
                    let text = buf.trim().to_string();
                    take_text(d, &name, text);
                    current = None;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !root_seen {
        return Err("not_feed".into());
    }
    Ok(feed)
}

fn push_enclosure(d: &mut Draft, e: &BytesStart) {
    if let Some(url) = attr(e, b"url") {
        let len = attr(e, b"length").and_then(|l| l.trim().parse().ok());
        d.enclosures.push((url, attr(e, b"type"), len));
    }
}

fn push_atom_link(d: &mut Draft, e: &BytesStart, href: String) {
    if attr(e, b"rel").as_deref() == Some("enclosure") {
        let len = attr(e, b"length").and_then(|l| l.trim().parse().ok());
        d.enclosures.push((href, attr(e, b"type"), len));
    } else {
        d.links.push(href);
    }
}

fn push_torznab(d: &mut Draft, e: &BytesStart) {
    let (Some(name), Some(value)) = (attr(e, b"name"), attr(e, b"value")) else {
        return;
    };
    match name.to_ascii_lowercase().as_str() {
        "infohash" => d.hash = d.hash.take().or(Some(value)),
        "magneturl" if value.starts_with("magnet:") => {
            d.magnet.get_or_insert(value);
        }
        "size" => d.size = d.size.or_else(|| value.trim().parse().ok()),
        _ => {}
    }
}

fn take_text(d: &mut Draft, name: &str, text: String) {
    if text.is_empty() {
        return;
    }
    match name {
        "title" => d.title = text,
        "guid" | "id" => d.guid = Some(text),
        "link" => d.links.push(text),
        "infohash" | "info_hash" => d.hash = Some(text),
        "magneturi" | "magnet" => {
            if text.starts_with("magnet:") {
                d.magnet = Some(text);
            }
        }
        "contentlength" | "size" => {
            if let Ok(n) = text.parse() {
                d.size = Some(n);
            }
        }
        "pubdate" | "published" | "updated" | "date" => {
            if d.published.is_none() || name == "published" || name == "pubdate" {
                if let Some(t) = parse_date(&text) {
                    d.published = Some(t);
                }
            }
        }
        _ => {}
    }
}

/// RFC 2822 (`Tue, 15 Sep 2026 12:50:59 +0000`, RSS) or RFC 3339
/// (`2026-09-15T12:50:59Z`, Atom), to seconds since the epoch.
///
/// Only ordering depends on it — which of two re-uploads is newer — so a date
/// that cannot be read is simply absent rather than an error.
fn parse_date(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.len() >= 19 && s.as_bytes().get(4) == Some(&b'-') {
        let y: i64 = s.get(0..4)?.parse().ok()?;
        let mo: i64 = s.get(5..7)?.parse().ok()?;
        let d: i64 = s.get(8..10)?.parse().ok()?;
        let h: i64 = s.get(11..13)?.parse().ok()?;
        let mi: i64 = s.get(14..16)?.parse().ok()?;
        let se: i64 = s.get(17..19)?.parse().ok()?;
        let tail = s[19..].trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
        let offset = match tail {
            "" | "Z" | "z" => 0,
            t => zone_offset(&t.replace(':', ""))?,
        };
        return Some(epoch(y, mo, d, h, mi, se) - offset);
    }
    let body = s.split_once(',').map(|(_, b)| b).unwrap_or(s);
    let parts: Vec<&str> = body.split_whitespace().collect();
    if parts.len() < 4 {
        return None;
    }
    let d: i64 = parts[0].parse().ok()?;
    let months = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let mo = months
        .iter()
        .position(|m| parts[1].to_ascii_lowercase().starts_with(m))? as i64
        + 1;
    let mut y: i64 = parts[2].parse().ok()?;
    if y < 100 {
        y += 2000;
    }
    let hms: Vec<i64> = parts[3].split(':').filter_map(|p| p.parse().ok()).collect();
    let (h, mi, se) = (
        *hms.first()?,
        *hms.get(1)?,
        hms.get(2).copied().unwrap_or(0),
    );
    let offset = parts.get(4).and_then(|z| zone_offset(z)).unwrap_or(0);
    Some(epoch(y, mo, d, h, mi, se) - offset)
}

fn zone_offset(z: &str) -> Option<i64> {
    match z.to_ascii_uppercase().as_str() {
        "GMT" | "UT" | "UTC" | "Z" => return Some(0),
        "MSK" => return Some(3 * 3600),
        _ => {}
    }
    let (sign, digits) = match z.as_bytes().first()? {
        b'+' => (1, &z[1..]),
        b'-' => (-1, &z[1..]),
        _ => return None,
    };
    if digits.len() != 4 {
        return None;
    }
    let h: i64 = digits[0..2].parse().ok()?;
    let m: i64 = digits[2..4].parse().ok()?;
    Some(sign * (h * 3600 + m * 60))
}

/// Days from the civil calendar, Howard Hinnant's algorithm.
fn epoch(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> i64 {
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86_400 + h * 3600 + mi * 60 + s
}

// ---- Commands ----------------------------------------------------------------

#[tauri::command]
pub async fn feed_read(url: String, proxy: String) -> Result<Feed, String> {
    let bytes = fetch(&url, &proxy, MAX_FEED_BYTES).await?;
    parse_feed(&decode_body(&bytes))
}

/// Fetch an item's `.torrent` and put it where `torrent_add` looks first.
///
/// **The point is the cache, not the file.** A `.torrent` URL handed straight to
/// `torrent_add` is the one source with no hash to name a folder by, and a bare
/// magnet built from a feed's hash is a DHT lookup that can take ninety seconds.
/// With the metadata written to `.meta/<hash>.torrent`, a magnet built from the
/// hash opens in milliseconds, lands in a hash-named folder and is remembered
/// like any other — so everything downstream is the ordinary magnet path.
#[tauri::command]
pub async fn feed_torrent(
    app: tauri::AppHandle,
    url: String,
    proxy: String,
) -> Result<FeedTorrent, String> {
    let bytes = fetch(&url, &proxy, MAX_TORRENT_BYTES).await?;
    let (info_hash, name) = crate::torrent::cache_metadata(&app, &bytes)?;
    Ok(FeedTorrent { info_hash, name })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape AniLiberty serves: RSS 2.0, the info hash as the guid, a
    /// `.torrent` enclosure, a comment before the root and CDATA titles.
    #[test]
    fn a_release_feed_with_the_hash_in_the_guid() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
 <!-- Should be this (with ?) for valid RSS -->
<rss version="2.0"><channel><title>AniLiberty</title>
  <item>
    <title><![CDATA[Необъятный океан 3 | WEB-DL 1080p | AVC | 1-11]]></title>
    <guid>EC9423BA2737E354531CB7753E5D8CF80298DDD2</guid>
    <pubDate>Tue, 15 Sep 2026 12:50:59 +0000</pubDate>
    <description><![CDATA[<img src="x"/><br/>text]]></description>
    <enclosure url="https://example.org/api/torrents/ec94/file" type="application/x-bittorrent" length="17010925363"></enclosure>
  </item>
</channel></rss>"#;
        let feed = parse_feed(xml).unwrap();
        assert_eq!(feed.title.as_deref(), Some("AniLiberty"));
        assert_eq!(feed.items.len(), 1);
        let item = &feed.items[0];
        assert_eq!(item.title, "Необъятный океан 3 | WEB-DL 1080p | AVC | 1-11");
        assert_eq!(
            item.info_hash.as_deref(),
            Some("ec9423ba2737e354531cb7753e5d8cf80298ddd2")
        );
        assert_eq!(item.torrent_url.as_deref(), Some("https://example.org/api/torrents/ec94/file"));
        assert_eq!(item.size, Some(17_010_925_363));
        assert_eq!(item.published, Some(1_789_476_659));
    }

    /// nyaa: the hash in a namespaced element, the `.torrent` in `<link>`.
    #[test]
    fn namespaced_hash_and_torrent_link() {
        let xml = r#"<rss xmlns:nyaa="https://nyaa.si/xmlns/nyaa" version="2.0"><channel>
  <item><title>[Group] Show - 03 (1080p) [ABCD]</title>
    <link>https://nyaa.example/download/1.torrent</link>
    <guid isPermaLink="true">https://nyaa.example/view/1</guid>
    <nyaa:infoHash>0123456789abcdef0123456789abcdef01234567</nyaa:infoHash>
    <nyaa:size>1.4 GiB</nyaa:size>
  </item></channel></rss>"#;
        let item = &parse_feed(xml).unwrap().items[0];
        assert_eq!(item.info_hash.as_deref(), Some("0123456789abcdef0123456789abcdef01234567"));
        assert_eq!(item.torrent_url.as_deref(), Some("https://nyaa.example/download/1.torrent"));
        // "1.4 GiB" is not a byte count and must not become one.
        assert_eq!(item.size, None);
    }

    /// EZTV's `torrent:` namespace and Torznab's attributes, with an escaped
    /// ampersand in a magnet that must come back unescaped.
    #[test]
    fn magnets_from_eztv_and_torznab() {
        let xml = r#"<rss xmlns:torrent="http://xmlns.ezrss.it/0.1/" xmlns:torznab="http://torznab.com/schemas/2015/feed"><channel>
  <item><title>Show S01E02</title>
    <torrent:magnetURI><![CDATA[magnet:?xt=urn:btih:0123456789ABCDEF0123456789ABCDEF01234567&dn=Show]]></torrent:magnetURI>
    <torrent:contentLength>1000</torrent:contentLength>
  </item>
  <item><title>Other</title>
    <torznab:attr name="magneturl" value="magnet:?xt=urn:btih:AEBAGBAFAYDQQCIKBMGA2DQPCAIREEYU&amp;dn=Other"/>
    <torznab:attr name="size" value="2048"/>
  </item></channel></rss>"#;
        let feed = parse_feed(xml).unwrap();
        assert_eq!(feed.items[0].info_hash.as_deref(), Some("0123456789abcdef0123456789abcdef01234567"));
        assert_eq!(feed.items[0].size, Some(1000));
        assert!(feed.items[0].magnet.as_deref().unwrap().ends_with("&dn=Show"));
        // Base32 comes back as hex: 0x01..0x14.
        assert_eq!(feed.items[1].info_hash.as_deref(), Some("0102030405060708090a0b0c0d0e0f1011121314"));
        assert_eq!(feed.items[1].magnet.as_deref().unwrap(), "magnet:?xt=urn:btih:AEBAGBAFAYDQQCIKBMGA2DQPCAIREEYU&dn=Other");
        assert_eq!(feed.items[1].size, Some(2048));
    }

    #[test]
    fn atom_with_an_enclosure_link() {
        let xml = r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>T</title>
  <entry><title type="text">Film &amp; more</title><id>tag:x,1</id>
    <link rel="alternate" href="https://t.example/topic/1"/>
    <link rel="enclosure" type="application/x-bittorrent" href="https://t.example/dl?id=1" length="99"/>
    <updated>2026-09-15T15:04:04+03:00</updated>
  </entry></feed>"#;
        let feed = parse_feed(xml).unwrap();
        let item = &feed.items[0];
        assert_eq!(item.title, "Film & more");
        assert_eq!(item.torrent_url.as_deref(), Some("https://t.example/dl?id=1"));
        assert_eq!(item.info_hash, None);
        assert_eq!(item.published, Some(1_789_484_644 - 3 * 3600));
    }

    /// A topic feed that only says a page changed carries nothing to open. It
    /// stays in the list with no source rather than being guessed at: the
    /// page link is an HTML page, not a torrent.
    #[test]
    fn an_item_with_nothing_to_open() {
        let xml = r#"<rss><channel><item><title>Topic</title>
  <link>https://forum.example/viewtopic.php?t=1</link></item></channel></rss>"#;
        let item = &parse_feed(xml).unwrap().items[0];
        assert!(item.info_hash.is_none() && item.magnet.is_none() && item.torrent_url.is_none());
    }

    #[test]
    fn an_html_page_is_not_a_feed() {
        assert_eq!(parse_feed("<!doctype html><html><body>hi</body></html>"), Err("not_feed".into()));
        assert_eq!(parse_feed("not xml at all"), Err("not_feed".into()));
        assert_eq!(parse_feed(r#"{"json": true}"#), Err("not_feed".into()));
    }

    #[test]
    fn windows_1251_is_decoded_by_its_declaration() {
        let (bytes, _, _) = encoding_rs::WINDOWS_1251
            .encode("<?xml version=\"1.0\" encoding=\"windows-1251\"?><rss><channel><item><title>Серия</title></item></channel></rss>");
        let feed = parse_feed(&decode_body(&bytes)).unwrap();
        assert_eq!(feed.items[0].title, "Серия");
    }

    /// Against a real feed, off by default because it needs the network:
    /// `FP_TEST_FEED=<url> cargo test --lib feed::tests::live_feed -- --nocapture`.
    /// Reads the feed, fetches each item's `.torrent` and checks that the hash
    /// the feed claims is the hash the file actually has — the one fact the
    /// update check rests on and no fixture can vouch for.
    #[test]
    fn live_feed() {
        let Ok(url) = std::env::var("FP_TEST_FEED") else {
            return;
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let bytes = fetch(&url, "", MAX_FEED_BYTES).await.expect("feed fetch");
            let feed = parse_feed(&decode_body(&bytes)).expect("parse");
            println!("feed: {:?}, {} item(s)", feed.title, feed.items.len());
            for item in feed.items.iter().take(5) {
                println!("  {} | hash {:?} | size {:?} | date {:?}", item.title, item.info_hash, item.size, item.published);
                let Some(t) = &item.torrent_url else { continue };
                let body = fetch(t, "", MAX_TORRENT_BYTES).await.expect("torrent fetch");
                let meta = librqbit::torrent_from_bytes(&body).expect("a torrent");
                let actual = meta.info_hash.as_string();
                println!("    .torrent hash {actual}");
                if let Some(claimed) = &item.info_hash {
                    assert_eq!(claimed, &actual, "the feed's hash is not the torrent's");
                }
            }
        });
    }

    #[test]
    fn dates() {
        assert_eq!(parse_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
        assert_eq!(parse_date("1 Jan 1970 03:00:00 +0300"), Some(0));
        assert_eq!(parse_date("1970-01-01T00:00:00.123Z"), Some(0));
        assert_eq!(parse_date("yesterday"), None);
    }
}
