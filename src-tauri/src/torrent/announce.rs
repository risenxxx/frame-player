//! The one tracker request of our own: peers for a magnet's metadata fetch.

use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::Duration;

use super::dirs::is_info_hash;
use super::http::urlencode_bytes;

/// How long one seeding announce may take before it is written off, and how
/// many of a magnet's trackers are asked. Both bounds exist so a dead tracker
/// cannot add to a wait that is already up to `RESOLVE_TIMEOUT` — this is a head
/// start, never a prerequisite. They run concurrently, so the cost is one
/// timeout, not four.
const SEED_ANNOUNCE_TIMEOUT: Duration = Duration::from_secs(6);
const SEED_ANNOUNCE_TRACKERS: usize = 4;

/// Ask a magnet's own trackers who is in the swarm, so resolving it has
/// somewhere to start.
///
/// **A torrent is added paused — and upstream ties the *announced port* to
/// whether it is started.** `make_peer_rx` passes `None` for the port unless the
/// torrent is running, and a tracker asked from port 0 treats the caller as
/// unable to accept connections and answers with almost nothing: measured
/// against one such tracker on one info hash, **1 peer from port 0 against 26 from a
/// real one**. Adding paused is not negotiable here (it is the whole reason
/// "what is in this torrent" costs no download), so metadata is left to the DHT
/// alone — and a torrent whose DHT records have gone stale then never resolves
/// at all. Measured on a release with 24 live seeders: three fresh opens, three
/// 90-second timeouts, while the same torrent streams at 800 KB/s the moment it
/// has metadata.
///
/// So the swarm is asked directly, once, and the addresses are handed over as
/// `initial_peers`. Best effort in every direction — a tracker that fails,
/// answers something unparseable or is not HTTP costs nothing, because the DHT
/// is running regardless and this only ever *adds* somewhere to look.
///
/// The peer id is a throwaway rather than the session's, which is private. The
/// cost is that a tracker may briefly list two ids from this address; they
/// expire, and a client that restarts does the same thing.
pub(super) async fn announce_peers(
    magnet: &str,
    info_hash: &str,
    port: u16,
    proxy: &str,
    device: Option<&str>,
) -> Vec<SocketAddr> {
    let (Some(hash), Ok(parsed)) = (hex_bytes(info_hash), librqbit::Magnet::parse(magnet)) else {
        return Vec::new();
    };
    let urls: Vec<String> = parsed
        .trackers
        .iter()
        .filter(|t| t.starts_with("http://") || t.starts_with("https://"))
        .take(SEED_ANNOUNCE_TRACKERS)
        .cloned()
        .collect();
    if urls.is_empty() {
        return Vec::new();
    }

    let peer_id = format!("-rQ0000-{:012x}", rand::random::<u64>() & 0xffff_ffff_ffff);
    // **Through the proxy as well, when there is one.** This is the only
    // tracker request the player makes for itself — librqbit's own go through
    // the session's client, which it builds with the proxy — and one direct
    // announce would hand a tracker this machine's address while every other
    // packet went somewhere else, which is precisely the leak the setting is
    // there to close. An address that will not parse is left off rather than
    // reported: the session refuses to start on the same value and says so,
    // and a second, worse message about it helps nobody.
    //
    // **And out of the same interface as the session**, for the same reason: a
    // route setting of "past the VPN" that the one hand-made announce ignored
    // would put that announce through the tunnel the viewer just left — or,
    // the other way round, this machine's own address in front of a tracker
    // while the session stays inside it.
    let mut builder = reqwest::Client::builder();
    if let Some(p) = (!proxy.is_empty())
        .then(|| reqwest::Proxy::all(proxy).ok())
        .flatten()
    {
        builder = builder.proxy(p);
    }
    let builder = crate::net_route::scope_client(builder, device);
    let client = builder.build().unwrap_or_else(|_| reqwest::Client::new());
    let asked = urls.len();
    let results = futures_util::future::join_all(
        urls.iter()
            .map(|url| announce_one(&client, url, &hash, peer_id.as_bytes(), port)),
    )
    .await;

    let mut seen = HashSet::new();
    let peers: Vec<SocketAddr> = results
        .into_iter()
        .flatten()
        .filter(|a| seen.insert(*a))
        .collect();
    eprintln!(
        "[torrent] seeding the resolve with {} peer(s) from {asked} tracker(s)",
        peers.len()
    );
    peers
}

async fn announce_one(
    client: &reqwest::Client,
    url: &str,
    info_hash: &[u8],
    peer_id: &[u8],
    port: u16,
) -> Vec<SocketAddr> {
    // The announce URL's own query is kept and re-appended, exactly as the
    // vendored tracker client does it: the `?magnet` marker some URLs carry, and a
    // private tracker's passkey live there, and replacing the query is a 403.
    let (base, base_query) = match url.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (url, None),
    };
    let mut target = format!(
        "{base}?info_hash={}&peer_id={}&event=started&port={port}\
         &uploaded=0&downloaded=0&left=1&compact=1&no_peer_id=1&numwant=200",
        urlencode_bytes(info_hash),
        urlencode_bytes(peer_id),
    );
    if let Some(q) = base_query {
        target.push('&');
        target.push_str(q);
    }

    let req = client
        .get(&target)
        // Without it the WAF in front of several trackers answers 403 — the
        // same header the vendored announce had to grow, for the same reason.
        .header(reqwest::header::USER_AGENT, "rqbit")
        .send();
    let body = match tokio::time::timeout(SEED_ANNOUNCE_TIMEOUT, req).await {
        Ok(Ok(res)) if res.status().is_success() => res.bytes().await.ok(),
        Ok(Ok(res)) => {
            eprintln!("[torrent] {base} answered {}", res.status());
            None
        }
        Ok(Err(e)) => {
            eprintln!("[torrent] {base} failed: {e}");
            None
        }
        Err(_) => {
            eprintln!("[torrent] {base} timed out");
            None
        }
    };
    body.map(|b| compact_peers(&b)).unwrap_or_default()
}

/// The `peers` field of an announce response, compact form only.
///
/// Scanned rather than deserialized, because this wants one field out of a
/// dictionary whose other keys vary by tracker and are of no interest — and
/// because a strict parser is exactly what made librqbit discard whole
/// responses over two absent statistics (see vendor/README.md). The length being
/// a non-zero multiple of six is the sanity check: the dictionary form of
/// `peers` fails it, and so does a stray match inside binary data.
fn compact_peers(body: &[u8]) -> Vec<SocketAddr> {
    const KEY: &[u8] = b"5:peers";
    let Some(at) = body.windows(KEY.len()).position(|w| w == KEY) else {
        return Vec::new();
    };
    let rest = &body[at + KEY.len()..];
    let Some(colon) = rest.iter().position(|b| *b == b':') else {
        return Vec::new();
    };
    let Ok(len) = std::str::from_utf8(&rest[..colon]).unwrap_or("").parse::<usize>() else {
        return Vec::new();
    };
    if len == 0 || len % 6 != 0 || rest.len() < colon + 1 + len {
        return Vec::new();
    }
    rest[colon + 1..colon + 1 + len]
        .as_chunks::<6>()
        .0
        .iter()
        .map(|c| {
            SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::new(c[0], c[1], c[2], c[3]),
                u16::from_be_bytes([c[4], c[5]]),
            ))
        })
        // Port zero is not an address anything can be reached at.
        .filter(|a| a.port() != 0)
        .collect()
}

fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    if !is_info_hash(s) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one field wanted out of an announce response, and the reason it is
    /// scanned rather than deserialized: a strict parser is exactly what made
    /// librqbit throw whole responses away (see vendor/README.md). A wrong
    /// answer here is silent — no peers, and a magnet that never resolves.
    #[test]
    fn announce_peers_parsing() {
        // Verbatim shape from a real tracker: no `complete`, no `incomplete`.
        let mut real = b"d8:intervali3595e12:min intervali3595e5:peers12:".to_vec();
        real.extend_from_slice(&[93, 100, 177, 140, 0x7F, 0xA1]);
        real.extend_from_slice(&[5, 77, 195, 179, 0xA7, 0x30]);
        real.push(b'e');
        assert_eq!(
            compact_peers(&real),
            vec![
                "93.100.177.140:32673".parse::<SocketAddr>().unwrap(),
                "5.77.195.179:42800".parse::<SocketAddr>().unwrap(),
            ]
        );

        // A port of zero is not somewhere anything can be reached.
        let mut zero = b"d5:peers6:".to_vec();
        zero.extend_from_slice(&[1, 2, 3, 4, 0, 0]);
        zero.push(b'e');
        assert_eq!(compact_peers(&zero), Vec::<SocketAddr>::new());

        // Everything that is not a compact list degrades to "no peers" rather
        // than to garbage addresses: the dictionary form, a truncated body, a
        // length that is not a whole number of records, and no field at all.
        assert!(compact_peers(b"d5:peersld2:ip9:127.0.0.1eee").is_empty());
        assert!(compact_peers(b"d5:peers12:abc").is_empty());
        assert!(compact_peers(b"d5:peers7:abcdefge").is_empty());
        assert!(compact_peers(b"d8:intervali60ee").is_empty());
        assert!(compact_peers(b"").is_empty());
    }

    /// The info hash reaches the announce as raw bytes, and half of them are
    /// not printable. Getting the decode wrong asks the tracker about a
    /// different torrent, which answers cheerfully with nothing.
    #[test]
    fn info_hash_bytes() {
        assert_eq!(
            hex_bytes("378032034812493fd0e8a83e746323800a24078f").unwrap(),
            vec![
                0x37, 0x80, 0x32, 0x03, 0x48, 0x12, 0x49, 0x3F, 0xD0, 0xE8, 0xA8, 0x3E, 0x74,
                0x63, 0x23, 0x80, 0x0A, 0x24, 0x07, 0x8F
            ]
        );
        // Upper case is the same torrent; anything that is not a hash is none.
        assert_eq!(
            hex_bytes("4C0DD90150D41A5B647AA78EA828B2942C43AF45"),
            hex_bytes("4c0dd90150d41a5b647aa78ea828b2942c43af45")
        );
        assert!(hex_bytes("").is_none());
        assert!(hex_bytes("not a hash").is_none());
        assert!(hex_bytes("378032034812493fd0e8a83e746323800a24078").is_none());

        // And the bytes have to survive being put in a URL: `%3F` is a `?` in
        // the middle of an info hash, and unescaped it would truncate the query.
        assert_eq!(
            urlencode_bytes(&hex_bytes("378032034812493fd0e8a83e746323800a24078f").unwrap()),
            "7%802%03H%12I%3F%D0%E8%A8%3Etc%23%80%0A%24%07%8F"
        );
    }
}
