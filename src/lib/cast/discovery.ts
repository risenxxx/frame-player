/** Finding televisions: both discoveries, the AirPlay route check, and the addresses remembered for the next search. */

import { invoke } from '@tauri-apps/api/core';

import { IS_MAC } from '../platform';
import { refreshCastPlan } from './ladder';
import { cast, type CastDevice, type DlnaDevice } from './state.svelte';

const DEVICE_POLL_MS = 800;
/// How long the picker admits it is still looking for a second transport.
const DLNA_SWEEP_MS = 4500;
/// Rebuild discovery from scratch after this many empty polls.
///
/// **A permission granted after the sockets were opened does not reach them.**
/// On macOS the system asks for Local Network access the first time something
/// multicasts, and a third-party firewall may ask again per destination; both
/// prompts are answered *after* the search has started, and the sockets that
/// were refused in the meantime stay refused. Re-sending queries on them
/// changes nothing — the daemon has to be torn down and built again, which is
/// what turns "allow" into devices appearing without the viewer closing the
/// panel and opening it a second time to make it work.
///
/// **Only Cast is rebuilt.** DLNA opens fresh sockets for every search round,
/// so a late permission reaches it on its own, and tearing it down bought
/// nothing but a lost round — which on a network with no Cast device was every
/// round: a renderer whose round outlasted this window (one LOCATION that
/// never answered was enough) was aborted, restarted and aborted again, and the
/// picker said it was still searching forever.
const REBUILD_AFTER_EMPTY_POLLS = 9;

let deviceTimer: ReturnType<typeof setInterval> | null = null;

export function startCastDiscovery() {
  if (devicetimerRunning()) return;
  cast.discovering = true;
  emptyPolls = 0;
  cast.rebuilds = 0;
  const hints = loadHosts();
  void invoke('cast_discover_start', { hints }).catch((e) => {
    console.warn('cast_discover_start failed:', e);
    cast.discovering = false;
  });
  void invoke('dlna_discover_start', { hints }).catch((e) =>
    console.warn('dlna_discover_start failed:', e),
  );
  cast.dlnaSweeping = true;
  if (IS_MAC) void invoke('airplay_detect', { on: true }).catch(() => {});
  // Cleared by the first sweep that finds anything, or by this deadline — a
  // spinner that never stops is worse than one that gives up.
  setTimeout(() => (cast.dlnaSweeping = false), DLNA_SWEEP_MS);
  void refreshCastPlan();
  const pull = async () => {
    const [castList, dlnaList, airplayRoutes] = await Promise.all([
      invoke<CastDevice[]>('cast_devices').catch(() => []),
      invoke<DlnaDevice[]>('dlna_devices').catch(() => []),
      IS_MAC ? invoke<boolean>('airplay_routes').catch(() => false) : false,
    ]);
    cast.devices = castList;
    cast.dlnaDevices = dlnaList;
    cast.airplayRoutes = airplayRoutes;
    rememberHosts([...castList.map((d) => d.ip), ...dlnaList.map((d) => d.ip)]);
    if (dlnaList.length > 0) cast.dlnaSweeping = false;
    if (castList.length || dlnaList.length) {
      emptyPolls = 0;
    } else if (++emptyPolls >= REBUILD_AFTER_EMPTY_POLLS) {
      void rebuildDiscovery();
    }
    // Only while the source is a torrent still downloading: it is the one plan
    // that changes under the panel, and re-reading it for a plain file would be
    // a handful of mpv round trips every tick for an answer that cannot move.
    if (cast.plan?.streaming) void refreshCastPlan();
  };
  void pull();
  deviceTimer = setInterval(() => void pull(), DEVICE_POLL_MS);
}

/// Tear Cast discovery down and start it again. Cheap: one mDNS daemon. DLNA
/// is left running — see `REBUILD_AFTER_EMPTY_POLLS`.
async function rebuildDiscovery() {
  emptyPolls = 0;
  cast.rebuilds++;
  await invoke('cast_discover_stop').catch(() => {});
  await invoke('cast_discover_start', { hints: loadHosts() }).catch(() => {});
}

let emptyPolls = 0;

/**
 * Addresses a television answered at, newest first — asked directly before
 * the rest of the network on the next search.
 *
 * Discovery is multicast, and plenty of routers drop multicast between a wired
 * or 5 GHz machine and a 2.4 GHz television while routing everything else
 * between them, so Rust also sweeps the LAN with unicast queries (lan_sweep.rs).
 * The sweep covers the subnet by itself; what these add is the television that
 * lives somewhere a sweep does not reach — a subnet too wide to sweep whole, or
 * another band the router puts on a subnet of its own — once it has been seen
 * even a single time. An address, not an identity: a DHCP lease moves, and a
 * stale one costs one unanswered datagram.
 */
const HOSTS_KEY = 'frameplayer.tv-hosts';
const HOSTS_LIMIT = 8;

function loadHosts(): string[] {
  try {
    const stored: unknown = JSON.parse(localStorage.getItem(HOSTS_KEY) ?? '[]');
    return Array.isArray(stored) ? stored.filter((h): h is string => typeof h === 'string') : [];
  } catch {
    return [];
  }
}

function rememberHosts(seen: string[]) {
  if (seen.length === 0) return;
  const known = loadHosts();
  const next = [...new Set([...seen, ...known])].slice(0, HOSTS_LIMIT);
  // The poll runs every 800 ms; only a change is worth a write.
  if (next.join() === known.join()) return;
  try {
    localStorage.setItem(HOSTS_KEY, JSON.stringify(next));
  } catch {
    // A full or blocked storage costs only the shortcut.
  }
}

function devicetimerRunning(): boolean {
  return deviceTimer !== null;
}

export function stopCastDiscovery() {
  if (deviceTimer !== null) {
    clearInterval(deviceTimer);
    deviceTimer = null;
  }
  cast.discovering = false;
  cast.dlnaSweeping = false;
  void invoke('cast_discover_stop').catch(() => {});
  void invoke('dlna_discover_stop').catch(() => {});
  if (IS_MAC) void invoke('airplay_detect', { on: false }).catch(() => {});
}
