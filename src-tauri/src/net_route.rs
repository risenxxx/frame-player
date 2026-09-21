//! Which network interface the torrent traffic leaves by.
//!
//! **The question this answers is "past the VPN or through it", and the player
//! does not route anything itself to answer it.** librqbit already knows how to
//! scope every socket it opens to one interface (`SessionOptions::
//! bind_device_name` — `IP_BOUND_IF` on macOS, `IP_UNICAST_IF` on Windows via
//! the vendored `librqbit-dualstack-sockets`), and a VPN's default route lives
//! on its own adapter, so a socket scoped to the Wi-Fi simply never considers
//! it. What is left for this module is the part only the player can do: say
//! which interfaces exist, which of them is a tunnel, which one is the way out
//! *without* the tunnel, and where the system would send a packet right now.
//!
//! Three things it deliberately does not do. It does not touch the routing
//! table — nothing here needs privileges or leaves a trace on the machine. It
//! does not promise the bypass works: a VPN with a kill switch
//! (`includeAllNetworks` on macOS, WFP filters on Windows — WireGuard turns one
//! on by itself for a `0.0.0.0/0` tunnel) drops scoped packets as readily as any
//! others, and that cannot be seen from here without sending traffic. And it
//! does not cover the loopback server mpv reads from, the casting server or the
//! catalog: this is about the swarm.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

/// One interface, as the settings list shows it.
#[derive(Clone, Debug, serde::Serialize)]
pub struct NetIface {
    /// What the bind is made with: the BSD name on macOS (`en0`), the alias on
    /// Windows (`Wi-Fi`, `Ethernet 2`).
    pub name: String,
    /// What a person calls it: "Wi-Fi", "USB 10/100/1000 LAN". Equal to `name`
    /// when the system has nothing better (a VPN's `utun4`).
    pub label: String,
    /// The first usable address, for recognising which one is which.
    pub addr: Option<String>,
    /// A tunnel rather than a way out of the machine.
    pub vpn: bool,
    /// Has a router behind it. Only such an interface can carry a bypass.
    #[serde(skip)]
    gateway: bool,
    /// Lower first: the order the system itself would prefer them in.
    #[serde(skip)]
    rank: u32,
    #[serde(skip)]
    ips: Vec<IpAddr>,
}

/// What the frontend asked for, in its own vocabulary — persisted in
/// localStorage, so it is ours rather than an enum of librqbit's.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Route {
    /// Whatever the system does — through the VPN when one is up. What every
    /// release before this one did, and the default.
    #[default]
    Auto,
    /// The interface the machine would use with no tunnel, found anew each
    /// time a session is built (Wi-Fi today, Ethernet tomorrow).
    Direct,
    /// One named interface, whatever it is — including a particular tunnel.
    Iface { name: String },
}

/// The settings view: every interface, which one is the bypass, and where the
/// traffic goes if nobody chooses.
#[derive(Clone, Debug, serde::Serialize)]
pub struct NetView {
    pub interfaces: Vec<NetIface>,
    /// The interface `Route::Direct` resolves to now, if there is one.
    pub direct: Option<String>,
    /// The interface the system route to the internet uses right now.
    pub via: Option<String>,
    /// That interface is a tunnel — i.e. `Route::Auto` means "through the VPN".
    pub via_vpn: bool,
}

pub fn view() -> NetView {
    let all = interfaces();
    let direct = direct_of(&all).map(|i| i.name.clone());
    let via = default_route_iface(&all).cloned();
    let via_vpn = via.as_ref().is_some_and(|v| v.vpn);
    // **Only what can carry a torrent is offered**: a way out with a router
    // behind it, or a tunnel. A virtual machine's bridge has an address and no
    // route anywhere — measured on a Mac with three of them, they were half the
    // list — and choosing one is a torrent that never finds a peer.
    let interfaces = all.into_iter().filter(|i| i.vpn || i.gateway).collect();
    NetView {
        direct,
        via: via.map(|v| v.name),
        via_vpn,
        interfaces,
    }
}

/// Turn a preference into the device name librqbit binds to, or `None` for the
/// system route. An error is a sentence key the frontend translates — failing
/// is the honest answer, since quietly falling back to the system route would
/// send the traffic exactly where the viewer asked it not to go.
pub fn resolve(route: &Route) -> Result<Option<String>, String> {
    match route {
        Route::Auto => Ok(None),
        Route::Direct => direct_of(&interfaces())
            .map(|i| Some(i.name.clone()))
            .ok_or_else(|| "route_no_direct".to_string()),
        Route::Iface { name } => {
            if interfaces().iter().any(|i| &i.name == name) {
                Ok(Some(name.clone()))
            } else {
                Err(format!("route_missing:{name}"))
            }
        }
    }
}

/// The bypass: a way out that is not a tunnel and has a router behind it, in
/// the system's own order of preference.
fn direct_of(list: &[NetIface]) -> Option<&NetIface> {
    list.iter()
        .filter(|i| !i.vpn && i.gateway && i.ips.iter().any(|ip| ip.is_ipv4()))
        .min_by_key(|i| i.rank)
}

/// Which interface the system would send a packet to the internet by, asked
/// of the system itself rather than guessed from names. `connect` on a UDP
/// socket sends nothing — it only runs the route lookup and fixes the source
/// address — so this costs one syscall and no traffic, and it sees every way a
/// VPN can take the route: a replaced default, the `0/1` + `128/1` pair, a
/// scoped primary service.
fn default_route_iface(list: &[NetIface]) -> Option<&NetIface> {
    // Any public address serves: nothing is sent, and a VPN that routes the
    // internet routes all of it.
    let probe = SocketAddr::from((Ipv4Addr::new(1, 1, 1, 1), 53));
    let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    sock.connect(probe).ok()?;
    let local = sock.local_addr().ok()?.ip();
    list.iter().find(|i| i.ips.contains(&local))
}

/// Name prefixes that are never a way out of the machine on macOS: tunnels of
/// every kind, and Apple's own peer-to-peer links, which carry addresses but no
/// route anywhere.
#[cfg(target_os = "macos")]
const MAC_TUNNEL: &[&str] = &["utun", "ipsec", "ppp", "tun", "tap", "wg", "gif", "stf"];
#[cfg(target_os = "macos")]
const MAC_SKIP: &[&str] = &["lo", "awdl", "llw", "anpi", "ap"];

/// A usable address: not loopback, not link-local. A tunnel interface that
/// exists but is not up carries only `fe80::`, which is what hides the four
/// idle `utun`s every Mac has.
fn usable(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => !v4.is_loopback() && !v4.is_link_local() && !v4.is_unspecified(),
        IpAddr::V6(v6) => {
            !v6.is_loopback() && !v6.is_unspecified() && (v6.segments()[0] & 0xffc0) != 0xfe80
        }
    }
}

fn first_addr(ips: &[IpAddr]) -> Option<String> {
    ips.iter()
        .find(|ip| ip.is_ipv4())
        .or_else(|| ips.first())
        .map(|ip| ip.to_string())
}

#[cfg(target_os = "macos")]
pub fn interfaces() -> Vec<NetIface> {
    use std::collections::BTreeMap;
    use std::ffi::CStr;

    let sc = mac_sc::read();
    let mut by_name: BTreeMap<String, (u32, Vec<IpAddr>)> = BTreeMap::new();
    unsafe {
        let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut head) != 0 {
            return Vec::new();
        }
        let mut cur = head;
        while !cur.is_null() {
            let ifa = &*cur;
            cur = ifa.ifa_next;
            let flags = ifa.ifa_flags;
            if flags & libc::IFF_UP as u32 == 0 || flags & libc::IFF_RUNNING as u32 == 0 {
                continue;
            }
            let name = CStr::from_ptr(ifa.ifa_name).to_string_lossy().into_owned();
            if ifa.ifa_addr.is_null() {
                continue;
            }
            let ip = match (*ifa.ifa_addr).sa_family as i32 {
                libc::AF_INET => {
                    let sin = &*(ifa.ifa_addr as *const libc::sockaddr_in);
                    IpAddr::V4(Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)))
                }
                libc::AF_INET6 => {
                    let sin6 = &*(ifa.ifa_addr as *const libc::sockaddr_in6);
                    IpAddr::from(sin6.sin6_addr.s6_addr)
                }
                _ => continue,
            };
            let entry = by_name.entry(name).or_insert((flags, Vec::new()));
            if usable(&ip) {
                entry.1.push(ip);
            }
        }
        libc::freeifaddrs(head);
    }

    by_name
        .into_iter()
        .filter(|(name, (_, ips))| {
            !ips.is_empty() && !MAC_SKIP.iter().any(|p| starts_with_unit(name, p))
        })
        .map(|(name, (flags, ips))| {
            let vpn = flags & libc::IFF_POINTOPOINT as u32 != 0
                || MAC_TUNNEL.iter().any(|p| starts_with_unit(&name, p));
            let index = {
                let c = std::ffi::CString::new(name.clone()).unwrap_or_default();
                unsafe { libc::if_nametoindex(c.as_ptr()) }
            };
            NetIface {
                label: sc.labels.get(&name).cloned().unwrap_or_else(|| name.clone()),
                addr: first_addr(&ips),
                gateway: sc.routed.contains_key(&name),
                // Service order first (what System Settings shows as the
                // order of networks), the index as the tie-break.
                rank: sc.routed.get(&name).copied().unwrap_or(1000) * 1000 + index,
                vpn,
                ips,
                name,
            }
        })
        .collect()
}

/// `utun4` starts with `utun`, but `en0` must not start with `e`: a prefix
/// counts only when what follows it is the unit number.
#[cfg(target_os = "macos")]
fn starts_with_unit(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit()))
}

/// What SystemConfiguration knows and `getifaddrs` does not: the names people
/// see, and which interfaces have a router — with the rank of the service that
/// owns each, which is the system's order of preference.
#[cfg(target_os = "macos")]
mod mac_sc {
    use core_foundation::array::{CFArray, CFArrayRef};
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::string::{CFString, CFStringRef};
    use std::collections::HashMap;
    use std::ffi::c_void;

    #[link(name = "SystemConfiguration", kind = "framework")]
    unsafe extern "C" {
        fn SCDynamicStoreCreate(
            alloc: *const c_void,
            name: CFStringRef,
            callout: *const c_void,
            ctx: *const c_void,
        ) -> *const c_void;
        fn SCDynamicStoreCopyValue(store: *const c_void, key: CFStringRef) -> *const c_void;
        fn SCDynamicStoreCopyKeyList(store: *const c_void, pattern: CFStringRef) -> CFArrayRef;
        fn SCNetworkInterfaceCopyAll() -> CFArrayRef;
        fn SCNetworkInterfaceGetBSDName(iface: *const c_void) -> CFStringRef;
        fn SCNetworkInterfaceGetLocalizedDisplayName(iface: *const c_void) -> CFStringRef;
    }

    #[derive(Default)]
    pub struct Sc {
        /// BSD name → "Wi-Fi".
        pub labels: HashMap<String, String>,
        /// BSD name → position of its service in the service order, for every
        /// interface whose IPv4 state names a router.
        pub routed: HashMap<String, u32>,
    }

    fn string_of(r: CFStringRef) -> Option<String> {
        (!r.is_null()).then(|| unsafe { CFString::wrap_under_get_rule(r) }.to_string())
    }

    fn dict_string(d: &CFDictionary<CFString, CFType>, key: &str) -> Option<String> {
        d.find(CFString::new(key))
            .and_then(|v| v.downcast::<CFString>())
            .map(|s| s.to_string())
    }

    pub fn read() -> Sc {
        let mut sc = Sc::default();
        unsafe {
            let all = SCNetworkInterfaceCopyAll();
            if !all.is_null() {
                let all: CFArray<CFType> = CFArray::wrap_under_create_rule(all);
                for item in all.iter() {
                    let p = item.as_CFTypeRef();
                    if let (Some(bsd), Some(label)) = (
                        string_of(SCNetworkInterfaceGetBSDName(p)),
                        string_of(SCNetworkInterfaceGetLocalizedDisplayName(p)),
                    ) {
                        sc.labels.insert(bsd, label);
                    }
                }
            }

            let name = CFString::new("frame-player");
            let store = SCDynamicStoreCreate(
                std::ptr::null(),
                name.as_concrete_TypeRef(),
                std::ptr::null(),
                std::ptr::null(),
            );
            if store.is_null() {
                return sc;
            }
            let store_ref = CFType::wrap_under_create_rule(store);

            let copy_dict = |key: &str| -> Option<CFDictionary<CFString, CFType>> {
                let k = CFString::new(key);
                let v = SCDynamicStoreCopyValue(store, k.as_concrete_TypeRef());
                (!v.is_null()).then(|| {
                    CFDictionary::wrap_under_create_rule(v as CFDictionaryRef)
                })
            };

            // The order System Settings shows, as service ids.
            let order: Vec<String> = copy_dict("Setup:/Network/Global/IPv4")
                .and_then(|d| d.find(CFString::new("ServiceOrder")).map(|v| v.clone()))
                // `CFArray` has no `downcast` (it is generic, so not a
                // `ConcreteCFType`): checked by type id and wrapped by hand.
                .filter(|v| v.type_of() == CFArray::<CFType>::type_id())
                .map(|v| CFArray::<CFType>::wrap_under_get_rule(v.as_CFTypeRef() as CFArrayRef))
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.downcast::<CFString>().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            let pattern = CFString::new("State:/Network/Service/[^/]+/IPv4");
            let keys = SCDynamicStoreCopyKeyList(store, pattern.as_concrete_TypeRef());
            if !keys.is_null() {
                let keys: CFArray<CFType> = CFArray::wrap_under_create_rule(keys);
                for key in keys.iter() {
                    let Some(key) = key.downcast::<CFString>().map(|k| k.to_string()) else {
                        continue;
                    };
                    let Some(d) = copy_dict(&key) else { continue };
                    let (Some(iface), Some(_router)) =
                        (dict_string(&d, "InterfaceName"), dict_string(&d, "Router"))
                    else {
                        continue;
                    };
                    let id = key
                        .trim_start_matches("State:/Network/Service/")
                        .trim_end_matches("/IPv4");
                    let rank = order.iter().position(|s| s == id).unwrap_or(999) as u32;
                    let e = sc.routed.entry(iface).or_insert(rank);
                    *e = (*e).min(rank);
                }
            }
            drop(store_ref);
        }
        sc
    }
}

#[cfg(windows)]
pub fn interfaces() -> Vec<NetIface> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
        GAA_FLAG_SKIP_MULTICAST, GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR};

    /// IANA ifType values that are a way out of the machine: Ethernet and
    /// 802.11. Everything else (PPP 23, tunnel 131, "proprietary virtual" 53 —
    /// which is what Wintun, and so WireGuard and most modern VPNs, present)
    /// is either a tunnel or not a route anywhere.
    const IF_ETHERNET: u32 = 6;
    const IF_WIFI: u32 = 71;
    const IF_PPP: u32 = 23;
    const IF_VIRTUAL: u32 = 53;
    const IF_TUNNEL: u32 = 131;
    const IF_LOOPBACK: u32 = 24;
    /// Adapters that present as Ethernet and are VPNs all the same — OpenVPN's
    /// TAP driver, AnyConnect's miniport and their kind. Matched against the
    /// driver description, lower-cased.
    const VPN_WORDS: &[&str] = &[
        "tap-windows", "wintun", "wireguard", "openvpn", "ovpn", "vpn", "tunnel",
        "nordlynx", "anyconnect", "fortinet", "globalprotect", "pangp", "tailscale",
        "zerotier", "hamachi", "radmin",
    ];

    fn wide(p: *const u16) -> String {
        if p.is_null() {
            return String::new();
        }
        let mut len = 0;
        unsafe {
            while *p.add(len) != 0 {
                len += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
        }
    }

    fn ip_of(sa: *const SOCKADDR) -> Option<IpAddr> {
        if sa.is_null() {
            return None;
        }
        // Read by offset rather than through the union-heavy structs: the
        // layout of `sockaddr_in`/`sockaddr_in6` is the one fixed thing here.
        unsafe {
            let bytes = sa as *const u8;
            match (*sa).sa_family {
                AF_INET => {
                    let mut a = [0u8; 4];
                    std::ptr::copy_nonoverlapping(bytes.add(4), a.as_mut_ptr(), 4);
                    Some(IpAddr::from(a))
                }
                AF_INET6 => {
                    let mut a = [0u8; 16];
                    std::ptr::copy_nonoverlapping(bytes.add(8), a.as_mut_ptr(), 16);
                    Some(IpAddr::from(a))
                }
                _ => None,
            }
        }
    }

    let flags = GAA_FLAG_INCLUDE_GATEWAYS
        | GAA_FLAG_SKIP_ANYCAST
        | GAA_FLAG_SKIP_MULTICAST
        | GAA_FLAG_SKIP_DNS_SERVER;
    // The documented dance: 15 KB is Microsoft's recommended first guess, and
    // the call says how much it wanted when that is not enough.
    let mut size: u32 = 15 * 1024;
    let mut buf: Vec<u64> = Vec::new();
    for _ in 0..4 {
        buf = vec![0u64; (size as usize).div_ceil(8)];
        let rc = unsafe {
            GetAdaptersAddresses(
                AF_UNSPEC as u32,
                flags,
                std::ptr::null(),
                buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH,
                &mut size,
            )
        };
        match rc {
            0 => break,
            111 /* ERROR_BUFFER_OVERFLOW */ => continue,
            _ => return Vec::new(),
        }
    }

    let mut out = Vec::new();
    let mut cur = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    while !cur.is_null() {
        let a = unsafe { &*cur };
        cur = a.Next;
        if a.OperStatus != IfOperStatusUp || a.IfType == IF_LOOPBACK {
            continue;
        }
        let mut ips = Vec::new();
        let mut u = a.FirstUnicastAddress;
        while !u.is_null() {
            let ua = unsafe { &*u };
            if let Some(ip) = ip_of(ua.Address.lpSockaddr) {
                if usable(&ip) {
                    ips.push(ip);
                }
            }
            u = ua.Next;
        }
        if ips.is_empty() {
            continue;
        }
        let gateway = !a.FirstGatewayAddress.is_null();
        let description = wide(a.Description).to_lowercase();
        let vpn = matches!(a.IfType, IF_PPP | IF_VIRTUAL | IF_TUNNEL)
            || VPN_WORDS.iter().any(|w| description.contains(w));
        let physical = matches!(a.IfType, IF_ETHERNET | IF_WIFI);
        let name = wide(a.FriendlyName);
        let index = unsafe { a.Anonymous1.Anonymous.IfIndex };
        out.push(NetIface {
            label: name.clone(),
            addr: first_addr(&ips),
            vpn,
            // A virtual switch with no router behind it (Hyper-V's Default
            // Switch) is Ethernet too; the gateway is what rules it out.
            gateway: gateway && physical,
            // Windows' own choice between two ways out is the metric.
            rank: a.Ipv4Metric.saturating_mul(1000).saturating_add(index),
            ips,
            name,
        });
    }
    out
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn interfaces() -> Vec<NetIface> {
    Vec::new()
}

/// The settings list and what the system route is doing right now.
#[tauri::command]
pub async fn net_interfaces() -> NetView {
    tauri::async_runtime::spawn_blocking(view)
        .await
        .unwrap_or(NetView { interfaces: Vec::new(), direct: None, via: None, via_vpn: false })
}

/// Scope a reqwest client to the same interface as the session, for the
/// requests the player makes to trackers itself. `interface()` on macOS; on
/// Windows reqwest has no such thing, so the interface's address is bound
/// instead, and the strong host model does the rest.
pub fn scope_client(builder: reqwest::ClientBuilder, device: Option<&str>) -> reqwest::ClientBuilder {
    let Some(device) = device else { return builder };
    #[cfg(target_os = "macos")]
    {
        builder.interface(device)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let ip = interfaces()
            .into_iter()
            .find(|i| i.name == device)
            .and_then(|i| i.ips.into_iter().find(|ip| ip.is_ipv4()));
        match ip {
            Some(ip) => builder.local_address(ip),
            None => builder,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_wire_format() {
        // The frontend persists these, so the spelling is a contract.
        let r: Route = serde_json::from_str(r#"{"kind":"auto"}"#).unwrap();
        assert_eq!(r, Route::Auto);
        let r: Route = serde_json::from_str(r#"{"kind":"direct"}"#).unwrap();
        assert_eq!(r, Route::Direct);
        let r: Route = serde_json::from_str(r#"{"kind":"iface","name":"en0"}"#).unwrap();
        assert_eq!(r, Route::Iface { name: "en0".into() });
    }

    #[test]
    fn link_local_is_not_usable() {
        assert!(!usable(&"fe80::1".parse().unwrap()));
        assert!(!usable(&"169.254.3.4".parse().unwrap()));
        assert!(usable(&"192.168.1.20".parse().unwrap()));
        assert!(usable(&"fd00::5".parse().unwrap()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn unit_prefix() {
        assert!(starts_with_unit("utun4", "utun"));
        assert!(!starts_with_unit("en0", "e"));
        assert!(starts_with_unit("ap1", "ap"));
        assert!(!starts_with_unit("apple0x", "ap"));
    }

    /// What this machine has — for looking at, not asserting: run with
    /// `cargo test --lib net_route::tests::show -- --nocapture`.
    #[test]
    fn show() {
        let v = view();
        for i in &v.interfaces {
            println!(
                "{:<10} {:<28} {:<40} vpn={} gw={} rank={}",
                i.name, i.label, i.addr.clone().unwrap_or_default(), i.vpn, i.gateway, i.rank
            );
        }
        println!("direct={:?} via={:?} via_vpn={}", v.direct, v.via, v.via_vpn);
    }
}
