//! The AVTransport sequences that take more than one action: a seek that
//! learns which unit the renderer speaks, and a load — `Stop`, the URI with its
//! metadata, then `Play` until the television has started.

use std::sync::{Arc, Mutex};

use super::control::{http, soap};
use super::metadata::{didl, extension_of, negotiate_mime};
use super::xml::{escape_text, tag};
use super::{set_state, DlnaService, DlnaState, AVTRANSPORT, PLAY_BUDGET, PLAY_RETRY};

/// `H:MM:SS` (what AVTransport speaks) from seconds, and back.
fn hms(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    format!("{}:{:02}:{:02}", total / 3600, (total % 3600) / 60, total % 60)
}

pub(super) fn parse_hms(text: &str) -> Option<f64> {
    let mut secs = 0.0;
    for part in text.trim().split(':') {
        secs = secs * 60.0 + part.parse::<f64>().ok()?;
    }
    Some(secs)
}

/// The `Seek` units worth trying, in order of preference.
///
/// **`REL_TIME` is not universal.** It is what the measured television takes
/// and what most renderers declare, but `ABS_TIME` is the only mode some
/// devices implement — and a renderer that declares both may still refuse one
/// of them, which is why this is a runtime ladder rather than a reading of the
/// SCPD's `A_ARG_TYPE_SeekMode` list. The byte modes are deliberately absent:
/// `X_DLNA_REL_BYTE` would need an offset we cannot compute from a timestamp
/// without the container index the television is holding.
const SEEK_UNITS: [&str; 2] = ["REL_TIME", "ABS_TIME"];

/// Seek, learning which unit this device speaks and then keeping to it.
///
/// The cost of the ladder is one extra round trip on the first seek of a
/// session, and only on a device that refuses the preferred unit; the cost of
/// not having it is a seekbar that silently does nothing on every renderer
/// outside the one this was measured against.
pub(super) async fn seek_to(
    client: &reqwest::Client,
    control: &str,
    state: &Arc<Mutex<DlnaState>>,
    target: f64,
) -> Result<(), String> {
    let known = state.lock().unwrap_or_else(|p| p.into_inner()).seek_unit;
    let units: Vec<&'static str> = match known {
        Some(u) => vec![u],
        None => SEEK_UNITS.to_vec(),
    };
    let mut last = None;
    for unit in units {
        let args = format!("<Unit>{unit}</Unit><Target>{}</Target>", hms(target));
        match soap(client, control, AVTRANSPORT, "Seek", &args).await {
            Ok(_) => {
                set_state(state, |s| s.seek_unit = Some(unit));
                return Ok(());
            }
            Err(e) => {
                eprintln!("[dlna] {e}");
                last = Some(e);
            }
        }
    }
    Err(last.unwrap_or_else(|| "Seek: no unit accepted".into()))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn load_url(
    service: &Arc<DlnaService>,
    url: String,
    mime: String,
    size: u64,
    duration: Option<f64>,
    position: f64,
    title: Option<String>,
    caption: Option<(String, &'static str)>,
) -> Result<(), String> {
    let (control, state, advertised) = {
        let inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
        let session = inner.session.as_ref().ok_or("not connected")?;
        (
            session.device.control_url.clone(),
            session.state.clone(),
            session.device.mimes.clone(),
        )
    };
    // The torrent path arrives with a MIME the frontend picked from its own
    // table; the renderer's list still gets the last word on how to spell it.
    let mime = negotiate_mime(&advertised, &extension_of(&url), &mime);
    set_state(&state, |s| {
        s.state = "loading".into();
        s.error = None;
        s.time = position;
        s.reported_at = None;
        if let Some(d) = duration {
            s.duration = d;
        }
    });

    let client = http();
    // **Stop before Set, always.** A renderer that is already playing refuses
    // a new URI outright — measured: UPnP **701 "Transition not available"** —
    // and it will be playing more often than not: the previous session, a
    // session this player did not end cleanly, or another sender on the same
    // television. The Stop is best-effort by design; on an idle renderer it is
    // a no-op, and its failure says nothing about whether the load will work.
    let _ = soap(client, &control, AVTRANSPORT, "Stop", "").await;
    let name = title.unwrap_or_else(|| "Frame Player".into());
    // The URL is percent-encoded by the server that issued it, so nothing in it
    // needs escaping today; escaping anyway is what keeps that a property of
    // this call rather than of whoever registered the file.
    let args = format!(
        "<CurrentURI>{}</CurrentURI><CurrentURIMetaData>{}</CurrentURIMetaData>",
        escape_text(&url),
        didl(
            &url,
            &mime,
            &name,
            size,
            duration,
            caption.as_ref().map(|(u, m)| (u.as_str(), *m))
        )
    );
    if let Err(e) = soap(client, &control, AVTRANSPORT, "SetAVTransportURI", &args).await {
        eprintln!("[dlna] {e}");
        set_state(&state, |s| {
            s.state = "error".into();
            s.error = Some("load_failed".into());
        });
        return Err(e);
    }
    // **Accepting a URI is not being ready for a command, and a refusal is not
    // a verdict.** Setting the URI is what makes the TV bring its player up,
    // and until that is done the renderer answers `Play` with a dropped
    // connection or with UPnP 701 "transition not available".
    //
    // The trap is that the refusal arrives while the television is *already
    // fetching the file*: measured on the LG, `SetAVTransportURI` alone starts
    // it, so counting refusals means giving up on a load that is under way.
    // Measured on a 10 GB 2160p MKV: the set answered `TRANSITIONING` from the
    // first poll to the last, served a HEAD, a read from byte 0 and then the
    // Matroska cues from the 10 GB mark ~18 s in — while the old six-attempt
    // budget expired around seven seconds and reported "the TV could not open
    // this file". Casting the same file again succeeded at once, against a
    // player that was by then already up.
    //
    // So the renderer is asked what it is doing rather than counted at. It is
    // started if it answers `Play`, if it reports `PLAYING` whoever caused it,
    // or if it reports `TRANSITIONING` — that is a television preparing, not
    // one refusing. Only "still not started when the budget runs out" is a
    // failed load.
    let mut playing = false;
    let mut starting = false;
    let mut refusal: Option<String> = None;
    let deadline = std::time::Instant::now() + PLAY_BUDGET;
    loop {
        match soap(client, &control, AVTRANSPORT, "Play", "<Speed>1</Speed>").await {
            Ok(_) => {
                playing = true;
                break;
            }
            // Printed unconditionally, like a failed Cast LOAD: the renderer
            // names its own reason, and collapsing every one of them into
            // `load_failed` is what made this failure take a wire log to tell
            // apart from a file the television cannot decode.
            Err(e) => {
                eprintln!("[dlna] {e}");
                refusal = Some(e);
            }
        }
        if let Ok(info) = soap(client, &control, AVTRANSPORT, "GetTransportInfo", "").await {
            match tag(&info, "CurrentTransportState").unwrap_or("") {
                "PLAYING" => {
                    playing = true;
                    break;
                }
                "TRANSITIONING" => starting = true,
                _ => {}
            }
            // The device saying outright that it could not take the stream —
            // no reason to spend the rest of the budget asking again.
            if tag(&info, "CurrentTransportStatus")
                .is_some_and(|s| s.eq_ignore_ascii_case("ERROR_OCCURRED"))
            {
                refusal = Some("renderer reported ERROR_OCCURRED".into());
                break;
            }
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(PLAY_RETRY).await;
    }
    if !playing && !starting {
        set_state(&state, |s| {
            s.state = "error".into();
            s.error = Some("load_failed".into());
        });
        return Err(refusal.unwrap_or_else(|| "renderer never accepted Play".into()));
    }
    if position > 1.0 {
        // Best-effort, and worth saying why it may do nothing: a renderer that
        // is only `TRANSITIONING` has not read the container's index yet, so it
        // has no way to turn a time into an offset and answers accordingly.
        // Losing the resume point is a far smaller failure than refusing the
        // load over it, which is why this stays a nudge — but a silent one is
        // indistinguishable from a television that ignores seeks altogether.
        //
        // Deliberately *not* where the seek unit gets learned: a refusal here
        // usually means "too early", not "wrong unit", and recording ABS_TIME
        // from it would pin the whole session to the fallback. `seek_to` only
        // remembers what succeeded, so a failure at this point leaves the
        // ladder untouched for the viewer's first real seek.
        let _ = seek_to(client, &control, &state, position).await;
    }
    // **Not "playing" — "buffering".** A renderer accepting `Play` says nothing
    // about it managing to decode what it fetched, and claiming playback here
    // blinded every detector downstream: the frontend marks the session as
    // having played, and a television that takes the file and shows nothing
    // then looks exactly like one that is watching it. The poll reports the
    // truth a second later (TRANSITIONING → PLAYING), and until it does, the
    // session is honestly still starting.
    set_state(&state, |s| s.state = "buffering".into());
    Ok(())
}
