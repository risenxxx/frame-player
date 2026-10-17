//! The installation count on the landing page: one request, once in the life of
//! an installation, carrying the platform and nothing else.
//!
//! No identifier is sent or kept — the count is of first launches, so there is
//! nothing to tell one installation from another and nothing to ask for again.
//! The site's worker adds it to a number it keeps itself (`site/installs/`).
//!
//! The state is one file in the config directory, `install-count`:
//! absent — never decided; `pending` — a first launch whose request has not
//! been answered yet, retried on every start until it is; `done` — nothing more
//! to do, ever.
//!
//! **Installations older than this code are already in the site's starting
//! number**, so they must not report when they update into it. What tells them
//! apart is `mpv.conf`: every version since the first writes it into the same
//! directory on its first start (`user_mpv_conf`), and this runs before the
//! window exists, so before the frontend can write it for this launch.

use std::time::Duration;

use crate::portable;

const ENDPOINT: &str = "https://frameplayer.app/api/install";
const STATE_FILE: &str = "install-count";

/// Decide synchronously, then report in the background. Call before the main
/// window is created.
pub fn start(app: &tauri::AppHandle) {
    // A developer's runs are not installations.
    if cfg!(debug_assertions) {
        return;
    }
    let Ok(dir) = portable::config_dir(app) else { return };
    let state = dir.join(STATE_FILE);
    match std::fs::read_to_string(&state).ok().as_deref().map(str::trim) {
        Some("done") => return,
        Some(_) => {}
        None if dir.join("mpv.conf").exists() => {
            let _ = std::fs::write(&state, "done");
            return;
        }
        None => {
            if std::fs::write(&state, "pending").is_err() {
                // Without a place to remember the answer, every start would
                // count again. Better not to count at all.
                return;
            }
        }
    }
    let version = app.package_info().version.to_string();
    tauri::async_runtime::spawn(async move {
        if report(&version).await {
            let _ = std::fs::write(&state, "done");
        }
    });
}

/// True once the site has answered at all: a refusal (the address already
/// counted today, a request it did not like) is an answer too, and asking again
/// on every start would not change it. Only a request that never arrived is
/// retried.
async fn report(version: &str) -> bool {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(format!("FramePlayer/{version}"))
        .build()
    else {
        return false;
    };
    let body = serde_json::json!({ "platform": platform() });
    match client.post(ENDPOINT).json(&body).send().await {
        Ok(_) => true,
        Err(e) => {
            eprintln!("[install-count] not reported, will retry next start: {e}");
            false
        }
    }
}

fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "other"
    }
}
