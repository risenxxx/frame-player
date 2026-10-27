//! Answering for a device without the GUI, both run inside the app because
//! macOS drops multicast for a process without Local Network permission:
//! `FP_DLNA_PROBE=1` prints what the network holds, `FP_DLNA_PLAY=<file>`
//! drives one whole session through the shipping commands.

use std::sync::Arc;
use std::time::Duration;

use super::control::{http, protocol_info, summarize_sink};
use super::description::describe;
use super::discovery::ssdp_locations;
use super::{
    cast_diagnose, dlna_connect, dlna_control, dlna_devices, dlna_disconnect, dlna_discover_start,
    dlna_discover_stop, dlna_load, dlna_status, DlnaService,
};

/// Print what the network has. Called at startup only under `FP_DLNA_PROBE=1`.
pub async fn probe() {
    eprintln!("[dlna] searching for UPnP devices…");
    let locations = ssdp_locations(Duration::from_secs(6), &[]).await;
    if locations.is_empty() {
        eprintln!("[dlna] nothing answered M-SEARCH — no UPnP devices, or multicast is being dropped");
        return;
    }

    let client = http();
    let mut renderers = 0;
    for location in &locations {
        let Some(r) = describe(client, location).await else {
            eprintln!("[dlna] {location}: description unreadable");
            continue;
        };
        eprintln!(
            "[dlna] {} — {} {} ({}) at {}",
            r.friendly_name, r.manufacturer, r.model, r.device_type, r.location
        );
        eprintln!(
            "[dlna]     AVTransport: {}",
            r.avtransport.as_deref().unwrap_or("(none — cannot be pushed to)")
        );
        if r.avtransport.is_some() {
            renderers += 1;
        }
        if let Some(cm) = &r.connection_manager {
            match protocol_info(client, cm).await {
                Some(sink) => {
                    let formats = summarize_sink(&sink);
                    eprintln!("[dlna]     accepts {} distinct MIME types:", formats.len());
                    for (mime, count) in formats {
                        eprintln!("[dlna]       {mime} ({count} profile(s))");
                    }
                }
                None => eprintln!("[dlna]     GetProtocolInfo: no answer"),
            }
        }
    }
    eprintln!("[dlna] done: {} renderer(s) that accept a pushed URL", renderers);
}

/// Drive the real command surface without the GUI: discover, connect, load,
/// watch, disconnect. Off unless `FP_DLNA_PLAY=<file>` is set (with an optional
/// `FP_DLNA_TARGET=<ip>`, and `FP_DLNA_SUB=<subtitle file>` to send one
/// along), and deliberately calling the same functions the frontend invokes rather than a parallel copy — a self-test of a private
/// re-implementation proves nothing about what ships.
pub async fn selftest(app: &tauri::AppHandle, target: Option<String>, path: String) {
    use tauri::Manager as _;

    let service = app.state::<Arc<DlnaService>>();
    let cast_service = app.state::<Arc<crate::cast::CastService>>();
    if let Err(e) = dlna_discover_start(service.clone(), None) {
        eprintln!("[dlna] discovery failed: {e}");
        return;
    }
    tokio::time::sleep(Duration::from_secs(5)).await;
    let devices = dlna_devices(service.clone());
    dlna_discover_stop(service.clone());
    let Some(device) = devices
        .into_iter()
        .find(|d| match &target {
            Some(ip) => &d.ip == ip,
            None => d.mimes.iter().any(|m| m.starts_with("video/")),
        })
    else {
        eprintln!("[dlna] no matching renderer");
        return;
    };
    eprintln!("[dlna] selftest on {} ({})", device.name, device.ip);
    let target_ip = device.ip.clone();
    // The description URL, which is the control URL minus its service path —
    // the device list keeps only the latter, and re-deriving it here keeps the
    // selftest honest about what the command actually receives.
    let location_for_report = device
        .control_url
        .split_once("/AVTransport/")
        .map(|(origin, _)| format!("{origin}/"))
        .unwrap_or_else(|| device.control_url.clone());
    if let Err(e) = dlna_connect(service.clone(), device) {
        eprintln!("[dlna] connect failed: {e}");
        return;
    }
    if let Err(e) = dlna_load(
        app.clone(),
        service.clone(),
        cast_service.clone(),
        path,
        0.0,
        Some("Frame Player".into()),
        false,
        // An external subtitle to send along — the one half of a load the
        // renderer's own menu cannot show us from here.
        std::env::var("FP_DLNA_SUB").ok(),
    )
    .await
    {
        eprintln!("[dlna] load failed: {e}");
        return;
    }
    for tick in 0..14 {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        if tick == 4 {
            match dlna_control(service.clone(), "seek".into(), Some(600.0)).await {
                Ok(()) => eprintln!("[dlna] seek to 10:00 sent"),
                Err(e) => eprintln!("[dlna] seek failed: {e}"),
            }
        }
        if tick == 8 {
            match dlna_control(service.clone(), "pause".into(), None).await {
                Ok(()) => eprintln!("[dlna] pause sent"),
                Err(e) => eprintln!("[dlna] pause failed: {e}"),
            }
        }
        if tick == 9 {
            // Seeking while paused is the case the renderer refuses outright
            // (UPnP 501) and `dlna_control` works around, so the selftest
            // exercises it here rather than the plain seek above.
            match dlna_control(service.clone(), "seek".into(), Some(900.0)).await {
                Ok(()) => eprintln!("[dlna] seek-while-paused ok"),
                Err(e) => eprintln!("[dlna] seek-while-paused failed: {e}"),
            }
        }
        if tick == 10 {
            let _ = dlna_control(service.clone(), "play".into(), None).await;
            eprintln!("[dlna] play sent");
        }
        let s = dlna_status(service.clone(), cast_service.clone());
        eprintln!(
            "[dlna] state={} t={:.1}/{:.1} vol={:.2} known={} fetches={}",
            s.state, s.time, s.duration, s.volume, s.volume_known, s.fetches
        );
    }
    // Print the diagnosis on the way out: the selftest is the only place it can
    // be exercised without the GUI, and a report nobody has read once is a
    // report that says what its author assumed.
    for l in cast_diagnose(
        target_ip.clone(),
        Some(8009),
        Some(location_for_report.clone()),
    )
    .await
    {
        eprintln!("[diag] {:<18} {:<8} {}", l.id, l.state, l.detail);
    }
    match dlna_disconnect(service.clone(), cast_service.clone()).await {
        Ok(last) => eprintln!("[dlna] disconnected at {last:.1}s"),
        Err(e) => eprintln!("[dlna] disconnect failed: {e}"),
    }
}
