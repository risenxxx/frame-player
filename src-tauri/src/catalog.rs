//! The catalog: **what** to watch (TMDB) and **where to get it** (a release
//! search server the viewer names — Torznab, or the jacred-format API).
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
    /// Empty when the server named no hash — then `torrent` is the way in.
    pub magnet: String,
    /// A `.torrent` download link, which Torznab gives where a tracker has no
    /// magnet (and where it is private, the only link that works). Empty from
    /// the jacred API. May carry the server's key, so it is used once to fetch
    /// the file and never remembered.
    pub torrent: String,
    pub created: String,
    /// The tracker's own page for this release. **Measured unique and stable**:
    /// 768 distinct URLs across 768 rows, so it is the indexer's identity for a
    /// row and the only exact handle we get on "this same release, later".
    pub url: String,
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

/// One row of the jacred-format API's `/api/v1.0/torrents`.
///
/// The field names are the API's, typo included — `relased` is what it answers
/// and renaming it here would only move the surprise. Everything is optional
/// because instances differ in what they fill in, and a row missing a dub list
/// must not cost the whole response.
#[derive(serde::Deserialize, Default)]
struct IndexerRow {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
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

/// How long a Torznab search may take. Longer than everything else here on
/// purpose: Jackett's `all` and a Prowlarr in front of several trackers answer
/// only once the slowest tracker behind them has, and that is routinely more
/// than the fifteen seconds a single service gets.
const TORZNAB_TIMEOUT: Duration = Duration::from_secs(45);

/// Which of the two dialects an address speaks.
///
/// **Torznab is the one the interface names**; it is what Jackett and Prowlarr
/// serve, and what the *arr applications have taught people to look for. The
/// jacred-format JSON API is the one this catalog was first written against,
/// and it stays working for anybody who already has such an address set — it
/// is simply not advertised.
enum Indexer {
    /// The base address, without a trailing slash.
    Jacred(String),
    /// The full Torznab endpoint, `/api` included, the key already in its query.
    Torznab(reqwest::Url),
}

/// Read the address the viewer gave, and the key beside it.
///
/// **Torznab is told apart by what a Torznab address always has**: a key (both
/// Jackett and Prowlarr refuse a search without one), a `torznab` in Jackett's
/// path, or the `/api` endpoint itself. Anything else is the jacred base it
/// always was. Asking the server instead would cost a round trip on every
/// search to learn something the address already says.
///
/// What people copy is not always the endpoint: Jackett's "Copy Torznab Feed"
/// ends in `/torznab/` and Prowlarr's indexer address in `/<id>/`, and the *arr
/// applications append `/api` themselves. So does this — and any `t` or `q` a
/// pasted search URL still carries is dropped, since those are ours to set.
///
/// Plaintext is **not** refused here, unlike the metadata proxy's address: a
/// Jackett or Prowlarr lives on this machine or on a NAS in the same flat, and
/// is served over `http://` there almost without exception.
fn indexer(base: &str, key: &str) -> Result<Indexer, String> {
    let base = base.trim();
    if base.is_empty() {
        return Err("no_indexer".into());
    }
    let mut url = reqwest::Url::parse(base).map_err(|_| "bad_indexer".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("bad_indexer".into());
    }
    let key = key.trim();
    let path = url.path().trim_end_matches('/').to_string();
    let lower = path.to_ascii_lowercase();
    let key_in_query = url.query_pairs().any(|(k, _)| k.eq_ignore_ascii_case("apikey"));
    if key.is_empty() && !key_in_query && !lower.contains("torznab") && !lower.ends_with("/api") {
        return Ok(Indexer::Jacred(base.trim_end_matches('/').to_string()));
    }
    if !lower.ends_with("/api") {
        url.set_path(&format!("{path}/api"));
    }
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| {
            let k = k.to_ascii_lowercase();
            // The field's key wins over one left in the address.
            k != "t" && k != "q" && (k != "apikey" || key.is_empty())
        })
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    url.set_query(None);
    if !kept.is_empty() || !key.is_empty() {
        let mut q = url.query_pairs_mut();
        for (k, v) in &kept {
            q.append_pair(k, v);
        }
        if !key.is_empty() {
            q.append_pair("apikey", key);
        }
    }
    Ok(Indexer::Torznab(url))
}

/// One release as either dialect answered it, before the filters.
///
/// The filters need two things the frontend does not: the names to compare
/// against what was searched for, and every year the release mentions.
struct Candidate {
    release: Release,
    names: Vec<String>,
    years: Vec<i32>,
}

/// Turn a transport failure into the three answers the panel tells apart.
fn indexer_status(status: reqwest::StatusCode) -> Result<(), String> {
    match status.as_u16() {
        200..=299 => Ok(()),
        401 | 403 => Err("indexer_key".into()),
        404 => Err("indexer_not_found".into()),
        n => Err(format!("http_{n}")),
    }
}

async fn ask_jacred(base: &str, query: &str) -> Result<Vec<Candidate>, String> {
    let url = reqwest::Url::parse_with_params(
        &format!("{base}/api/v1.0/torrents"),
        &[("search", query), ("apikey", "null")],
    )
    .map_err(|_| "bad_indexer".to_string())?;
    let response = http()
        .get(url)
        .send()
        .await
        .map_err(|_| "indexer_unreachable".to_string())?;
    indexer_status(response.status())?;
    // Tolerant on purpose: an instance that adds a field must not break the
    // whole search, and one row that will not parse must not take the rest with
    // it — which is the failure the librqbit tracker client already paid for,
    // where a strict parser silently discarded every peer in a valid response.
    // A body that is not a JSON list at all is a server that is not this API.
    let rows: Vec<serde_json::Value> = response
        .json()
        .await
        .map_err(|_| "indexer_not_found".to_string())?;
    Ok(rows
        .into_iter()
        .filter_map(|v| serde_json::from_value::<IndexerRow>(v).ok())
        .filter(|row| !row.magnet.is_empty())
        .map(|row| Candidate {
            // The API's own parse of the release name is what is compared,
            // never the raw tracker title: that string carries the year, the
            // codec and the dub list, so a substring test against it matches
            // anything that merely mentions the film.
            names: vec![row.name, row.originalname],
            years: (row.relased > 0).then_some(row.relased).into_iter().collect(),
            release: Release {
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
                torrent: String::new(),
                created: row.create_time,
                url: row.url,
            },
        })
        .collect())
}

/// The error a Torznab server answers with instead of a feed, as its code.
///
/// The spec's `<error code="…" description="…"/>` arrives with a 200 from some
/// servers and with a 4xx from others, so the body is read either way. `0`
/// when the element is there and its code is not.
fn torznab_error(text: &str) -> Option<u32> {
    let mut head = text.trim_start();
    if head.starts_with("<?xml") {
        head = head.split_once("?>").map(|(_, rest)| rest.trim_start()).unwrap_or(head);
    }
    let rest = head.strip_prefix("<error")?;
    let tag = rest.split('>').next().unwrap_or(rest);
    let code = tag
        .split_once("code=")
        .map(|(_, v)| {
            v.trim_start_matches(['"', '\''])
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
        })
        .and_then(|digits| digits.parse().ok());
    Some(code.unwrap_or(0))
}

async fn ask_torznab(endpoint: &reqwest::Url, query: &str) -> Result<Vec<Candidate>, String> {
    let mut url = endpoint.clone();
    // `search` rather than `movie`/`tvsearch`: it is the one function every
    // server and every indexer behind it implements, and the filtering below is
    // ours anyway — the same reasoning that sends the jacred API `search` alone.
    url.query_pairs_mut()
        .append_pair("t", "search")
        .append_pair("q", query);
    let response = http()
        .get(url)
        .timeout(TORZNAB_TIMEOUT)
        .send()
        .await
        .map_err(|_| "indexer_unreachable".to_string())?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|_| "indexer_unreachable".to_string())?;
    let text = crate::feed::decode_body(&bytes);
    if let Some(code) = torznab_error(&text) {
        // 100–102 are the spec's three ways of saying the key is wrong.
        return Err(if (100..=102).contains(&code) {
            "indexer_key".into()
        } else {
            "indexer_error".into()
        });
    }
    indexer_status(status)?;
    let feed = crate::feed::parse_feed(&text).map_err(|_| "indexer_not_found".to_string())?;
    Ok(feed.items.into_iter().filter_map(torznab_candidate).collect())
}

fn torznab_candidate(item: crate::feed::FeedItem) -> Option<Candidate> {
    // A magnet only when it names the hash the item reports, so that one
    // release is one hash everywhere downstream — a base32 magnet beside a hex
    // hash would otherwise read as two torrents to the update check.
    let magnet = match (&item.info_hash, item.magnet) {
        (Some(hash), Some(m)) if m.to_ascii_lowercase().contains(hash.as_str()) => m,
        (Some(hash), _) => format!("magnet:?xt=urn:btih:{hash}"),
        (None, Some(m)) => m,
        (None, None) => String::new(),
    };
    let torrent = item.torrent_url.unwrap_or_default();
    if magnet.is_empty() && torrent.is_empty() {
        return None;
    }
    let title = item.title;
    let mut years = title_years(&title);
    years.extend(item.year);
    Some(Candidate {
        names: title_names(&title),
        years,
        release: Release {
            quality: title_quality(&title),
            video_type: title_dynamic(&title).to_string(),
            seasons: title_seasons(&title),
            // A dub list is the one field there is no honest way to read out of
            // a release name across trackers, so it is left absent rather than
            // guessed — the panel already draws a row without one.
            voices: Vec::new(),
            tracker: item.source.unwrap_or_default(),
            size: item.size.unwrap_or(0),
            seeders: item.seeders.unwrap_or(0),
            peers: item.peers.unwrap_or(0),
            magnet,
            torrent,
            created: item.published.map(iso_time).unwrap_or_default(),
            url: item.page.unwrap_or_default(),
            title,
        },
    })
}

async fn ask(indexer: &Indexer, query: &str) -> Result<Vec<Candidate>, String> {
    match indexer {
        Indexer::Jacred(base) => ask_jacred(base, query).await,
        Indexer::Torznab(endpoint) => ask_torznab(endpoint, query).await,
    }
}

// ---- Reading a release name --------------------------------------------------
//
// The jacred API hands back its own parse of every release name; Torznab hands
// back the name and nothing else. So for Torznab the quality, the dynamic
// range, the seasons, the years and the title itself are read here, out of the
// two shapes release names come in: the scene's `Title.2024.2160p.WEB-DL…` and
// the forum tracker's `Название / Title (Director) [2024, Country, WEB-DL 1080p]`.

/// The alphanumeric runs of a name, lower-cased, each with the text before it.
fn words(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut sep = String::new();
    let mut word = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() {
            word.extend(c.to_lowercase());
        } else {
            if !word.is_empty() {
                out.push((std::mem::take(&mut sep), std::mem::take(&mut word)));
            }
            sep.push(c);
        }
    }
    if !word.is_empty() {
        out.push((sep, word));
    }
    out
}

/// `1080p` → 1080. Only the `p`/`i` spellings: a bare number is never a height.
fn height(word: &str) -> Option<i64> {
    let digits = word.strip_suffix('p').or_else(|| word.strip_suffix('i'))?;
    let n: i64 = digits.parse().ok()?;
    matches!(n, 360 | 480 | 540 | 576 | 720 | 1080 | 1440 | 2160 | 4320).then_some(n)
}

/// 480/720/1080/2160, or 0 when the name does not say.
///
/// A written height wins over `4K`/`UHD`, because those also appear in names
/// as a claim about the source ("4K Remaster") on a 1080p rip.
fn title_quality(title: &str) -> i64 {
    let w = words(title);
    if let Some(h) = w.iter().filter_map(|(_, x)| height(x)).max() {
        return h;
    }
    if w.iter().any(|(_, x)| x == "4k" || x == "uhd") {
        2160
    } else {
        0
    }
}

/// `dv`, `hdr` or empty, which is all `dynamic_rank` and the tags distinguish.
fn title_dynamic(title: &str) -> &'static str {
    let w = words(title);
    let has = |want: &str| w.iter().any(|(_, x)| x == want);
    if has("dv") || has("dovi") || w.windows(2).any(|p| p[0].1 == "dolby" && p[1].1 == "vision") {
        return "dv";
    }
    if w.iter().any(|(_, x)| matches!(x.as_str(), "hdr" | "hdr10" | "hdr10plus" | "hlg")) {
        "hdr"
    } else {
        ""
    }
}

/// Every four-digit year a name mentions.
///
/// All of them rather than one, because which one is the release year is not
/// knowable from the name: `Blade Runner 2049 (2017)`, `1917 (2019)`, and a
/// series' `[2019-2024]`. The filter then asks whether *any* of them fits.
fn title_years(title: &str) -> Vec<i32> {
    words(title)
        .iter()
        .filter(|(_, w)| w.len() == 4)
        .filter_map(|(_, w)| w.parse::<i32>().ok())
        .filter(|y| (1900..=2100).contains(y))
        .collect()
}

const SEASON_WORDS: [&str; 6] = ["season", "seasons", "сезон", "сезоны", "сезона", "сезонов"];

/// `s01`, `s1`, `s01e05` → the season.
fn tagged_season(word: &str) -> Option<i32> {
    let rest = word.strip_prefix('s')?;
    let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits > 2 {
        return None;
    }
    let tail = &rest[digits..];
    let episode = tail
        .strip_prefix('e')
        .is_some_and(|e| !e.is_empty() && e.chars().all(|c| c.is_ascii_digit()));
    if !tail.is_empty() && !episode {
        return None;
    }
    rest[..digits].parse().ok().filter(|&n| n > 0)
}

fn small_number(word: &str) -> Option<i32> {
    (word.len() <= 2 && word.bytes().all(|b| b.is_ascii_digit()))
        .then(|| word.parse().ok())
        .flatten()
        .filter(|&n| n > 0)
}

/// The seasons a name says it holds: `S02`, `S01-S03`, `S01E05`, `Season 2`,
/// `Сезон: 1`, `Сезоны 1-4`, `3 сезон`.
///
/// Empty when it says nothing, and that is common: a film, a complete-series
/// pack, a tracker that writes the season only on the page. The season filter
/// keeps such rows rather than guessing, exactly as it does for the jacred API.
fn title_seasons(title: &str) -> Vec<i32> {
    let w = words(title);
    let is_range = |sep: &str| sep.contains('-') || sep.contains('–');
    let mut out: Vec<i32> = Vec::new();
    for i in 0..w.len() {
        let word = w[i].1.as_str();
        // Where the first number is, and the index of the word holding it.
        let start = if let Some(n) = tagged_season(word) {
            Some((n, i))
        } else if SEASON_WORDS.contains(&word) {
            match w.get(i + 1).and_then(|(_, x)| small_number(x)) {
                Some(n) => Some((n, i + 1)),
                // "3 сезон", "1-3 сезон": the number came first.
                None if i > 0 => small_number(&w[i - 1].1).map(|n| {
                    match (i > 1 && is_range(&w[i - 1].0)).then(|| small_number(&w[i - 2].1)) {
                        Some(Some(first)) if first <= n => {
                            out.extend(first..n);
                            (n, i)
                        }
                        _ => (n, i),
                    }
                }),
                None => None,
            }
        } else {
            None
        };
        let Some((first, at)) = start else { continue };
        let last = w
            .get(at + 1)
            .filter(|(sep, _)| is_range(sep))
            .and_then(|(_, x)| tagged_season(x).or_else(|| small_number(x)))
            .filter(|&n| n >= first && n - first <= 40)
            .unwrap_or(first);
        out.extend(first..=last);
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// A word that ends a title inside a release name.
fn ends_title(word: &str) -> bool {
    (word.len() == 4 && word.parse::<i32>().is_ok_and(|y| (1900..=2100).contains(&y)))
        || height(word).is_some()
        || tagged_season(word).is_some()
        || SEASON_WORDS.contains(&word)
        || matches!(
            word,
            "4k" | "uhd"
                | "web" | "webrip" | "webdl" | "webdlrip"
                | "bdrip" | "bdremux" | "remux" | "bluray" | "blu"
                | "hdtv" | "hdtvrip" | "hdrip" | "dvdrip" | "dvd"
                | "x264" | "x265" | "h264" | "h265" | "hevc" | "avc"
                | "complete" | "серии" | "серия"
        )
}

/// The names a release could be filed under, for an exact comparison.
///
/// **Whole names, compared after `fold`, never a substring of the title** —
/// the rule the jacred path keeps by comparing the API's own parse, and the
/// reason that path never matched anything that merely mentioned the film. A
/// forum name is split on ` / ` into its local and original names; a scene name
/// has its dots turned back into spaces; and each part also offers its prefix
/// before every word that ends a title (a year, a height, a season, a source),
/// since where the title stops is exactly what a name does not mark. That makes
/// `Blade Runner` a candidate of `Blade.Runner.2049.2017.1080p` as well as
/// `Blade Runner 2049` — harmless, because the year filter then sees 2049 and
/// 2017 and neither is 1982.
fn title_names(title: &str) -> Vec<String> {
    let mut s = title.trim();
    // A leading release group, `[Group] Name - 05 (1080p)`.
    while let Some(rest) = s
        .strip_prefix('[')
        .and_then(|r| r.split_once(']'))
        .map(|(_, r)| r.trim_start())
    {
        s = rest;
    }
    let head = s.split(['(', '[', '|']).next().unwrap_or(s);
    let mut out: Vec<String> = Vec::new();
    let mut push = |name: String| {
        if !name.is_empty() && !out.contains(&name) {
            out.push(name);
        }
    };
    for part in head.split(" / ") {
        let part = if part.trim().contains(' ') {
            part.to_string()
        } else {
            part.replace(['.', '_'], " ")
        };
        let mut so_far = String::new();
        for (i, (sep, word)) in words(&part).iter().enumerate() {
            if i > 0 && ends_title(word) {
                push(so_far.clone());
            }
            // An episode's ` - 05` ends the title as well.
            if i > 0 && (sep.contains(" - ") || sep.contains(" – ")) {
                push(so_far.clone());
            }
            if !so_far.is_empty() {
                so_far.push(' ');
            }
            so_far.push_str(word);
        }
        push(so_far);
    }
    out
}

/// Seconds since the epoch as `YYYY-MM-DDTHH:MM:SSZ`, which sorts as text —
/// `created` is only ever compared, by `catalog_find_update`.
fn iso_time(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days, the inverse of `feed::epoch`.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

/// Find the releases for one title.
///
/// **Searched by the original name first.** A Russian tracker files a foreign
/// film under both names, and the original is the one that survives translation
/// — TMDB's localised title is one of several possible renderings, while the
/// original is what the uploader typed. The localised one is the fallback and
/// the extra query, not the first guess. The two are asked at once: a Torznab
/// aggregator answers only when its slowest tracker has, and two of those in a
/// row is a wait nobody should sit through.
///
/// Filtering happens here rather than in the query, because which parameters a
/// server honours varies — by dialect, by version, and for Torznab by every
/// tracker behind it. A plain search is the one thing all of them do; year and
/// season are matched against what comes back.
#[tauri::command]
pub async fn catalog_releases(
    base: String,
    key: Option<String>,
    title: String,
    original_title: String,
    year: Option<i32>,
    season: Option<i32>,
) -> Result<Vec<Release>, String> {
    let indexer = indexer(&base, key.as_deref().unwrap_or(""))?;
    let mut queries: Vec<String> = Vec::new();
    for q in [original_title.trim(), title.trim()] {
        if !q.is_empty() && !queries.iter().any(|had| fold(had) == fold(q)) {
            queries.push(q.to_string());
        }
    }
    if queries.is_empty() {
        return Err("no_query".into());
    }

    let (first, second) = tokio::join!(ask(&indexer, &queries[0]), async {
        match queries.get(1) {
            Some(q) => Some(ask(&indexer, q).await),
            None => None,
        }
    });
    // The first query failing is the indexer being unreachable and is worth
    // reporting — but only when the other one did not answer either: results in
    // hand are not worth losing over one failed spelling.
    let answers: Vec<Vec<Candidate>> = match (first, second) {
        (Err(e), None | Some(Err(_))) => return Err(e),
        (a, b) => a.into_iter().chain(b.and_then(Result::ok)).collect(),
    };

    let wanted: Vec<String> = queries.iter().map(|q| fold(q)).collect();
    let mut out: Vec<Release> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for rows in answers {
        for c in rows {
            let matches_name = c
                .names
                .iter()
                .filter(|n| !n.is_empty())
                .any(|n| wanted.contains(&fold(n)));
            if !matches_name {
                continue;
            }
            // A year off by one is routine — a festival run, a national release
            // date, an indexer reading it out of the file name — so the window
            // is ±1 rather than exact, and a row with no year at all is kept:
            // refusing it would drop releases whose name matched exactly. With
            // a season asked for, a later year is fine too: TMDB's year is the
            // series' first, and season three came out years after it.
            if let (Some(want), false) = (year, c.years.is_empty()) {
                let fits = |y: &i32| {
                    (y - want).abs() <= 1 || (season.is_some_and(|s| s > 1) && *y > want)
                };
                if !c.years.iter().any(fits) {
                    continue;
                }
            }
            let row = c.release;
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
            // one; the info hash is what says they are the same bytes. A row
            // with only a `.torrent` link has no hash to compare, so its link
            // stands in for one.
            let identity = if row.magnet.is_empty() {
                row.torrent.clone()
            } else {
                magnet_hash(&row.magnet)
            };
            if !seen.insert(identity) {
                continue;
            }
            out.push(row);
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

/// What a release's `.torrent` link turned out to be.
#[derive(Serialize)]
pub struct ReleaseSource {
    pub magnet: Option<String>,
    pub info_hash: Option<String>,
    pub name: Option<String>,
}

/// A season's `.torrent` is a few hundred kilobytes; past this it is a page.
const MAX_TORRENT_BYTES: usize = 32 * 1024 * 1024;

/// Fetch a release's `.torrent` from the server that listed it, and put the
/// metadata where `torrent_add` looks first.
///
/// The same move as `feed_torrent`, for the same reason: a magnet built from
/// the hash then opens without a DHT lookup — and on a private tracker, where
/// the DHT is off, the `.torrent` is the only way in at all. Two differences.
/// It goes **direct**, like every other request to the indexer, rather than
/// through the torrent proxy: the link points at the same Jackett or Prowlarr
/// the search just reached, often on this machine. And it does not follow
/// redirects blindly, because for a tracker that only has magnets both of them
/// answer the download link with a redirect *to* the magnet.
#[tauri::command]
pub async fn catalog_torrent(app: tauri::AppHandle, url: String) -> Result<ReleaseSource, String> {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    let client = CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(format!("FramePlayer/{}", env!("CARGO_PKG_VERSION")))
            .timeout(TORZNAB_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_default()
    });
    let mut next = reqwest::Url::parse(url.trim()).map_err(|_| "bad_url".to_string())?;
    for _ in 0..5 {
        if !matches!(next.scheme(), "http" | "https") {
            return Err("bad_url".into());
        }
        let response = client
            .get(next.clone())
            .send()
            .await
            .map_err(|_| "indexer_unreachable".to_string())?;
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or("bad_url")?;
            if location.starts_with("magnet:") {
                return Ok(ReleaseSource {
                    magnet: Some(location.to_string()),
                    info_hash: None,
                    name: None,
                });
            }
            next = next.join(location).map_err(|_| "bad_url".to_string())?;
            continue;
        }
        indexer_status(response.status())?;
        if response
            .content_length()
            .is_some_and(|n| n as usize > MAX_TORRENT_BYTES)
        {
            return Err("too_large".into());
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|_| "indexer_unreachable".to_string())?;
        if bytes.len() > MAX_TORRENT_BYTES {
            return Err("too_large".into());
        }
        let (info_hash, name) = crate::torrent::cache_metadata(&app, &bytes)?;
        return Ok(ReleaseSource {
            magnet: None,
            info_hash: Some(info_hash),
            name,
        });
    }
    Err("bad_url".into())
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

/// Look for a newer release of something already on disk.
///
/// **Two paths, and the measurement says which one actually pays.** The obvious
/// one is exact: a release's tracker URL is its identity at the indexer (768
/// unique URLs across 768 rows), so the same URL carrying a different magnet
/// means the uploader replaced the torrent on the same page. The other is
/// fuzzy: a *different* page whose parsed name is close enough to be the same
/// release, published later.
///
/// Measured against the live service, the exact path is the rare one. Of 464
/// rows in one broad response, **1 had been re-crawled in the last 90 days and
/// 6 in 180** — 456 of them shared a single sweep date. New rows appear
/// promptly (the service reports hundreds a day), old rows are revisited in
/// occasional bulk passes. So a re-upload that creates a new page is found
/// quickly, while an edit to an existing page may not surface for months.
///
/// Both are therefore tried, exact first, and **neither is applied here** — the
/// answer goes back to the caller as a candidate. Replacing a season's torrent
/// costs a re-check of everything already on disk, which is not a thing to do
/// on a guess.
#[tauri::command]
pub async fn catalog_find_update(
    base: String,
    key: Option<String>,
    title: String,
    original_title: String,
    year: Option<i32>,
    season: Option<i32>,
    known_url: String,
    known_hash: String,
    known_name: String,
    known_quality: i64,
) -> Result<Option<Release>, String> {
    let mut releases = catalog_releases(base, key, title, original_title, year, season).await?;
    // **Only a release whose hash is known can be an update.** Both paths below
    // decide "different torrent" by comparing hashes, and a Torznab row that
    // carries nothing but a `.torrent` link has none — so it would differ from
    // every torrent, including the one already on disk.
    releases.retain(|r| !r.magnet.is_empty());
    let known_hash = known_hash.to_lowercase();
    let known_url = known_url.trim();

    // Exact: the same page, a different torrent on it.
    if !known_url.is_empty() {
        if let Some(hit) = releases
            .iter()
            .find(|r| r.url == known_url && magnet_hash(&r.magnet) != known_hash)
        {
            return Ok(Some(hit.clone()));
        }
    }

    // Fuzzy: another page, the same release by name, published later. Ordered by
    // date so the newest candidate wins rather than whichever the sort happened
    // to put first — the release list is ordered for *choosing*, not for this.
    let mut candidates: Vec<&Release> = releases
        .iter()
        .filter(|r| {
            magnet_hash(&r.magnet) != known_hash
                && r.url != known_url
                // **Same quality, or it is not an update.** The name alone is
                // not enough, and that is measured rather than assumed: two
                // releases of one series differing only in source and
                // resolution share 7 of 11 tokens, which clears the 0.6
                // threshold. Offering one as an update to the other would
                // re-check everything on disk and fetch a different rip
                // entirely. A release that changes quality is a different
                // release; `0` means the indexer could not read it, and an
                // unknown must not match anything.
                && known_quality > 0
                && r.quality == known_quality
                && looks_like_same_release(&r.title, &known_name)
        })
        .collect();
    candidates.sort_by(|a, b| b.created.cmp(&a.created));
    Ok(candidates.first().map(|r| (*r).clone()))
}

/// Token overlap, the same rule the frontend already applies when a magnet is
/// pasted into the link box — a re-upload keeps most of its name and changes
/// the episode count in the middle of it, so a prefix test is no use.
///
/// Kept here rather than called across the boundary because this runs over a
/// hundred rows per check and the frontend's copy exists for a single
/// comparison; the threshold is the same 0.6 and a divergence would show up as
/// the two disagreeing about one release, which is a test's job to catch.
fn looks_like_same_release(a: &str, b: &str) -> bool {
    let tokens = |s: &str| -> std::collections::HashSet<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.chars().count() > 1)
            .map(|w| w.to_string())
            .collect()
    };
    let (x, y) = (tokens(a), tokens(b));
    if x.len() < 3 || y.len() < 3 {
        return false;
    }
    let (small, large) = if x.len() <= y.len() { (&x, &y) } else { (&y, &x) };
    let shared = small.iter().filter(|w| large.contains(*w)).count();
    shared as f64 / small.len() as f64 >= 0.6
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
            torrent: String::new(),
            created: String::new(),
            url: String::new(),
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
    fn same_release_matches_a_re_upload_and_not_a_neighbour() {
        // The case this exists for: an uploader adds an episode and the count
        // changes in the middle of a long name. A prefix test fails here, which
        // is why it is token overlap.
        let before = "Some Show / Сериал [2024, WEB-DL 1080p] Серии: 1-8 из 10 Dub + Sub";
        let after = "Some Show / Сериал [2024, WEB-DL 1080p] Серии: 1-10 из 10 Dub + Sub";
        assert!(looks_like_same_release(before, after));

        // **And the name alone is not enough**, which is the finding this test
        // exists to record. Two releases of one series differing only in source
        // and resolution share 7 of 11 tokens — 0.64, over the 0.6 threshold —
        // so token overlap calls them the same release and they are not. That
        // is why `catalog_find_update` also requires the quality to match:
        // offering a 4K remux as an "update" to a 1080p season would re-check
        // everything on disk and fetch a different rip.
        let other = "Some Show / Сериал [2024, BDRemux 2160p HDR] Серии: 1-10 из 10 MVO";
        assert!(
            looks_like_same_release(before, other),
            "if this stops matching, the quality guard in catalog_find_update is no \
             longer load-bearing and the comment there should say so"
        );

        // Too little to judge is a refusal, not a guess: two short names would
        // otherwise match on one shared word.
        assert!(!looks_like_same_release("Show", "Show"));
    }

    fn torznab_url(base: &str, key: &str) -> String {
        match indexer(base, key).unwrap() {
            Indexer::Torznab(url) => url.to_string(),
            Indexer::Jacred(base) => panic!("{base} read as the jacred API"),
        }
    }

    #[test]
    fn an_address_says_which_dialect_it_speaks() {
        // What Jackett's "Copy Torznab Feed" puts on the clipboard, with the
        // key from its dashboard in the field beside it.
        assert_eq!(
            torznab_url("http://127.0.0.1:9117/api/v2.0/indexers/all/results/torznab/", "K"),
            "http://127.0.0.1:9117/api/v2.0/indexers/all/results/torznab/api?apikey=K"
        );
        // Prowlarr's per-indexer address is recognisable only by the key.
        assert_eq!(torznab_url("http://nas.local:9696/3/", "K"), "http://nas.local:9696/3/api?apikey=K");
        // The whole endpoint pasted, key included: kept as it is, and a search
        // the URL still carried is dropped, since `t` and `q` are ours to set.
        assert_eq!(
            torznab_url("http://h:9696/3/api?apikey=K&t=search&q=dune", ""),
            "http://h:9696/3/api?apikey=K"
        );
        // The field's key beats one left in the address.
        assert_eq!(torznab_url("http://h/3/api?apikey=OLD", "NEW"), "http://h/3/api?apikey=NEW");
        // A bare base with no key is the jacred API it always was.
        assert!(matches!(indexer("https://example.org/", ""), Ok(Indexer::Jacred(b)) if b == "https://example.org"));
        // And the three ways of not being an address at all.
        assert_eq!(indexer("  ", "K").err().as_deref(), Some("no_indexer"));
        assert_eq!(indexer("localhost:9117", "").err().as_deref(), Some("bad_indexer"));
        assert_eq!(indexer("not a url", "").err().as_deref(), Some("bad_indexer"));
    }

    #[test]
    fn torznab_errors_are_read_out_of_the_body() {
        assert_eq!(
            torznab_error(r#"<?xml version="1.0" encoding="UTF-8"?><error code="100" description="Invalid API Key" />"#),
            Some(100)
        );
        assert_eq!(torznab_error("<error description=\"x\"/>"), Some(0));
        assert_eq!(torznab_error("<rss><channel></channel></rss>"), None);
        // An item that merely mentions an error is not one.
        assert_eq!(torznab_error("<rss><item><title><error></title></item></rss>"), None);
    }

    #[test]
    fn a_name_offers_its_titles_and_not_its_mentions() {
        let forum = "Дюна: Часть вторая / Dune: Part Two (Дени Вильнёв / Denis Villeneuve) [2024, США, фантастика, WEB-DL 2160p, HDR10] Dub + Original";
        let names: Vec<String> = title_names(forum).iter().map(|n| fold(n)).collect();
        assert!(names.contains(&fold("Dune: Part Two")));
        assert!(names.contains(&fold("Дюна: Часть вторая")));
        // The director in the parentheses is not a title.
        assert!(!names.contains(&fold("Denis Villeneuve")));

        let scene = "Blade.Runner.2049.2017.1080p.BluRay.x264-GRP";
        let names: Vec<String> = title_names(scene).iter().map(|n| fold(n)).collect();
        assert!(names.contains(&fold("Blade Runner 2049")));
        // A prefix offered too — which is why the year filter has to see 2017.
        assert!(names.contains(&fold("Blade Runner")));
        assert_eq!(title_years(scene), vec![2049, 2017]);

        let anime = "[Group] Sousou no Frieren - 05 (1080p) [ABCD1234]";
        let names: Vec<String> = title_names(anime).iter().map(|n| fold(n)).collect();
        assert!(names.contains(&fold("Sousou no Frieren")));

        // And a different film that contains the title is not the title.
        let sequel = "Dune.Part.Three.2026.2160p.WEB-DL";
        assert!(!title_names(sequel).iter().any(|n| fold(n) == fold("Dune: Part Two")));
    }

    #[test]
    fn quality_and_dynamic_range_from_a_name() {
        assert_eq!(title_quality("Film.2024.2160p.WEB-DL.DV.HDR"), 2160);
        assert_eq!(title_quality("Фильм [2024, BDRip 1080p]"), 1080);
        // A written height beats a "4K" that describes the source.
        assert_eq!(title_quality("Film 4K Remaster 1080p"), 1080);
        assert_eq!(title_quality("Film UHD BDRemux"), 2160);
        assert_eq!(title_quality("Film DVDRip"), 0);
        assert_eq!(title_dynamic("Film.2160p.DV.HDR10"), "dv");
        assert_eq!(title_dynamic("Film 2160p Dolby Vision"), "dv");
        assert_eq!(title_dynamic("Film [WEB-DL 2160p, HDR10+]"), "hdr");
        // HDRip and DVDRip are rips, not dynamic range.
        assert_eq!(title_dynamic("Film HDRip DVDRip"), "");
    }

    #[test]
    fn seasons_from_a_name() {
        assert_eq!(title_seasons("Show.S02E05.1080p"), vec![2]);
        assert_eq!(title_seasons("Show S01-S03 Complete"), vec![1, 2, 3]);
        assert_eq!(title_seasons("Show.S01-03.1080p"), vec![1, 2, 3]);
        assert_eq!(title_seasons("Сериал / Show / Сезон: 2 / Серии: 1-8 из 8 [2024]"), vec![2]);
        assert_eq!(title_seasons("Сериал (Сезоны 1-4) [2019-2024]"), vec![1, 2, 3, 4]);
        assert_eq!(title_seasons("Сериал 3 сезон"), vec![3]);
        assert_eq!(title_seasons("Show Season 2 1080p"), vec![2]);
        // An episode range is not a season range.
        assert_eq!(title_seasons("Show.S01E01-E05"), vec![1]);
        // A film says nothing, and nothing is what comes back.
        assert!(title_seasons("Film.2024.1080p.WEB-DL").is_empty());
        assert!(title_seasons("Seven Samurai 1954").is_empty());
    }

    #[test]
    fn a_jackett_item_becomes_a_release() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:torznab="http://torznab.com/schemas/2015/feed"><channel>
  <item>
    <title>Film.2024.2160p.WEB-DL.DV.HDR</title>
    <guid>https://tracker.example/details/1</guid>
    <jackettindexer id="t">Tracker</jackettindexer>
    <comments>https://tracker.example/details/1</comments>
    <pubDate>Tue, 15 Sep 2026 12:50:59 +0000</pubDate>
    <size>1000</size>
    <link>http://127.0.0.1:9117/dl/t/?jackett_apikey=K&amp;path=x</link>
    <enclosure url="http://127.0.0.1:9117/dl/t/?jackett_apikey=K&amp;path=x" length="1000" type="application/x-bittorrent" />
    <torznab:attr name="seeders" value="12" />
    <torznab:attr name="peers" value="15" />
    <torznab:attr name="infohash" value="0123456789ABCDEF0123456789ABCDEF01234567" />
  </item>
  <item><title>Nothing to open</title><guid>https://tracker.example/details/2</guid></item>
</channel></rss>"#;
        let feed = crate::feed::parse_feed(xml).unwrap();
        let rows: Vec<Candidate> = feed.items.into_iter().filter_map(torznab_candidate).collect();
        assert_eq!(rows.len(), 1, "an item with nothing to open is not a release");
        let r = &rows[0].release;
        assert_eq!((r.quality, r.video_type.as_str(), r.seeders, r.size), (2160, "dv", 12, 1000));
        assert_eq!(r.tracker, "Tracker");
        assert_eq!(r.url, "https://tracker.example/details/1");
        assert_eq!(r.magnet, "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567");
        assert_eq!(r.torrent, "http://127.0.0.1:9117/dl/t/?jackett_apikey=K&path=x");
        assert_eq!(r.created, "2026-09-15T12:50:59Z");
        assert_eq!(rows[0].years, vec![2024]);
    }

    #[test]
    fn iso_time_inverts_the_feed_epoch() {
        assert_eq!(iso_time(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_time(1_789_476_659), "2026-09-15T12:50:59Z");
        assert_eq!(iso_time(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn tmdb_lang_is_a_full_tag() {
        assert_eq!(tmdb_lang("ru"), "ru-RU");
        assert_eq!(tmdb_lang("en"), "en-US");
    }
}
