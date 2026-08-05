//! Subtitle search and download through the OpenSubtitles REST API.
//!
//! Everything here follows what the live API actually answered on 2026-08-03,
//! not what the documentation implies — several details differ and each one
//! changes the code:
//!
//! * **The rate limit is 5 requests per second**, not the 1/s our research
//!   assumed (`ratelimit-limit: 5`, with `ratelimit-remaining` counting down
//!   and `ratelimit-reset: 1`). That is why a search may safely make a second
//!   request when the first one comes back empty.
//! * **Searching is not quota'd at all; downloading is.** Anonymous callers get
//!   5 downloads per 24 h per IP, an account 10–1000 — and a consumer marked
//!   *Under Development* gets 100/day with **no user authentication whatsoever**
//!   (measured: an anonymous `POST /download` returned 200 with
//!   `remaining: 99`). So the download button works before anybody signs in,
//!   and signing in is what raises the ceiling rather than what unlocks the
//!   feature.
//! * A hash search marks its results with `moviehash_match`, which is the only
//!   trustworthy "this is for your exact file" signal — a text query is fuzzy
//!   enough that searching "interstellar" puts a documentary *about* the film
//!   first.
//! * The `User-Agent` must name the application and version; the `Api-Key`
//!   header goes on every request, search included.
//!
//! The API key identifies the *application*, never the user: OpenSubtitles bans
//! apps that make their users register their own keys. It is therefore shipped
//! with the binary, cannot be a secret, and is deliberately replaceable without
//! a release — see `api_key`.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use tauri::Manager;

const API: &str = "https://api.opensubtitles.com/api/v1";
const TIMEOUT: Duration = Duration::from_secs(15);

/// Half of what the hash reads: 64 KiB from the head and 64 KiB from the tail.
const HASH_CHUNK: u64 = 64 * 1024;

fn user_agent(app: &tauri::AppHandle) -> String {
    format!("FramePlayer v{}", app.package_info().version)
}

/// The application's API key, looked up in the order that lets it be replaced
/// without shipping a new build.
///
/// 1. `<app_data>/opensubtitles.key` — where a key delivered through the same
///    signed channel as `latest.json` lands. A leaked key costs *availability*
///    (abuse gets it revoked and the feature dies for everyone at once), so
///    being able to swap it in an afternoon is the whole mitigation.
/// 2. The developer's own key next to the crate, debug builds only — it is not
///    in the repository and must never reach one.
/// 3. Compiled in from the CI secret. This is what a release actually carries.
fn api_key(app: &tauri::AppHandle) -> Option<String> {
    let cleaned = |s: String| {
        let s = s.trim().to_string();
        (!s.is_empty()).then_some(s)
    };
    if let Ok(dir) = app.path().app_data_dir() {
        if let Ok(k) = std::fs::read_to_string(dir.join("opensubtitles.key")) {
            if let Some(k) = cleaned(k) {
                return Some(k);
            }
        }
    }
    if cfg!(debug_assertions) {
        let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join(".opensubtitles-key");
        if let Ok(k) = std::fs::read_to_string(dev) {
            if let Some(k) = cleaned(k) {
                return Some(k);
            }
        }
    }
    option_env!("FP_OPENSUBTITLES_KEY").and_then(|k| cleaned(k.to_string()))
}

/// The OpenSubtitles hash: the file size plus every 64-bit little-endian word
/// of the first and last 64 KiB, added with wrapping arithmetic.
///
/// It identifies a *release* rather than a title, which is what makes it worth
/// having — subtitles matched this way are already in sync, where a title match
/// is a guess about which rip you have. It is also cheap over a network source
/// (two ranged reads), which is why it survives the torrent work intact.
pub fn movie_hash(path: &Path) -> std::io::Result<u64> {
    let mut file = std::fs::File::open(path)?;
    let size = file.metadata()?.len();
    if size < HASH_CHUNK * 2 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "file shorter than 128 KiB",
        ));
    }
    let mut hash = size;
    let mut buf = vec![0u8; HASH_CHUNK as usize];

    file.read_exact(&mut buf)?;
    hash = fold(hash, &buf);
    file.seek(SeekFrom::Start(size - HASH_CHUNK))?;
    file.read_exact(&mut buf)?;
    hash = fold(hash, &buf);

    Ok(hash)
}

fn fold(mut hash: u64, buf: &[u8]) -> u64 {
    for word in buf.chunks_exact(8) {
        hash = hash.wrapping_add(u64::from_le_bytes(word.try_into().unwrap()));
    }
    hash
}

#[derive(serde::Serialize)]
pub struct SubtitleHit {
    file_id: i64,
    file_name: String,
    language: String,
    release: String,
    /// The film or episode this belongs to, as OpenSubtitles knows it.
    movie: String,
    year: Option<i64>,
    /// For an episode: the series it belongs to, and which episode it is. The
    /// `title` of an episode result is usually just "Episode 3", so without the
    /// parent title a list of them says nothing at all.
    parent: String,
    season: Option<i64>,
    episode: Option<i64>,
    downloads: i64,
    /// Matched by the file's own hash — the same release, already in sync.
    hash_match: bool,
    hearing_impaired: bool,
    from_trusted: bool,
    /// Machine output. Shown because it is usually worth avoiding, not hidden,
    /// because for a rare language it may be all there is.
    ai_translated: bool,
    fps: Option<f64>,
    uploader: String,
}

#[derive(serde::Serialize)]
pub struct SearchResult {
    items: Vec<SubtitleHit>,
    /// `"hash"` — these are matches for this exact file. `"title"` — they are
    /// not, whatever the request contained.
    ///
    /// Derived from the results rather than from what was sent, and that
    /// distinction is the whole fix for the first version's worst behaviour:
    /// **the API answers a hash-and-query request with the union of both**
    /// (measured: hash alone 11 results, all matching; the same hash with an
    /// unrelated query 50 results, still 11 matching), so "we sent a hash"
    /// says nothing about what came back. It claimed a file match over a list
    /// of unrelated films.
    match_kind: &'static str,
    /// The file's hash was looked up and matched nothing. This is the
    /// interesting negative: it means OpenSubtitles has nothing for this exact
    /// release, which is worth saying rather than quietly showing title guesses.
    hash_tried: bool,
    /// Suppressed because the file sits in a private folder: the hash names
    /// what is being watched as surely as a stored position does.
    hash_blocked: bool,
    total: i64,
}

fn parse_hits(body: &serde_json::Value) -> Vec<SubtitleHit> {
    let Some(data) = body.get("data").and_then(|d| d.as_array()) else {
        return Vec::new();
    };
    data.iter()
        .filter_map(|item| {
            let a = item.get("attributes")?;
            // Multi-CD releases exist and the second file is the second half of
            // the film, not an alternative — take the first and let the release
            // name say what it is.
            let file = a.get("files")?.as_array()?.first()?;
            let feature = a.get("feature_details");
            let name = |v: Option<&serde_json::Value>, k: &str| {
                v.and_then(|v| v.get(k))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string()
            };
            Some(SubtitleHit {
                file_id: file.get("file_id")?.as_i64()?,
                file_name: name(Some(file), "file_name"),
                language: name(Some(a), "language"),
                release: name(Some(a), "release"),
                movie: {
                    let title = name(feature, "movie_name");
                    if title.is_empty() {
                        name(feature, "title")
                    } else {
                        title
                    }
                },
                year: feature.and_then(|f| f.get("year")).and_then(|v| v.as_i64()),
                parent: name(feature, "parent_title"),
                season: feature
                    .and_then(|f| f.get("season_number"))
                    .and_then(|v| v.as_i64()),
                episode: feature
                    .and_then(|f| f.get("episode_number"))
                    .and_then(|v| v.as_i64()),
                downloads: a.get("download_count").and_then(|v| v.as_i64()).unwrap_or(0),
                hash_match: a
                    .get("moviehash_match")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                hearing_impaired: a
                    .get("hearing_impaired")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                from_trusted: a
                    .get("from_trusted")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                ai_translated: a
                    .get("ai_translated")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                    || a.get("machine_translated")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                fps: a.get("fps").and_then(|v| v.as_f64()).filter(|f| *f > 0.0),
                uploader: name(a.get("uploader"), "name"),
            })
        })
        .collect()
}

async fn get_json(
    app: &tauri::AppHandle,
    key: &str,
    params: &[(&str, String)],
) -> Result<serde_json::Value, ApiFailure> {
    let url = reqwest::Url::parse_with_params(&format!("{API}/subtitles"), params)
        .map_err(|e| local_failure(e.to_string()))?;
    let response = reqwest::Client::new()
        .get(url)
        .header("Api-Key", key)
        .header("User-Agent", user_agent(app))
        .header("Accept", "application/json")
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|e| local_failure(e.to_string()))?;
    let status = response.status();
    let body = response.text().await.map_err(|e| local_failure(e.to_string()))?;
    if !status.is_success() {
        return Err(api_error(status.as_u16(), &body));
    }
    serde_json::from_str(&body).map_err(|e| local_failure(e.to_string()))
}

/// A failed request, kept structured until the last moment.
///
/// The daily download allowance is spent is **HTTP 406** — measured, by
/// exhausting a production key: five downloads answer 200 with `remaining`
/// counting down, and the sixth returns 406 with `remaining: -1`, a
/// human-readable `message` and `reset_time`. The rate limit is a different
/// thing entirely (5 requests a second, its own `RateLimit-*` headers), so the
/// two must not be collapsed into one "request failed".
struct ApiFailure {
    status: u16,
    /// The API's own words when it has any, and it usually does — including
    /// when the quota renews, which is the one thing worth telling the viewer.
    message: String,
    reset: Option<String>,
}

impl ApiFailure {
    fn is_quota(&self) -> bool {
        self.status == 406
    }
}

/// For the paths that still speak in strings.
impl From<ApiFailure> for String {
    fn from(failure: ApiFailure) -> Self {
        if failure.message.is_empty() {
            failure.status.to_string()
        } else {
            format!("{}: {}", failure.status, failure.message)
        }
    }
}

/// What the panel receives when a download fails. `kind` is what it branches
/// on: a spent quota is not an error to apologise for, it is a state with a way
/// out (wait, or sign in for a larger allowance).
#[derive(serde::Serialize)]
pub struct SubsError {
    kind: &'static str,
    message: String,
    reset: Option<String>,
}

impl From<ApiFailure> for SubsError {
    fn from(failure: ApiFailure) -> Self {
        SubsError {
            kind: if failure.is_quota() { "quota" } else { "other" },
            reset: failure.reset.clone(),
            message: String::from(failure),
        }
    }
}

impl From<String> for SubsError {
    fn from(message: String) -> Self {
        SubsError {
            kind: "other",
            message,
            reset: None,
        }
    }
}

/// A failure of ours rather than the server's — no status, no reset time.
fn local_failure(message: String) -> ApiFailure {
    ApiFailure { status: 0, message, reset: None }
}

/// Turn a failed response into something the UI can show. The API puts a
/// human-readable reason in `message` or `errors` often enough to be worth
/// preferring over a bare status code.
fn api_error(status: u16, body: &str) -> ApiFailure {
    let parsed = serde_json::from_str::<serde_json::Value>(body).ok();
    let text = |v: &serde_json::Value, key: &str| {
        v.get(key).and_then(|m| m.as_str()).map(|s| s.trim().to_string())
    };
    let message = parsed
        .as_ref()
        .and_then(|v| {
            text(v, "message").or_else(|| {
                let errors = v.get("errors")?.as_array()?;
                let joined: Vec<_> = errors.iter().filter_map(|e| e.as_str()).collect();
                (!joined.is_empty()).then(|| joined.join("; "))
            })
        })
        .unwrap_or_default();
    ApiFailure {
        status,
        message,
        reset: parsed.as_ref().and_then(|v| text(v, "reset_time")),
    }
}

/// Search by the file's hash, by a typed query, or both.
///
/// The hash is only computed for a local file that is **not** in a private
/// folder: sending it names what is being watched to a third party just as
/// surely as storing a position does, which makes this the fourth place the
/// privacy roots have to be enforced (after the position store, the resume
/// snapshot and the thumbnail cache). A typed query is the user's own words and
/// stays available there — what is refused is the automatic part.
#[tauri::command]
pub async fn subs_search(
    app: tauri::AppHandle,
    path: Option<String>,
    query: Option<String>,
    languages: String,
    season: Option<i64>,
    episode: Option<i64>,
) -> Result<SearchResult, String> {
    let key = api_key(&app).ok_or_else(|| "no API key".to_string())?;

    let local = path.filter(|p| !p.contains("://"));
    let hash_blocked = local
        .as_deref()
        .is_some_and(crate::thumb_service::is_private);
    let hash = match &local {
        Some(p) if !hash_blocked => {
            let p = PathBuf::from(p);
            tauri::async_runtime::spawn_blocking(move || movie_hash(&p))
                .await
                .ok()
                .and_then(|r| r.ok())
        }
        _ => None,
    };

    let query = query.map(|q| q.trim().to_string()).filter(|q| !q.is_empty());
    if hash.is_none() && query.is_none() {
        return Err("nothing to search by".into());
    }

    // An empty language filter means "any" and has to be *absent*: sending
    // `languages=` is a malformed value rather than a wildcard.
    let language_param = |params: &mut Vec<(&'static str, String)>| {
        if !languages.is_empty() {
            params.push(("languages", languages.clone()));
        }
    };

    // The two searches are deliberately **sequential and separate**, never one
    // request carrying both. Sent together the API returns the union, so the
    // answer cannot be attributed: a hash matching 11 subtitles plus an
    // unrelated query came back as 50 results with no way to tell, from the
    // request, that 39 of them were about a different film entirely.
    let mut hash_tried = false;
    if let Some(h) = hash {
        hash_tried = true;
        let mut params = Vec::new();
        language_param(&mut params);
        params.push(("moviehash", format!("{h:016x}")));
        // Belt and braces on top of asking by hash alone: `only` tells the API
        // to drop anything that is not a match for this file.
        params.push(("moviehash_match", "only".into()));

        let body = get_json(&app, &key, &params).await?;
        let mut items = parse_hits(&body);
        if !items.is_empty() {
            items.sort_by(|a, b| b.downloads.cmp(&a.downloads));
            let total = body.get("total_count").and_then(|v| v.as_i64()).unwrap_or(0);
            return Ok(SearchResult {
                items,
                match_kind: "hash",
                hash_tried,
                hash_blocked,
                total,
            });
        }
    }

    // Nothing for this exact file — which is the normal case for anything but a
    // well-known release. The typed title is the fallback, and the UI is told
    // plainly that this is what happened.
    let Some(q) = query else {
        return Ok(SearchResult {
            items: Vec::new(),
            match_kind: "hash",
            hash_tried,
            hash_blocked,
            total: 0,
        });
    };
    let mut params = Vec::new();
    language_param(&mut params);
    params.push(("query", q));
    // The numbers are what reach a series at all. Measured: "the day of the
    // jackal" alone returns the 1973 FILM, and with season/episode it returns
    // 122 subtitles for the 2024 series — the same query, a different work.
    // `year` is not a substitute: tried, and it answered with 10 000 results of
    // "Empty Movie (SubScene)".
    if let (Some(season), Some(episode)) = (season, episode) {
        params.push(("season_number", season.to_string()));
        params.push(("episode_number", episode.to_string()));
    }
    let body = get_json(&app, &key, &params).await?;
    let mut items = parse_hits(&body);
    let total = body.get("total_count").and_then(|v| v.as_i64()).unwrap_or(0);

    // The API's own relevance order is not usable as-is: "interstellar" puts a
    // documentary *about* the film above subtitles for the film.
    items.sort_by(|a, b| b.downloads.cmp(&a.downloads));

    Ok(SearchResult {
        // Derived from what came back, not from what was asked. A stray
        // `moviehash_match` here would be the API's doing and is still the truth.
        match_kind: if items.iter().any(|i| i.hash_match) {
            "hash"
        } else {
            "title"
        },
        items,
        hash_tried,
        hash_blocked,
        total,
    })
}

// ---- The account ----------------------------------------------------------
//
// Signing in is an *upgrade*, never a gate: measured, downloading works with no
// account at all (5 a day per IP, 100 for a consumer in development). What an
// account buys is the ceiling — 10 to 1000 a day depending on rank — so the
// panel works signed out and says what signing in would change.
//
// What is stored, and where, follows from what it costs to lose. The **token**
// goes into the OS keychain by default: it expires by itself, can be revoked
// from the site, and without it the viewer signs in again for practically every
// film, since it lasts 24 h with no refresh endpoint. The **password** is
// stored only when explicitly asked for, because it is the most sensitive thing
// this application will ever hold and is very likely reused elsewhere; what it
// buys is silent re-login once the token expires.
//
// Neither ever touches localStorage or a file of ours.

/// One service, two entries. Keyed by the bundle identifier so a future
/// second account of some other kind cannot collide with this one.
const KEYCHAIN_SERVICE: &str = "app.frameplayer.opensubtitles";
const KEYCHAIN_SESSION: &str = "session";
const KEYCHAIN_PASSWORD: &str = "password";

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Session {
    username: String,
    token: String,
    /// VIP accounts are answered by a different host, handed to us at login.
    /// Ignoring it works until someone with a VIP account wonders why their
    /// 1000 downloads behave like 20.
    #[serde(default)]
    base_url: Option<String>,
}

static SESSION: std::sync::Mutex<Option<Session>> = std::sync::Mutex::new(None);
static SESSION_LOADED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn keychain(key: &str) -> Option<keyring::Entry> {
    keyring::Entry::new(KEYCHAIN_SERVICE, key).ok()
}

/// The session, from memory or — once per run — from the keychain.
///
/// Blocking: on macOS a keychain read can put a prompt on screen, and a dev
/// build gets one whenever the code signature changes. Callers are async, so
/// this belongs inside `spawn_blocking`.
fn session_now() -> Option<Session> {
    use std::sync::atomic::Ordering;
    if !SESSION_LOADED.swap(true, Ordering::Relaxed) {
        let stored = keychain(KEYCHAIN_SESSION)
            .and_then(|e| e.get_password().ok())
            .and_then(|raw| serde_json::from_str::<Session>(&raw).ok());
        if stored.is_some() {
            *SESSION.lock().unwrap_or_else(|e| e.into_inner()) = stored;
        }
    }
    SESSION.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Returns whether the keychain took it. A refusal is not fatal — the session
/// still works until the app quits — but it must be reported rather than
/// silently turning "stay signed in" into a lie.
fn remember_session(session: &Session) -> bool {
    *SESSION.lock().unwrap_or_else(|e| e.into_inner()) = Some(session.clone());
    SESSION_LOADED.store(true, std::sync::atomic::Ordering::Relaxed);
    let Ok(raw) = serde_json::to_string(session) else {
        return false;
    };
    keychain(KEYCHAIN_SESSION).is_some_and(|e| e.set_password(&raw).is_ok())
}

fn forget_session() {
    *SESSION.lock().unwrap_or_else(|e| e.into_inner()) = None;
    SESSION_LOADED.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Some(entry) = keychain(KEYCHAIN_SESSION) {
        let _ = entry.delete_credential();
    }
}

fn stored_password() -> Option<String> {
    keychain(KEYCHAIN_PASSWORD).and_then(|e| e.get_password().ok())
}

fn forget_password() {
    if let Some(entry) = keychain(KEYCHAIN_PASSWORD) {
        let _ = entry.delete_credential();
    }
}

/// The host to talk to. VIP accounts are served by their own, and the login
/// response is the only place that is announced.
fn base_for(session: Option<&Session>) -> String {
    session
        .and_then(|s| s.base_url.clone())
        .map(|host| {
            let host = host.trim_end_matches('/').to_string();
            if host.starts_with("http") {
                format!("{host}/api/v1")
            } else {
                format!("https://{host}/api/v1")
            }
        })
        .unwrap_or_else(|| API.to_string())
}

#[derive(serde::Serialize, Default)]
pub struct AccountInfo {
    signed_in: bool,
    username: Option<String>,
    /// A password is in the keychain, so an expired token renews itself.
    remembered_password: bool,
    /// Downloads a day this account is entitled to, and how many are left.
    allowed_downloads: Option<i64>,
    remaining_downloads: Option<i64>,
    level: Option<String>,
    /// The keychain refused to keep the session: it lasts until the app quits.
    /// Said out loud rather than discovered at the next launch.
    keychain_failed: bool,
}

/// Sign in. `remember_password` is the explicit opt-in described above; the
/// token is kept either way.
///
/// Note the endpoint's own rate limit is **1 request per second** (measured —
/// the search endpoint allows 5), so nothing here may retry in a loop.
#[tauri::command]
pub async fn subs_login(
    app: tauri::AppHandle,
    username: String,
    password: String,
    remember_password: bool,
) -> Result<AccountInfo, String> {
    let key = api_key(&app).ok_or_else(|| "no API key".to_string())?;
    let body = post_json(
        &app,
        &key,
        API,
        "/login",
        None,
        &serde_json::json!({ "username": username, "password": password }),
    )
    .await?;

    let token = body
        .get("token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "no token in the response".to_string())?
        .to_string();
    let session = Session {
        username: username.clone(),
        token,
        base_url: body
            .get("base_url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    };
    let user = body.get("user");

    let kept = tauri::async_runtime::spawn_blocking({
        let session = session.clone();
        let password = password.clone();
        move || {
            let kept = remember_session(&session);
            if remember_password {
                if let Some(entry) = keychain(KEYCHAIN_PASSWORD) {
                    let _ = entry.set_password(&password);
                }
            } else {
                forget_password();
            }
            kept
        }
    })
    .await
    .unwrap_or(false);

    Ok(AccountInfo {
        signed_in: true,
        username: Some(username),
        remembered_password: remember_password,
        allowed_downloads: user
            .and_then(|u| u.get("allowed_downloads"))
            .and_then(|v| v.as_i64()),
        remaining_downloads: user
            .and_then(|u| u.get("remaining_downloads"))
            .and_then(|v| v.as_i64()),
        level: user
            .and_then(|u| u.get("level"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        keychain_failed: !kept,
    })
}

/// Sign out: tell the server to revoke the token, then drop both secrets.
///
/// The local half runs whatever the server says — a token we can no longer use
/// is exactly what "signed out" means, and leaving it in the keychain because
/// the network was down would be the wrong way round.
#[tauri::command]
pub async fn subs_logout(app: tauri::AppHandle) -> Result<AccountInfo, String> {
    let session = tauri::async_runtime::spawn_blocking(session_now)
        .await
        .unwrap_or(None);
    if let (Some(key), Some(session)) = (api_key(&app), session.as_ref()) {
        let base = base_for(Some(session));
        let _ = post_json(
            &app,
            &key,
            &base,
            "/logout",
            Some(&session.token),
            &serde_json::json!({}),
        )
        .await;
    }
    tauri::async_runtime::spawn_blocking(|| {
        forget_session();
        forget_password();
    })
    .await
    .ok();
    Ok(AccountInfo::default())
}

/// The current state, for the panel. With `refresh` it also asks the server for
/// the live quota — worth doing when the panel opens, and never worth a
/// download to find out.
#[tauri::command]
pub async fn subs_account(app: tauri::AppHandle, refresh: bool) -> AccountInfo {
    let (session, has_password) = tauri::async_runtime::spawn_blocking(|| {
        let session = session_now();
        let has_password = session.is_some() && stored_password().is_some();
        (session, has_password)
    })
    .await
    .unwrap_or((None, false));

    let Some(session) = session else {
        return AccountInfo::default();
    };
    let mut info = AccountInfo {
        signed_in: true,
        username: Some(session.username.clone()),
        remembered_password: has_password,
        ..Default::default()
    };
    if !refresh {
        return info;
    }
    let Some(key) = api_key(&app) else {
        return info;
    };

    let url = format!("{}/infos/user", base_for(Some(&session)));
    let response = reqwest::Client::new()
        .get(url)
        .header("Api-Key", &key)
        .header("User-Agent", user_agent(&app))
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {}", session.token))
        .timeout(TIMEOUT)
        .send()
        .await;
    let Ok(response) = response else { return info };

    // An expired token is the expected end of every session: 24 h, no refresh
    // endpoint. With a remembered password that is invisible; without one the
    // panel simply shows the signed-out state again.
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        if let Some(renewed) = renew(&app, &key, &session.username).await {
            return renewed;
        }
        tauri::async_runtime::spawn_blocking(forget_session).await.ok();
        return AccountInfo::default();
    }
    let Ok(body) = response.json::<serde_json::Value>().await else {
        return info;
    };
    let data = body.get("data");
    info.allowed_downloads = data
        .and_then(|d| d.get("allowed_downloads"))
        .and_then(|v| v.as_i64());
    info.remaining_downloads = data
        .and_then(|d| d.get("remaining_downloads"))
        .and_then(|v| v.as_i64());
    info.level = data
        .and_then(|d| d.get("level"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    info
}

/// Sign in again with the remembered password, if there is one. Returns `None`
/// when there is nothing to try, which is the caller's cue to show the
/// signed-out state.
async fn renew(app: &tauri::AppHandle, key: &str, username: &str) -> Option<AccountInfo> {
    let password = tauri::async_runtime::spawn_blocking(stored_password)
        .await
        .ok()
        .flatten()?;
    let body = post_json(
        app,
        key,
        API,
        "/login",
        None,
        &serde_json::json!({ "username": username, "password": password }),
    )
    .await
    .ok()?;
    let token = body.get("token").and_then(|v| v.as_str())?.to_string();
    let session = Session {
        username: username.to_string(),
        token,
        base_url: body
            .get("base_url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    };
    let user = body.get("user").cloned();
    let kept = tauri::async_runtime::spawn_blocking({
        let session = session.clone();
        move || remember_session(&session)
    })
    .await
    .unwrap_or(false);
    Some(AccountInfo {
        signed_in: true,
        username: Some(username.to_string()),
        remembered_password: true,
        allowed_downloads: user
            .as_ref()
            .and_then(|u| u.get("allowed_downloads"))
            .and_then(|v| v.as_i64()),
        remaining_downloads: user
            .as_ref()
            .and_then(|u| u.get("remaining_downloads"))
            .and_then(|v| v.as_i64()),
        level: user
            .as_ref()
            .and_then(|u| u.get("level"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        keychain_failed: !kept,
    })
}

async fn post_json(
    app: &tauri::AppHandle,
    key: &str,
    base: &str,
    path: &str,
    token: Option<&str>,
    body: &serde_json::Value,
) -> Result<serde_json::Value, ApiFailure> {
    let mut request = reqwest::Client::new()
        .post(format!("{base}{path}"))
        .header("Api-Key", key)
        .header("User-Agent", user_agent(app))
        .header("Accept", "application/json")
        .json(body)
        .timeout(TIMEOUT);
    if let Some(token) = token {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    let response = request.send().await.map_err(|e| local_failure(e.to_string()))?;
    let status = response.status();
    let text = response.text().await.map_err(|e| local_failure(e.to_string()))?;
    if !status.is_success() {
        return Err(api_error(status.as_u16(), &text));
    }
    serde_json::from_str(&text).map_err(|e| local_failure(e.to_string()))
}

#[derive(serde::Serialize)]
pub struct DownloadResult {
    /// Where it landed, for mpv's `sub-add`.
    path: String,
    /// Downloads left today. Anonymous callers get 5 per IP, an account 10–1000,
    /// and a consumer in development 100 — so this is worth showing rather than
    /// discovering when it runs out.
    remaining: Option<i64>,
    /// Human-readable time until the quota renews, straight from the API.
    reset: Option<String>,
}

/// Fetch one subtitle file and put it next to the video.
///
/// Two steps, and the first is the one that is quota'd: `POST /download` does
/// not return the file, it returns a **short-lived link** to it. The file then
/// comes over plain HTTP with no key at all.
///
/// It is saved as `<video name>.<language>.<ext>` in the video's own folder,
/// which is what makes it stick: mpv's `sub-auto=fuzzy` picks up anything whose
/// name starts with the video's, so the subtitle is there by itself the next
/// time the episode is opened, with nothing remembered on our side. A folder
/// that cannot be written to (a read-only share, a mounted image) falls back to
/// our cache directory — the subtitle still loads, it just will not be found
/// again automatically.
///
/// A private folder is deliberately **not** special-cased here: downloading is
/// an explicit act, and putting the file where the viewer asked for it is the
/// point. What privacy governs is the automatic hash lookup in `subs_search`.
#[tauri::command]
pub async fn subs_download(
    app: tauri::AppHandle,
    file_id: i64,
    video_path: Option<String>,
    language: String,
) -> Result<DownloadResult, SubsError> {
    let key = api_key(&app).ok_or_else(|| SubsError::from("no API key".to_string()))?;

    // Signed in, the download is charged to the account and its larger quota;
    // signed out it goes against the anonymous per-IP allowance. Both work,
    // which is why there is no check here for being signed in.
    let session = tauri::async_runtime::spawn_blocking(session_now)
        .await
        .unwrap_or(None);
    let ticket = {
        let request = serde_json::json!({ "file_id": file_id });
        let attempt = post_json(
            &app,
            &key,
            &base_for(session.as_ref()),
            "/download",
            session.as_ref().map(|s| s.token.as_str()),
            &request,
        )
        .await;
        match attempt {
            Ok(ticket) => ticket,
            // A token that expired mid-session: renew it once if a password is
            // remembered, otherwise fall back to the anonymous quota rather
            // than failing outright — the viewer asked for a subtitle, not for
            // a lecture about tokens.
            Err(e) if e.status == 401 && session.is_some() => {
                let username = session.as_ref().map(|s| s.username.clone()).unwrap_or_default();
                let renewed = renew(&app, &key, &username).await;
                let token = if renewed.is_some() {
                    tauri::async_runtime::spawn_blocking(session_now)
                        .await
                        .unwrap_or(None)
                } else {
                    tauri::async_runtime::spawn_blocking(forget_session).await.ok();
                    None
                };
                post_json(
                    &app,
                    &key,
                    &base_for(token.as_ref()),
                    "/download",
                    token.as_ref().map(|s| s.token.as_str()),
                    &request,
                )
                .await?
            }
            Err(e) => return Err(e.into()),
        }
    };

    let link = ticket
        .get("link")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "no download link in the response".to_string())?;
    let remote_name = ticket
        .get("file_name")
        .and_then(|v| v.as_str())
        .unwrap_or("subtitle.srt");

    let bytes = reqwest::Client::new()
        .get(link)
        .header("User-Agent", user_agent(&app))
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;

    let extension = Path::new(remote_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("srt")
        .to_lowercase();
    let target = destination(&app, video_path.as_deref(), &language, &extension, remote_name)?;
    std::fs::write(&target, &bytes).map_err(|e| e.to_string())?;

    Ok(DownloadResult {
        path: target.to_string_lossy().to_string(),
        remaining: ticket.get("remaining").and_then(|v| v.as_i64()),
        reset: ticket
            .get("reset_time")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    })
}

/// Extensions this command will delete. Not a formality: the path comes from
/// the frontend, and a delete-by-path command that takes any path is a way to
/// remove the video itself by getting one argument wrong.
const SUBTITLE_EXTENSIONS: [&str; 8] = ["srt", "ass", "ssa", "sub", "vtt", "idx", "sup", "smi"];

/// Delete a subtitle file this player downloaded.
///
/// Only ever called for a path the frontend recorded as ours (see
/// `frameplayer.subs`), because a subtitle the viewer made or corrected is not
/// ours to remove. The extension check is the second lock on the same door.
#[tauri::command]
pub fn subs_delete_file(path: String) -> Result<(), String> {
    let file = Path::new(&path);
    let ok = file
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .is_some_and(|e| SUBTITLE_EXTENSIONS.contains(&e.as_str()));
    if !ok {
        return Err("not a subtitle file".into());
    }
    std::fs::remove_file(file).map_err(|e| e.to_string())
}

/// Where the subtitle goes: beside the video if that folder takes it, our cache
/// directory otherwise. Never over an existing file — a subtitle already sitting
/// there may be one the viewer made or corrected.
fn destination(
    app: &tauri::AppHandle,
    video: Option<&str>,
    language: &str,
    extension: &str,
    remote_name: &str,
) -> Result<PathBuf, String> {
    let local = video.filter(|p| !p.contains("://")).map(Path::new);
    if let Some(video) = local {
        if let (Some(dir), Some(stem)) = (video.parent(), video.file_stem()) {
            if writable(dir) {
                let base = format!("{}.{}", stem.to_string_lossy(), language);
                return Ok(free_name(dir, &base, extension));
            }
        }
    }
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("subtitles");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let base = Path::new(remote_name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "subtitle".into());
    Ok(free_name(&dir, &base, extension))
}

/// Probing by writing is the only honest test: a directory can be listable and
/// still refuse a file (a read-only mount, a share without write permission),
/// and the metadata does not always say so.
fn writable(dir: &Path) -> bool {
    let probe = dir.join(".frameplayer-write-test");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn free_name(dir: &Path, base: &str, extension: &str) -> PathBuf {
    let first = dir.join(format!("{base}.{extension}"));
    if !first.exists() {
        return first;
    }
    for n in 2..100 {
        let candidate = dir.join(format!("{base} ({n}).{extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    first
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// The hash must agree with an independent implementation of the published
    /// algorithm, computed here over the same bytes — the algorithm is easy to
    /// get subtly wrong (endianness, the tail offset, wrapping) and a wrong
    /// hash simply returns no results, which reads as "this file has no
    /// subtitles" rather than as a bug.
    #[test]
    fn hash_matches_reference() {
        let dir = std::env::temp_dir().join("fp-oshash-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sample.bin");

        // 200 KiB of non-repeating bytes, so a mistake in either half shows.
        let bytes: Vec<u8> = (0..200 * 1024).map(|i| (i * 31 % 251) as u8).collect();
        std::fs::File::create(&path)
            .unwrap()
            .write_all(&bytes)
            .unwrap();

        let reference = {
            let mut h = bytes.len() as u64;
            for chunk in [
                &bytes[..HASH_CHUNK as usize],
                &bytes[bytes.len() - HASH_CHUNK as usize..],
            ] {
                for word in chunk.chunks_exact(8) {
                    h = h.wrapping_add(u64::from_le_bytes(word.try_into().unwrap()));
                }
            }
            h
        };

        assert_eq!(movie_hash(&path).unwrap(), reference);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn short_files_have_no_hash() {
        let dir = std::env::temp_dir().join("fp-oshash-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("short.bin");
        std::fs::File::create(&path).unwrap().write_all(&[0u8; 16]).unwrap();
        assert!(movie_hash(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }
}
