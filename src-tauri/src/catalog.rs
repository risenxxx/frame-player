//! The catalog: **what** to watch (TMDB) and **where to get it** (a
//! Torznab-compatible indexer).
//!
//! Two services and the split between them is the whole design. TMDB answers
//! "which film is this" — posters, localised titles, descriptions, how many
//! seasons a series has. An indexer answers "which releases exist for it" —
//! trackers, quality, dubs, seeders, a magnet. Neither can do the other's job:
//! an indexer holds no posters and no descriptions, so browsing one directly is
//! a list of raw release names, which is precisely the experience a catalog
//! exists to replace.
//!
//! ## There is no TMDB key in this binary, and that is the point
//!
//! Metadata goes through **our own proxy** (`services/tmdb`), which holds the
//! key. Baking one into the client was considered and rejected on the terms
//! rather than on the load: it would be a single anonymous credential shared by
//! every copy, which is what TMDB call attempting to conceal an application's
//! identity (§1.C), and handing it to every user reads like sublicensing a
//! licence that is explicitly non-sublicensable (§1.A). That the rate limit is
//! per-IP says only that a shared key costs TMDB no *load* — a fact about
//! capacity, not a permission, and conflating the two is the mistake to avoid
//! here.
//!
//! The proxy's address is a setting for the same reason the relay's is:
//! self-hosting is a setting, not a fork. An empty field means the default.
//!
//! ## Where the posters come from, and why the client decides
//!
//! Poster bytes are the whole of the bandwidth — a grid of twenty is roughly
//! 800 KB against ~25 KB of JSON for the same screen — so the cheapest possible
//! answer is to **not proxy them at all** and let the webview fetch straight
//! from TMDB's own CDN, which is closer to the viewer than any server of ours.
//! That works for most people and fails for the ones TMDB is not reachable
//! from, which is a real population rather than a hypothetical.
//!
//! So this returns the poster **path**, never a URL, and the frontend composes
//! one against whichever base it has found to work — see `posterUrl` in
//! `catalog.svelte.ts`. Deciding it there rather than here is deliberate: the
//! question is "can this webview load that image", and the honest way to answer
//! it is to try, not to infer it from an address. Geolocating the client IP
//! would need a database, would be wrong for anyone on a VPN, and would make
//! the service derive somebody's location from their address in order to guess
//! at something it can simply be told.
//!
//! ## Why this runs in Rust rather than in the webview
//!
//! The proxy address is user-supplied and arbitrary, so calling it from the
//! page would put an arbitrary host into the webview's fetch surface — the same
//! objection as the indexer's. And **reqwest sends no `User-Agent` at all** —
//! the trap the tracker announce and every UPnP call in this tree have already
//! paid for — so the one place that fixes it is a shared client, which also
//! keeps the connection pool alive across a search that makes two or three
//! calls.
//!
//! ## What travels
//!
//! A typed query and nothing else. Unlike subtitle search, nothing here is
//! derived from a file on disk — the viewer is looking for something they do
//! not have yet — so there is no path to gate against the privacy roots. That
//! stops being true the moment anything asks "which releases exist for the file
//! I am watching", and such a feature would need the gate before it ships.

use std::time::Duration;

use serde::Serialize;

const TIMEOUT: Duration = Duration::from_secs(15);

/// How many releases a single lookup may hand back. An indexer answers a broad
/// title with hundreds of rows across a dozen trackers, and a list nobody can
/// read to the end is not more useful than a list they can.
const MAX_RELEASES: usize = 120;

/// One client for every catalog conversation, with a `User-Agent` on it.
fn http() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(format!("FramePlayer/{}", env!("CARGO_PKG_VERSION")))
            .timeout(TIMEOUT)
            .build()
            .unwrap_or_default()
    })
}

/// Normalise the proxy's address: no trailing slash, and a scheme required.
///
/// **Plaintext is refused off loopback**, the same rule `socketUrl` applies to
/// the relay and for a stronger reason here: over `http://` the failure is
/// invisible because it works, and what travels is what somebody is searching
/// for. Loopback is exempt so a proxy running on the same machine needs no
/// certificate.
fn proxy_base(raw: &str) -> Result<String, String> {
    let base = raw.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("no_proxy".into());
    }
    let url = reqwest::Url::parse(base).map_err(|_| "bad_proxy".to_string())?;
    let loopback = matches!(url.host_str(), Some("127.0.0.1") | Some("localhost") | Some("[::1]"));
    match url.scheme() {
        "https" => Ok(base.to_string()),
        "http" if loopback => Ok(base.to_string()),
        _ => Err("insecure_proxy".into()),
    }
}

// ---- What the frontend gets ------------------------------------------------

/// One title in the catalog. `kind` is TMDB's own `movie`/`tv`, kept as a string
/// because it is also the path segment every later request uses.
#[derive(Serialize, Clone)]
pub struct CatalogItem {
    pub kind: String,
    pub id: i64,
    /// Localised, which is what the viewer reads.
    pub title: String,
    /// The original, which is what an indexer is searched by — a Russian release
    /// of a foreign film is filed under both, and the original matches far more
    /// reliably than a translation somebody chose.
    pub original_title: String,
    pub year: Option<i32>,
    pub poster: Option<String>,
    pub overview: String,
    pub rating: f64,
}

#[derive(Serialize)]
pub struct CatalogDetails {
    #[serde(flatten)]
    pub item: CatalogItem,
    /// Season numbers a series actually has, specials (season 0) dropped. Empty
    /// for a film, which is what tells the panel not to draw a season picker.
    pub seasons: Vec<i32>,
    pub runtime: Option<i32>,
    pub genres: Vec<String>,
}

/// One release from the indexer.
#[derive(Serialize, Clone)]
pub struct Release {
    /// The tracker's own title, kept whole: it is the only place the rip's real
    /// provenance is written, and a viewer choosing between two 4K rips reads it.
    pub title: String,
    pub tracker: String,
    pub size: u64,
    pub seeders: i64,
    pub peers: i64,
    /// 480/720/1080/2160, or 0 when the indexer could not tell.
    pub quality: i64,
    /// `hdr` / `sdr` / `dv`, as the indexer classified it.
    pub video_type: String,
    pub voices: Vec<String>,
    pub seasons: Vec<i32>,
    pub magnet: String,
    pub created: String,
}

// ---- TMDB ------------------------------------------------------------------

/// Whether the catalog can show pictures — i.e. whether a metadata proxy is
/// configured and answering.
///
/// A viewer with no proxy still gets a working panel: the indexer is searched
/// by the typed text directly. So this decides between a poster grid and a
/// plain release list, never whether to offer the feature at all.
///
/// It is a real request rather than a look at the setting, because "an address
/// is written down" and "there is a service there" are different facts and only
/// the second one makes a poster appear. Cheap — `/health` returns counters and
/// nothing else — and short-fused, since the panel waits on it.
#[tauri::command]
pub async fn catalog_ready(proxy: String) -> bool {
    let Ok(base) = proxy_base(&proxy) else {
        return false;
    };
    matches!(
        http()
            .get(format!("{base}/health"))
            .timeout(Duration::from_secs(4))
            .send()
            .await,
        Ok(r) if r.status().is_success()
    )
}

/// TMDB wants a full BCP-47 tag; the player stores a bare `ru`/`en`.
fn tmdb_lang(locale: &str) -> &'static str {
    if locale.starts_with("ru") {
        "ru-RU"
    } else {
        "en-US"
    }
}

/// One call to the proxy, which forwards it to TMDB with the key attached.
///
/// The path is TMDB's own (`/3/search/multi`), unchanged, so the proxy stays a
/// proxy rather than an API of its own — a route added here needs no deploy,
/// and pointing the setting at TMDB directly would work for anybody who has
/// their own key and wants to bypass us entirely.
async fn tmdb_get(
    proxy: &str,
    path: &str,
    params: &[(&str, String)],
) -> Result<serde_json::Value, String> {
    let base = proxy_base(proxy)?;
    let url = reqwest::Url::parse_with_params(&format!("{base}/3{path}"), params)
        .map_err(|e| e.to_string())?;
    let response = http().get(url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("http_{}", response.status().as_u16()));
    }
    response.json().await.map_err(|e| e.to_string())
}

/// Read one search/trending row into a `CatalogItem`, or nothing.
///
/// TMDB names the same field differently for films and series (`title` against
/// `name`, `release_date` against `first_air_date`), and a multi-search also
/// returns people — which have neither, and would otherwise arrive as untitled
/// rows with no poster.
fn read_item(v: &serde_json::Value, forced_kind: Option<&str>) -> Option<CatalogItem> {
    let kind = forced_kind
        .or_else(|| v.get("media_type").and_then(|m| m.as_str()))
        .unwrap_or("");
    if kind != "movie" && kind != "tv" {
        return None;
    }
    let str_of = |k: &str| v.get(k).and_then(|s| s.as_str()).unwrap_or("").to_string();
    let title = {
        let t = str_of("title");
        if t.is_empty() { str_of("name") } else { t }
    };
    let original = {
        let t = str_of("original_title");
        if t.is_empty() {
            str_of("original_name")
        } else {
            t
        }
    };
    if title.is_empty() && original.is_empty() {
        return None;
    }
    let date = {
        let d = str_of("release_date");
        if d.is_empty() {
            str_of("first_air_date")
        } else {
            d
        }
    };
    Some(CatalogItem {
        kind: kind.to_string(),
        id: v.get("id").and_then(|i| i.as_i64())?,
        title: if title.is_empty() {
            original.clone()
        } else {
            title
        },
        original_title: original,
        year: date.get(..4).and_then(|y| y.parse().ok()),
        // The **path**, not a URL: which base it hangs off is the frontend's to
        // decide, because only the frontend can find out whether TMDB's own CDN
        // loads in this webview. See `posterUrl` in catalog.svelte.ts.
        poster: v
            .get("poster_path")
            .and_then(|p| p.as_str())
            .map(|p| p.to_string()),
        overview: str_of("overview"),
        rating: v
            .get("vote_average")
            .and_then(|r| r.as_f64())
            .unwrap_or(0.0),
    })
}

fn read_list(body: &serde_json::Value, forced_kind: Option<&str>) -> Vec<CatalogItem> {
    body.get("results")
        .and_then(|r| r.as_array())
        .map(|rows| rows.iter().filter_map(|v| read_item(v, forced_kind)).collect())
        .unwrap_or_default()
}

/// What the panel shows before anybody types.
///
/// A week rather than a day: the daily list churns enough that the panel looks
/// different every time it is opened, which reads as randomness rather than as
/// a selection.
#[tauri::command]
pub async fn catalog_trending(
    proxy: String,
    locale: String,
) -> Result<Vec<CatalogItem>, String> {
    let body = tmdb_get(
        &proxy,
        "/trending/all/week",
        &[("language", tmdb_lang(&locale).to_string())],
    )
    .await?;
    Ok(read_list(&body, None))
}

#[tauri::command]
pub async fn catalog_search(
    proxy: String,
    query: String,
    locale: String,
) -> Result<Vec<CatalogItem>, String> {
    let query = query.trim().to_string();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let body = tmdb_get(
        &proxy,
        "/search/multi",
        &[
            ("query", query),
            ("language", tmdb_lang(&locale).to_string()),
            ("include_adult", "false".into()),
        ],
    )
    .await?;
    Ok(read_list(&body, None))
}

#[tauri::command]
pub async fn catalog_details(
    proxy: String,
    kind: String,
    id: i64,
    locale: String,
) -> Result<CatalogDetails, String> {
    if kind != "movie" && kind != "tv" {
        return Err("bad_kind".into());
    }
    let body = tmdb_get(
        &proxy,
        &format!("/{kind}/{id}"),
        &[("language", tmdb_lang(&locale).to_string())],
    )
    .await?;
    let item = read_item(&body, Some(&kind)).ok_or("no_item")?;
    // Season 0 is TMDB's bucket for specials and one-off extras. It is a real
    // season number to the API and never one to a release, so offering it would
    // put a picker entry that can only ever come back empty.
    let seasons = body
        .get("seasons")
        .and_then(|s| s.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|s| s.get("season_number").and_then(|n| n.as_i64()))
                .filter(|n| *n > 0)
                .map(|n| n as i32)
                .collect()
        })
        .unwrap_or_default();
    Ok(CatalogDetails {
        item,
        seasons,
        runtime: body
            .get("runtime")
            .and_then(|r| r.as_i64())
            .map(|r| r as i32),
        genres: body
            .get("genres")
            .and_then(|g| g.as_array())
            .map(|rows| {
                rows.iter()
                    .filter_map(|g| g.get("name").and_then(|n| n.as_str()))
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// Runtime defaults the service supplies, asked for rather than compiled in.
///
/// Ordinary remote configuration: a value built into the binary changes only
/// for people who download a new build, so it is a per-build constant rather
/// than a default. Asked for at runtime, the operator of an instance sets it
/// once and every player using that instance follows.
///
/// It never overrides a viewer's own setting, and the frontend does not persist
/// what comes back — so what the panel uses is the instance's current answer
/// rather than whatever it was the first time somebody opened it.
#[derive(serde::Deserialize, Serialize, Default)]
pub struct CatalogConfig {
    #[serde(default)]
    pub indexer: String,
    /// A switch above the address: a complaint may be about the feature rather
    /// than about where it points, and then removing the address is not an
    /// answer.
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub notice: String,
}

/// Read the configuration document.
///
/// A static file rather than an endpoint on the metadata proxy, and that is the
/// point: it is then independent of whether the proxy is up, deployed or
/// reachable, and it lives beside `latest.json` on infrastructure the updater
/// already depends on. `https` only, for the reason `proxy_base` gives.
///
/// **Every failure is the same answer — no configuration — because the player
/// has a working state without one: it asks the viewer.** A missing file, a
/// blocked host, a truncated document and a 404 are therefore not told apart;
/// none of them is worth an error path, and treating a parse failure as "no
/// configuration" is what stops half a document publishing half a setting.
#[tauri::command]
pub async fn catalog_config(url: String) -> CatalogConfig {
    let url = url.trim();
    if !url.starts_with("https://") {
        return CatalogConfig::default();
    }
    let Ok(response) = http()
        .get(url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
    else {
        return CatalogConfig::default();
    };
    if !response.status().is_success() {
        return CatalogConfig::default();
    }
    response.json().await.unwrap_or_default()
}

// ---- The indexer -----------------------------------------------------------

/// Fold a title down to what two spellings of the same film have in common.
///
/// Case, punctuation and the Latin/Cyrillic homoglyphs that release names mix
/// freely (`е`/`e`, `о`/`o`, `а`/`a`, `с`/`c`, `р`/`p`, `у`/`y`, `х`/`x`) — a
/// tracker title written half in one alphabet is ordinary, and comparing raw
/// strings makes those two different films.
fn fold(s: &str) -> String {
    s.chars()
        .filter_map(|c| {
            let c = c.to_lowercase().next().unwrap_or(c);
            match c {
                'а' => Some('a'),
                'е' | 'ё' => Some('e'),
                'о' => Some('o'),
                'с' => Some('c'),
                'р' => Some('p'),
                'у' => Some('y'),
                'х' => Some('x'),
                'к' => Some('k'),
                'м' => Some('m'),
                'т' => Some('t'),
                'в' => Some('b'),
                'н' => Some('h'),
                c if c.is_alphanumeric() => Some(c),
                _ => None,
            }
        })
        .collect()
}

/// One row of the indexer's `/api/v1.0/torrents`.
///
/// The field names are the indexer's, typo included — `relased` is what the API
/// answers and renaming it here would only move the surprise. Everything is
/// optional because instances differ in what they fill in, and a row missing a
/// dub list must not cost the whole response.
#[derive(serde::Deserialize, Default)]
struct IndexerRow {
    #[serde(default)]
    title: String,
    #[serde(default)]
    tracker: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    sid: i64,
    #[serde(default)]
    pir: i64,
    #[serde(default)]
    magnet: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    originalname: String,
    #[serde(default)]
    relased: i32,
    #[serde(default)]
    videotype: String,
    #[serde(default)]
    quality: i64,
    #[serde(default)]
    voices: Vec<String>,
    #[serde(default)]
    seasons: Vec<i32>,
    #[serde(default, rename = "createTime")]
    create_time: String,
}

async fn ask_indexer(base: &str, query: &str) -> Result<Vec<IndexerRow>, String> {
    let base = base.trim().trim_end_matches('/');
    let url = reqwest::Url::parse_with_params(
        &format!("{base}/api/v1.0/torrents"),
        &[("search", query), ("apikey", "null")],
    )
    .map_err(|e| e.to_string())?;
    let response = http().get(url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("http_{}", response.status().as_u16()));
    }
    // Tolerant on purpose: an instance that adds a field must not break the
    // whole search, and one row that will not parse must not take the rest with
    // it — which is the failure the librqbit tracker client already paid for,
    // where a strict parser silently discarded every peer in a valid response.
    let rows: Vec<serde_json::Value> = response.json().await.map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .filter_map(|v| serde_json::from_value::<IndexerRow>(v).ok())
        .collect())
}

/// Find the releases for one title.
///
/// **Searched by the original name first.** A Russian tracker files a foreign
/// film under both names, and the original is the one that survives translation
/// — TMDB's localised title is one of several possible renderings, while the
/// original is what the uploader typed. The localised one is the fallback and
/// the extra query, not the first guess.
///
/// Filtering happens here rather than in the query, because which parameters an
/// instance honours varies and a self-hosted one is not guaranteed to be the
/// same build as the public one. `search` is the one parameter every version
/// has; year and season are matched against the fields that come back.
#[tauri::command]
pub async fn catalog_releases(
    base: String,
    title: String,
    original_title: String,
    year: Option<i32>,
    season: Option<i32>,
) -> Result<Vec<Release>, String> {
    if base.trim().is_empty() {
        return Err("no_indexer".into());
    }
    let mut queries: Vec<String> = Vec::new();
    for q in [original_title.trim(), title.trim()] {
        if !q.is_empty() && !queries.iter().any(|had| fold(had) == fold(q)) {
            queries.push(q.to_string());
        }
    }
    if queries.is_empty() {
        return Err("no_query".into());
    }

    let wanted: Vec<String> = queries.iter().map(|q| fold(q)).collect();
    let mut out: Vec<Release> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for query in &queries {
        let rows = match ask_indexer(&base, query).await {
            Ok(rows) => rows,
            // The first query failing is the indexer being unreachable and is
            // worth reporting; a later one failing after results are already in
            // hand is not worth losing them over.
            Err(e) if out.is_empty() && query == &queries[0] => return Err(e),
            Err(_) => continue,
        };
        for row in rows {
            if row.magnet.is_empty() {
                continue;
            }
            // The indexer's own parse of the release name is what is compared,
            // never the raw tracker title: that string carries the year, the
            // codec and the dub list, so a substring test against it matches
            // anything that merely mentions the film.
            let matches_name = [row.name.as_str(), row.originalname.as_str()]
                .iter()
                .filter(|n| !n.is_empty())
                .any(|n| wanted.contains(&fold(n)));
            if !matches_name {
                continue;
            }
            // A year off by one is routine — a festival run, a national release
            // date, an indexer reading it out of the file name — so the window
            // is ±1 rather than exact, and a row with no year at all is kept:
            // refusing it would drop releases whose name matched exactly.
            if let (Some(want), true) = (year, row.relased > 0) {
                if (row.relased - want).abs() > 1 {
                    continue;
                }
            }
            // A season filter only applies to rows that declare seasons. A film
            // release inside a series' results has an empty list, and so does a
            // complete-series pack on some trackers — dropping those would hide
            // exactly the release a viewer starting a series wants.
            if let (Some(want), false) = (season, row.seasons.is_empty()) {
                if !row.seasons.contains(&want) {
                    continue;
                }
            }
            // The same release is on several trackers and cross-posted within
            // one; the info hash is what says they are the same bytes.
            let hash = magnet_hash(&row.magnet);
            if !seen.insert(hash) {
                continue;
            }
            out.push(Release {
                title: row.title,
                tracker: row.tracker,
                size: row.size,
                seeders: row.sid,
                peers: row.pir,
                quality: row.quality,
                video_type: row.videotype,
                voices: row.voices,
                seasons: row.seasons,
                magnet: row.magnet,
                created: row.create_time,
            });
        }
        // Enough to choose from. A second query on top of a full first one adds
        // duplicates of what is already there far more often than it adds a
        // release the first spelling missed.
        if out.len() >= MAX_RELEASES {
            break;
        }
    }

    sort_releases(&mut out);
    out.truncate(MAX_RELEASES);
    Ok(out)
}

/// Where a release's dynamic range puts it: 1 for anything the indexer flagged
/// as high dynamic range, 0 for ordinary.
///
/// Deliberately two buckets rather than a ladder. Measured across 768 rows from
/// the live public instance, the field only ever held `sdr` and `hdr` — so
/// ranking Dolby Vision above HDR10 would be a distinction invented here rather
/// than one the data makes, and it is not obviously the right way round anyway
/// (DV looks better on a display that handles it and worse on one that does
/// not). Anything unrecognised is treated as high, because the indexer only
/// fills this in when it found something: an empty value is the ordinary case
/// and a value we have not seen is more likely a new HDR flavour than a new way
/// of writing "sdr".
fn dynamic_rank(video_type: &str) -> i32 {
    match video_type.trim().to_ascii_lowercase().as_str() {
        "" | "sdr" => 0,
        _ => 1,
    }
}

/// Order the releases the way somebody choosing one actually reads them.
///
/// **Quality is the outer key and dynamic range the inner one**, so the list
/// runs 4K HDR → 4K SDR → 1080p HDR → 1080p SDR → … with seeders deciding
/// inside each group. That is a different answer from sorting by seeders alone,
/// which was the first version: it put a live 480p rip above a 4K HDR remux
/// with a healthy swarm, and the question a viewer is asking is "what is the
/// best copy I can get", not "what is the busiest".
///
/// **Except that a release nobody is seeding sinks to the bottom regardless.**
/// That is the one place this departs from a pure quality order, and it is
/// measured rather than defensive: of 95 4K rows in one live response, **13 had
/// no seeders at all**. Without this the top of the list is routinely occupied
/// by the best-looking thing that will never download, which is the worst
/// possible first row — "cannot be watched" outranks "would look nicer".
/// Nothing is hidden: they are still listed, still marked, still pickable.
///
/// Size breaks the last tie because between two otherwise identical releases
/// the bigger one is the less compressed.
///
/// Note this also decides *which* releases survive `MAX_RELEASES`, so it is a
/// selection order as well as a display order. The frontend may re-sort what
/// comes back — see `sortedReleases` — but it cannot recover a row this dropped.
fn sort_releases(out: &mut [Release]) {
    out.sort_by(|a, b| {
        (b.seeders > 0)
            .cmp(&(a.seeders > 0))
            .then(b.quality.cmp(&a.quality))
            .then(dynamic_rank(&b.video_type).cmp(&dynamic_rank(&a.video_type)))
            .then(b.seeders.cmp(&a.seeders))
            .then(b.size.cmp(&a.size))
    });
}

/// The info hash out of a magnet, lower-cased, or the whole link when there is
/// none to read — an unparseable magnet is still its own identity.
fn magnet_hash(magnet: &str) -> String {
    magnet
        .split(|c| c == '&' || c == '?')
        .find_map(|part| part.strip_prefix("xt=urn:btih:"))
        .map(|h| h.to_lowercase())
        .unwrap_or_else(|| magnet.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_crosses_the_alphabets() {
        // **Written with escapes on purpose.** These pairs are homoglyphs, so
        // spelled out as literals the two sides of each assertion look
        // character-for-character identical in the file — which is how the
        // first version of this test came to compare a string with itself and
        // pin nothing at all. The escape is what makes the case visible to a
        // reader as well as to the compiler.
        //
        // "Матрица" is a word, not a title: it is picked because every letter
        // that matters is here — М/M, а/a and р/p all have Latin twins — and a
        // release name routinely arrives with some of them substituted.
        // All-Cyrillic first, then the same word with Latin M, a and a.
        let cyrillic = "\u{041C}\u{0430}\u{0442}\u{0440}\u{0438}\u{0446}\u{0430}";
        let mixed = "\u{004D}\u{0061}\u{0442}\u{0440}\u{0438}\u{0446}\u{0061}";
        assert_ne!(cyrillic, mixed, "the fixtures must be different strings");
        assert_eq!(fold(cyrillic), fold(mixed));
        // Cyrillic С against Latin C at the head of a word.
        assert_eq!(fold("\u{0421}osmos"), fold("Cosmos"));
        // Case and punctuation, which is the ordinary half of the job.
        assert_eq!(fold("The Matrix"), fold("the  matrix!"));
        // And it must still tell genuinely different titles apart, or every
        // sequel in a series matches its predecessor.
        assert_ne!(fold("Матрица"), fold("Матрица 2"));
        // Two words differing by one letter, neither of which folds into the
        // other: `д`/`л` have no Latin twin, so this is the case the fold must
        // *not* collapse.
        assert_ne!(fold("дом"), fold("лом"));
    }

    #[test]
    fn magnet_hash_reads_the_btih() {
        assert_eq!(
            magnet_hash("magnet:?xt=urn:btih:61065EA115B7CC3E8DB9FB5AB1F6F327F08BD1C9&tr=http://x"),
            "61065ea115b7cc3e8db9fb5ab1f6f327f08bd1c9"
        );
        // Two links to the same torrent differing only in their tracker list
        // must dedupe, which is the whole reason this is not a string compare.
        assert_eq!(
            magnet_hash("magnet:?xt=urn:btih:ABC&tr=one"),
            magnet_hash("magnet:?xt=urn:btih:abc&tr=two&dn=name")
        );
    }

    fn rel(quality: i64, video_type: &str, seeders: i64, size: u64) -> Release {
        Release {
            title: format!("{quality}p {video_type} s{seeders}"),
            tracker: "t".into(),
            size,
            seeders,
            peers: 0,
            quality,
            video_type: video_type.into(),
            voices: vec![],
            seasons: vec![],
            magnet: format!("magnet:?xt=urn:btih:{quality}{video_type}{seeders}{size}"),
            created: String::new(),
        }
    }

    #[test]
    fn releases_group_by_quality_then_dynamic_range() {
        // Deliberately shuffled, and every pair below differs in exactly one
        // key — so a reordering of the comparison chain shows up as a specific
        // swap rather than as "the list looks different".
        let mut v = vec![
            rel(1080, "sdr", 900, 5),  // busiest of all, and still not first
            rel(2160, "hdr", 10, 5),
            rel(720, "hdr", 500, 5),
            rel(2160, "sdr", 400, 5),
            rel(2160, "hdr", 50, 5),
            rel(1080, "hdr", 3, 5),
        ];
        sort_releases(&mut v);
        let order: Vec<_> = v.iter().map(|r| (r.quality, r.video_type.as_str(), r.seeders)).collect();
        assert_eq!(
            order,
            vec![
                (2160, "hdr", 50),
                (2160, "hdr", 10),
                (2160, "sdr", 400),
                (1080, "hdr", 3),
                (1080, "sdr", 900),
                (720, "hdr", 500),
            ],
            "quality is the outer key, dynamic range the inner one, seeders decide inside a group"
        );
    }

    #[test]
    fn a_release_nobody_seeds_sinks_to_the_bottom() {
        // Measured on a live response: 13 of 95 4K rows had no seeders at all,
        // so without this the first row is routinely the best-looking thing that
        // will never download.
        let mut v = vec![
            rel(2160, "hdr", 0, 9),
            rel(480, "sdr", 1, 9),
            rel(2160, "hdr", 0, 20),
        ];
        sort_releases(&mut v);
        assert_eq!(v[0].quality, 480, "the only live release must come first");
        // And among the dead ones the ordinary rules still apply, so the list
        // does not become arbitrary below the fold — bigger first on a tie.
        assert_eq!((v[1].quality, v[1].size), (2160, 20));
        assert_eq!((v[2].quality, v[2].size), (2160, 9));
    }

    #[test]
    fn dynamic_rank_buckets_anything_flagged() {
        assert_eq!(dynamic_rank("sdr"), 0);
        assert_eq!(dynamic_rank(""), 0);
        assert_eq!(dynamic_rank("SDR"), 0);
        // Only `sdr` and `hdr` were observed, so an unrecognised value is far
        // more likely a new HDR flavour than a new spelling of "ordinary".
        assert_eq!(dynamic_rank("hdr"), 1);
        assert_eq!(dynamic_rank("HDR10"), 1);
        assert_eq!(dynamic_rank("dv"), 1);
    }

    #[test]
    fn proxy_base_refuses_plaintext_off_loopback() {
        // What travels here is what somebody is searching for, and over `http://`
        // the failure is invisible because it works — the same reasoning that
        // makes `socketUrl` refuse a plaintext relay.
        assert!(proxy_base("http://example.org").is_err());
        assert!(proxy_base("ws://example.org").is_err());
        assert!(proxy_base("example.org").is_err());
        assert!(proxy_base("   ").is_err());
        // Loopback is exempt, so a proxy on this machine needs no certificate.
        assert_eq!(proxy_base("http://127.0.0.1:8090").unwrap(), "http://127.0.0.1:8090");
        assert_eq!(proxy_base("http://localhost:8090/").unwrap(), "http://localhost:8090");
        // And the trailing slash goes, or every URL built from it doubles one.
        assert_eq!(proxy_base("https://example.org///").unwrap(), "https://example.org");
    }

    #[test]
    fn tmdb_lang_is_a_full_tag() {
        assert_eq!(tmdb_lang("ru"), "ru-RU");
        assert_eq!(tmdb_lang("en"), "en-US");
    }
}
