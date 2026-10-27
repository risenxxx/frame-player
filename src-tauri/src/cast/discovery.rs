//! Finding Cast devices: the mDNS browse and the unicast sweep's probe.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::StreamExt as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::net::TcpStream;

use super::castv2::tls_connect;

// ---- Discovery -------------------------------------------------------------

/// One Cast device as the picker shows it. `id` is the mDNS fullname — unique
/// on the network and stable while the device is up, which is all the picker
/// needs; nothing here is ever persisted.
#[derive(Serialize, Deserialize, Clone)]
pub struct CastDeviceInfo {
    pub id: String,
    pub name: String,
    pub model: String,
    pub ip: String,
    pub port: u16,
    /// Whether the receiver has a picture at all, from the `ca` capability
    /// bitmask its announcement carries (bit 0, video out): a Nest speaker or
    /// a speaker group answers on the same service as a Chromecast and plays
    /// sound only. `None` where nothing said — a device found by the unicast
    /// sweep, whose setup endpoint is asked for its name and nothing else.
    #[serde(default)]
    pub video: Option<bool>,
}

pub(super) struct Discovery {
    daemon: mdns_sd::ServiceDaemon,
    pub(super) devices: Arc<Mutex<HashMap<String, CastDeviceInfo>>>,
    /// Devices that answered a unicast query (see [`crate::lan_sweep`]), with
    /// when they last did — the ones multicast never delivered. Kept apart from
    /// `devices` because nothing announces their departure: they age out.
    swept: Arc<Mutex<HashMap<String, (CastDeviceInfo, Instant)>>>,
    sweep: tauri::async_runtime::JoinHandle<()>,
}

/// How long a sweep waits between rounds, and how long a device that stopped
/// answering stays listed — two missed rounds, so one lost probe does not
/// blink the row.
const SWEEP_EVERY: Duration = Duration::from_secs(12);
const SWEEP_FORGET: Duration = Duration::from_secs(30);

const CAST_SERVICE: &str = "_googlecast._tcp.local.";

impl Discovery {
    /// Browse-only: we query for `_googlecast._tcp` and never advertise a
    /// service of our own, so nothing about the app or its files is announced
    /// to the network.
    ///
    /// Alongside the browse, a unicast sweep asks `hints` (addresses a device
    /// was seen at before) and every host of the LAN subnets directly — the
    /// case it exists for is a router that drops multicast between a wired or
    /// 5 GHz machine and a 2.4 GHz television.
    pub(super) fn start(hints: Vec<Ipv4Addr>) -> Result<Discovery, String> {
        let daemon = mdns_sd::ServiceDaemon::new().map_err(|e| format!("mdns: {e}"))?;
        let receiver = daemon.browse(CAST_SERVICE).map_err(|e| format!("mdns: {e}"))?;
        let devices: Arc<Mutex<HashMap<String, CastDeviceInfo>>> = Arc::default();
        let sink = devices.clone();
        // A plain thread rather than a tokio task: flume's blocking iterator is
        // the crate's own idiom, and the thread ends when the daemon shuts down
        // and the channel closes.
        std::thread::spawn(move || {
            for event in receiver.iter() {
                match event {
                    mdns_sd::ServiceEvent::ServiceResolved(info) => {
                        // Prefer a routable IPv4; a device that only announced
                        // link-local or v6 addresses is skipped rather than
                        // guessed at.
                        let ip = info
                            .addresses
                            .iter()
                            .filter_map(|a| match a.to_ip_addr() {
                                IpAddr::V4(v4) if !v4.is_loopback() && !v4.is_link_local() => {
                                    Some(v4)
                                }
                                _ => None,
                            })
                            .next();
                        let Some(ip) = ip else { continue };
                        let txt = |key: &str| {
                            info.txt_properties
                                .get_property_val_str(key)
                                .unwrap_or("")
                                .to_string()
                        };
                        let name = {
                            let friendly = txt("fn");
                            if friendly.is_empty() {
                                info.fullname
                                    .split('.')
                                    .next()
                                    .unwrap_or("Chromecast")
                                    .to_string()
                            } else {
                                friendly
                            }
                        };
                        // A name the owner gave it; a report replaces it.
                        crate::logs::remember("device", &name);
                        let device = CastDeviceInfo {
                            id: info.fullname.clone(),
                            name,
                            model: txt("md"),
                            ip: ip.to_string(),
                            port: info.port,
                            video: txt("ca").parse::<u64>().ok().map(|ca| ca & 1 == 1),
                        };
                        let mut map = sink.lock().unwrap_or_else(|p| p.into_inner());
                        map.insert(info.fullname.clone(), device);
                    }
                    mdns_sd::ServiceEvent::ServiceRemoved(_, fullname) => {
                        let mut map = sink.lock().unwrap_or_else(|p| p.into_inner());
                        map.remove(&fullname);
                    }
                    _ => {}
                }
            }
        });
        let swept: Arc<Mutex<HashMap<String, (CastDeviceInfo, Instant)>>> = Arc::default();
        let sweep = tauri::async_runtime::spawn(sweep_loop(hints, swept.clone()));
        Ok(Discovery { daemon, devices, swept, sweep })
    }

    pub(super) fn stop(self) {
        self.sweep.abort();
        let _ = self.daemon.stop_browse(CAST_SERVICE);
        let _ = self.daemon.shutdown();
    }

    /// Both halves as one list. A device multicast found wins over its swept
    /// twin (same id or same address): the browse also hears it leave.
    pub(super) fn list(&self) -> Vec<CastDeviceInfo> {
        let mut list: Vec<CastDeviceInfo> = {
            let map = self.devices.lock().unwrap_or_else(|p| p.into_inner());
            map.values().cloned().collect()
        };
        let swept = self.swept.lock().unwrap_or_else(|p| p.into_inner());
        for (device, _) in swept.values() {
            if !list.iter().any(|d| d.id.eq_ignore_ascii_case(&device.id) || d.ip == device.ip) {
                list.push(device.clone());
            }
        }
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    }
}

/// The unicast half of discovery: every sweep target asked directly, repeated
/// while the picker is open. See [`crate::lan_sweep`] for why this is not an
/// mDNS query aimed at each host — that was measured and Cast does not answer.
async fn sweep_loop(
    hints: Vec<Ipv4Addr>,
    sink: Arc<Mutex<HashMap<String, (CastDeviceInfo, Instant)>>>,
) {
    loop {
        let targets = crate::lan_sweep::sweep_targets(&hints);
        if crate::logs::verbose() {
            eprintln!("[cast] unicast sweep of {} addresses", targets.len());
        }
        let found: Vec<CastDeviceInfo> = futures_util::stream::iter(targets)
            .map(probe_cast)
            .buffer_unordered(SWEEP_PARALLEL)
            .filter_map(|d| async move { d })
            .collect()
            .await;
        {
            let mut map = sink.lock().unwrap_or_else(|p| p.into_inner());
            for device in found {
                if crate::logs::verbose() && !map.contains_key(&device.id) {
                    eprintln!("[cast] unicast: {} at {}", device.name, device.ip);
                }
                map.insert(device.id.clone(), (device, Instant::now()));
            }
            map.retain(|_, (_, seen)| seen.elapsed() < SWEEP_FORGET);
        }
        tokio::time::sleep(SWEEP_EVERY).await;
    }
}

/// How many addresses are in flight at once. Most of a sweep is addresses with
/// nobody behind them, each waiting out `SWEEP_CONNECT`, so a round costs about
/// `targets / SWEEP_PARALLEL` of those — measured on a /24, 253 addresses took
/// 18 s at 48 in flight and 6 s at 128. Remembered addresses go first, so a known device is found in
/// the first wave whatever this is.
pub(super) const SWEEP_PARALLEL: usize = 128;
/// Generous for a LAN on purpose: a device on Wi-Fi power save was measured
/// missing a 1.5 s connect on the first try and answering the next one.
const SWEEP_CONNECT: Duration = Duration::from_secs(3);

/// Is there a Cast receiver at `ip`, and what is it called?
///
/// `:8009` answering is the cheap filter; the name comes from
/// `:8008/setup/eureka_info` — measured answering on a Cast soundbar that
/// ignores unicast mDNS, and the receiver's own setup endpoint rather than
/// anything a third party serves. A device that keeps 8008 closed (newer
/// firmware may) is still admitted if 8009 completes a TLS handshake, which is
/// what tells a receiver from, say, a Tomcat AJP port; it is listed under its
/// address, the one name it is known by.
pub(super) async fn probe_cast(ip: Ipv4Addr) -> Option<CastDeviceInfo> {
    let addr = IpAddr::V4(ip);
    tokio::time::timeout(SWEEP_CONNECT, TcpStream::connect((addr, 8009)))
        .await
        .ok()?
        .ok()?;
    let info = crate::dlna::http()
        .get(format!("http://{ip}:8008/setup/eureka_info?params=name,ssdp_udn"))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .ok()
        .and_then(|r| r.status().is_success().then_some(r));
    let info = match info {
        Some(r) => r.json::<Value>().await.ok(),
        None => None,
    };
    let (name, udn) = eureka_identity(info.as_ref());
    let name = match name {
        Some(name) => {
            crate::logs::remember("device", &name);
            name
        }
        None => {
            tls_connect(addr, 8009).await.ok()?;
            ip.to_string()
        }
    };
    Some(CastDeviceInfo {
        // Not the mDNS fullname — that embeds the model, which nothing here
        // reports — so a profile saved under one is found by name+model.
        id: udn.map(|u| format!("unicast:{u}")).unwrap_or_else(|| format!("unicast:{ip}")),
        name,
        model: String::new(),
        ip: ip.to_string(),
        port: 8009,
        video: None,
    })
}

/// The name and the UDN out of an `eureka_info` answer; empty strings count as
/// absent. Pure, so the shape is pinned by a test.
fn eureka_identity(info: Option<&Value>) -> (Option<String>, Option<String>) {
    let field = |key: &str| {
        info.and_then(|v| v.get(key))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    (field("name"), field("ssdp_udn"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn eureka_names_the_device_and_blank_is_absent() {
        use super::eureka_identity;
        let v: serde_json::Value = serde_json::from_str(
            r#"{"name":"Living Room","ssdp_udn":"00000000-1111-2222-3333-444444444444","ip_address":"x"}"#,
        )
        .unwrap();
        assert_eq!(
            eureka_identity(Some(&v)),
            (Some("Living Room".into()), Some("00000000-1111-2222-3333-444444444444".into()))
        );
        let blank: serde_json::Value = serde_json::from_str(r#"{"name":"  "}"#).unwrap();
        assert_eq!(eureka_identity(Some(&blank)), (None, None));
        assert_eq!(eureka_identity(None), (None, None));
    }
}
