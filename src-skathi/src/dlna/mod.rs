//! DLNA, discovery only: version 1's SSDP search and unicast sweep (`dlna/discovery.rs`), the device
//! description and its string-scanning XML, and `GetProtocolInfo` (`control.rs`), compiled as they are.
//! The renderer list is what the picker shows; connecting is not ported (see `cast`).
#![allow(dead_code)]

#[path = "../../../src-tauri/src/dlna/control.rs"]
mod control;
#[path = "../../../src-tauri/src/dlna/description.rs"]
mod description;
#[path = "../../../src-tauri/src/dlna/discovery.rs"]
pub mod discovery;
#[path = "../../../src-tauri/src/dlna/xml.rs"]
mod xml;

pub(crate) use control::http;

// As in version 1's dlna/mod.rs.
pub(crate) const SSDP_ADDR: &str = "239.255.255.250:1900";
const CONNECTION_MANAGER: &str = "urn:schemas-upnp-org:service:ConnectionManager:1";

/// One renderer as the picker shows it, and as `dlna_connect` takes it back (version 1's dlna/mod.rs).
#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
pub struct DlnaDeviceInfo {
    pub id: String,
    pub name: String,
    pub model: String,
    pub ip: String,
    pub control_url: String,
    pub rendering_url: Option<String>,
    pub mimes: Vec<String>,
}

// ---- Version 1's `dlna_discover_start`, `dlna_discover_stop` and `dlna_devices` (dlna/mod.rs).

type Live = std::sync::Arc<std::sync::Mutex<Vec<DlnaDeviceInfo>>>;
static DISCOVERY: std::sync::Mutex<Option<(tauri::async_runtime::JoinHandle<()>, Live)>> = std::sync::Mutex::new(None);

pub fn discover_start(hints: Vec<String>) -> Result<(), String> {
    use std::time::Duration;
    let mut d = DISCOVERY.lock().unwrap_or_else(|p| p.into_inner());
    if d.is_some() {
        return Ok(());
    }
    let hints = crate::lan_sweep::parse_hints(&hints);
    let list: Live = Default::default();
    let sink = list.clone();
    // Re-searched rather than searched once: SSDP replies are UDP and a device that missed the first
    // M-SEARCH would otherwise never appear.
    let task = tauri::async_runtime::spawn(async move {
        loop {
            let found = discovery::collect_renderers(Duration::from_secs(3), &hints, &sink).await;
            if !found.is_empty() {
                *sink.lock().unwrap_or_else(|p| p.into_inner()) = found;
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
    *d = Some((task, list));
    Ok(())
}

pub fn discover_stop() {
    if let Some((task, _)) = DISCOVERY.lock().unwrap_or_else(|p| p.into_inner()).take() {
        task.abort();
    }
}

pub fn devices() -> Vec<DlnaDeviceInfo> {
    DISCOVERY.lock().unwrap_or_else(|p| p.into_inner()).as_ref().map(|(_, l)| l.lock().unwrap_or_else(|p| p.into_inner()).clone()).unwrap_or_default()
}
