/**
 * The preferences a torrent session is built with — seeding, the forwarded
 * port, encryption, the proxy, the route — and the network questions that
 * decide them: where the system route goes, the VPN question at the first
 * torrent of a run, and whether the router really forwarded the port.
 */

import { invoke } from '@tauri-apps/api/core';

import { latest } from '../latest';
import { type PortStatus, torrent } from './state.svelte';

const PREFS_KEY = 'frameplayer.torrent';

/**
 * Whether to upload to other peers while streaming.
 *
 * **Off by default, and that default is a safety decision rather than a
 * technical one.** In Germany and several other jurisdictions the exposure from
 * *uploading* copyrighted material is categorically worse than from downloading
 * it — enforcement there works by connecting to peers who are advertising
 * pieces and recording that they served data. A video player must not opt its
 * users into that silently, and "the client defaults to it" is not a reason: the
 * viewer asked to watch something, not to redistribute it.
 *
 * What it buys when off is real but bounded, and worth stating honestly rather
 * than overselling: librqbit stops advertising which pieces it holds (no
 * bitfield, no `have` messages), refuses piece requests outright, and drops
 * peers once the file is complete. What it does **not** do is make the client
 * invisible — finding peers at all means announcing to trackers and the DHT, so
 * membership of the swarm is still observable. It removes the serving, not the
 * presence.
 */
/// How peer connections are encrypted. Our own vocabulary rather than
/// librqbit's `MseMode`, because this is what gets persisted — see the Rust
/// `Encryption` for the same argument from the other side.
export type Encryption = 'off' | 'on' | 'only';

/**
 * Which interface the swarm traffic leaves by — the spelling `net_route::Route`
 * deserializes, and persisted as it is.
 *
 * `auto` is the system route, which is through the VPN when one is up and is
 * what every release before this one did. `direct` is the way out the machine
 * would use with no tunnel, found anew for every session (the Wi-Fi today, the
 * Ethernet tomorrow). `iface` is one named interface, a particular tunnel
 * included.
 */
export type Route = { kind: 'auto' } | { kind: 'direct' } | { kind: 'iface'; name: string };

export interface NetIface {
  /// What the bind is made with: `en0` on macOS, the adapter alias on Windows.
  name: string;
  /// What a person calls it — "Wi-Fi".
  label: string;
  addr: string | null;
  vpn: boolean;
}

export interface NetView {
  interfaces: NetIface[];
  /// What `direct` resolves to right now, if anything.
  direct: string | null;
  /// Where the system route to the internet goes right now.
  via: string | null;
  /// …and that is a tunnel, i.e. `auto` means "through the VPN".
  via_vpn: boolean;
}

function routeOk(r: unknown): r is Route {
  if (!r || typeof r !== 'object') return false;
  const k = (r as { kind?: unknown }).kind;
  if (k === 'auto' || k === 'direct') return true;
  return k === 'iface' && typeof (r as { name?: unknown }).name === 'string';
}

export function sameRoute(a: Route, b: Route): boolean {
  return a.kind === b.kind && (a.kind !== 'iface' || a.name === (b as { name: string }).name);
}

const ENCRYPTION_CHOICES: readonly Encryption[] = ['off', 'on', 'only'];

class TorrentPrefs {
  seeding = $state(false);

  /**
   * Ask the router to open the BitTorrent port (UPnP).
   *
   * **Off by default because it changes the machine rather than the app**, and
   * on at all because it is the only large lever left on peer count. Measured on
   * one private tracker's swarm, one address at a time: of ~30 it handed out,
   * **20–22 never answered a SYN** — peers behind NAT, reachable only if they
   * dial us — 5 completed a handshake, and exactly one showed the mid-handshake
   * cut that means DPI. Everything else that suggests itself was measured and is
   * worth nothing here: public trackers know 0–1 extra peers for a
   * tracker-exclusive release, `numwant` is capped by the tracker, `peers6` is
   * not sent at all, and PEX already runs.
   *
   * It helps even with seeding off, which is the counter-intuitive part: a seed
   * serves us whether or not we serve anyone, so being reachable is about who
   * can *start* a connection, not about what goes out.
   */
  portForward = $state(false);

  /**
   * Whether to obfuscate the peer stream (MSE/PE).
   *
   * **On by default, and that is the point of it.** A plaintext BitTorrent
   * handshake opens with `\x13BitTorrent protocol`, which is what a middlebox
   * matches on — and cutting the connection right after it is what several
   * ISPs measurably do, showing up here as a peer that connects and then dies.
   * With MSE the first bytes are a Diffie-Hellman exchange instead. `on` falls
   * back to a plaintext redial when a peer will not encrypt, so it is never
   * worse than what shipped before; `only` drops such peers, which is for a
   * network where the plaintext handshake is the thing being cut.
   *
   * It does not hide *who* is being talked to, and it does not cover the DHT or
   * the trackers — those are their own plaintext sockets.
   */
  encryption = $state<Encryption>('on');

  /**
   * A SOCKS5 proxy for the torrent traffic, or empty for none.
   *
   * **One of two ways to send torrents somewhere other than the VPN**, and the
   * one that keeps this machine's address out of the swarm: a relay the viewer
   * chooses — a VPN provider's own SOCKS5 endpoint, a VPS. The other is `route`,
   * which leaves by the machine's own connection and says what that costs.
   *
   * Two limits worth knowing before it disappoints somebody. It carries the
   * peer connections and the HTTP announces and **nothing else**: the DHT and
   * the UDP trackers are their own sockets and keep going out the ordinary way.
   * And what travels through it is unencrypted BitTorrent, so a network that
   * filters on the protocol rather than on where it is going is unaffected —
   * that one needs MSE, which librqbit does not implement.
   */
  proxy = $state('');

  /**
   * Which interface the swarm traffic leaves by. See `Route`.
   *
   * **The default is the system's choice**, which means through the VPN when
   * one is up — nothing changes for anybody who never opens the setting. What
   * the other two buy is the case the proxy could not answer: a VPN that
   * forbids torrents, on a machine whose own connection carries them. And the
   * one thing to say wherever it is offered: past the VPN, the swarm sees this
   * machine's own address.
   */
  route = $state<Route>({ kind: 'auto' });

  /**
   * Whether to ask, at the first torrent of a run, when the system route goes
   * through a VPN. Off once somebody says "through the VPN, and stop asking" —
   * the question is for the viewer who did not know, not a toll on the one who
   * chose.
   */
  vpnAsk = $state(true);
}

export const torrentPrefs = new TorrentPrefs();

export function loadTorrentPrefs() {
  try {
    const raw = localStorage.getItem(PREFS_KEY);
    if (!raw) return;
    const saved = JSON.parse(raw) as {
      seeding?: boolean;
      portForward?: boolean;
      proxy?: string;
      encryption?: Encryption;
      route?: unknown;
      vpnAsk?: boolean;
    };
    if (typeof saved.seeding === 'boolean') torrentPrefs.seeding = saved.seeding;
    if (typeof saved.portForward === 'boolean') torrentPrefs.portForward = saved.portForward;
    if (typeof saved.proxy === 'string') torrentPrefs.proxy = saved.proxy;
    // Anything unrecognised is the system route — never a guess at an
    // interface, which would be a decision about where traffic goes that
    // nobody made.
    if (routeOk(saved.route)) torrentPrefs.route = saved.route;
    if (typeof saved.vpnAsk === 'boolean') torrentPrefs.vpnAsk = saved.vpnAsk;
    // A value from a build that spelled it differently must not turn encryption
    // off — an unknown one is nothing, and the field keeps its default.
    if (saved.encryption && ENCRYPTION_CHOICES.includes(saved.encryption)) {
      torrentPrefs.encryption = saved.encryption;
    }
  } catch {
    // A corrupted preference must not cost the safe default.
  }
}

function persistPrefs() {
  try {
    localStorage.setItem(
      PREFS_KEY,
      JSON.stringify({
        seeding: torrentPrefs.seeding,
        portForward: torrentPrefs.portForward,
        proxy: torrentPrefs.proxy,
        encryption: torrentPrefs.encryption,
        route: torrentPrefs.route,
        vpnAsk: torrentPrefs.vpnAsk,
      }),
    );
  } catch {
    // not critical
  }
}

/**
 * Returns true when a running session had to be torn down to apply this, which
 * also ended whatever was streaming. Only ever happens on a real change.
 */
export async function setSeeding(on: boolean): Promise<boolean> {
  torrentPrefs.seeding = on;
  persistPrefs();
  return await invoke<boolean>('torrent_set_seeding', { seeding: on }).catch(() => false);
}

/// Same contract as `setSeeding`: both flags are baked into the session when it
/// is built, so changing either costs whatever was streaming.
export async function setPortForward(on: boolean): Promise<boolean> {
  torrentPrefs.portForward = on;
  persistPrefs();
  return await invoke<boolean>('torrent_set_port_forward', { on }).catch(() => false);
}

/// And again for the proxy, which is baked into the session like the other two:
/// switching one off while pieces still arrive straight from the swarm would be
/// a lie about where this machine's traffic is going.
export async function setProxy(url: string): Promise<boolean> {
  torrentPrefs.proxy = url.trim();
  persistPrefs();
  return await invoke<boolean>('torrent_set_proxy', { url: torrentPrefs.proxy }).catch(() => false);
}

/// The fourth of them, and the cheapest to change one's mind about — but still
/// baked into the session, so still worth saying that the torrent stopped.
export async function setEncryption(mode: Encryption): Promise<boolean> {
  torrentPrefs.encryption = mode;
  persistPrefs();
  return await invoke<boolean>('torrent_set_encryption', { mode }).catch(() => false);
}

/**
 * Choose the interface the swarm leaves by, persisted. Same contract as the
 * four above — bound when the session is built, so a real change costs what
 * was streaming. Also ends a one-run choice made in the VPN question, since a
 * setting chosen by hand is the more deliberate of the two.
 */
export async function setRoute(route: Route): Promise<boolean> {
  torrentPrefs.route = route;
  net.runRoute = null;
  persistPrefs();
  return await invoke<boolean>('torrent_set_route', { route }).catch(() => false);
}

export function setVpnAsk(on: boolean) {
  torrentPrefs.vpnAsk = on;
  persistPrefs();
}

/// What the session is built with: a one-run choice from the VPN question
/// first, then the setting.
export function effectiveRoute(): Route {
  return net.runRoute ?? torrentPrefs.route;
}

class NetState {
  /// The interfaces and where the system route goes, as last read.
  view = $state<NetView | null>(null);
  /// "Past the VPN, this time": the question's answer without "remember". Not
  /// persisted — gone at the next launch, which is when the question returns.
  runRoute = $state<Route | null>(null);
  /// The question is up: the tunnel the traffic would take and the way out
  /// past it. Answered through `answerVpnAsk`.
  ask = $state<{ via: string; direct: string } | null>(null);
}

export const net = new NetState();

/// A settings sheet can be closed and reopened inside one read.
const netReads = latest();

export async function refreshNet(): Promise<NetView | null> {
  const run = netReads.begin();
  const view = await invoke<NetView>('net_interfaces').catch(() => null);
  if (!run.stale) net.view = view;
  return view;
}

/// Asked at most once a run: "through the VPN" without "remember" means "not
/// now", and a question repeated at every episode of a season would be a toll.
let askedThisRun = false;
let answer: ((route: Route | null) => void) | null = null;

/**
 * Before a torrent's first session of the run: if the system route is a tunnel
 * and there is a way out past it, ask which one to use.
 *
 * **Asked of the system, not guessed from names**: `net_interfaces` runs the
 * route lookup for a public address (a UDP `connect`, which sends nothing) and
 * reports which interface it picked. So a VPN that is installed but not
 * routing — a mesh network, a split tunnel — asks nothing, and one that took
 * the default route under an unfamiliar name still does.
 */
export async function askAboutVpn() {
  if (askedThisRun || !torrentPrefs.vpnAsk) return;
  if (torrentPrefs.route.kind !== 'auto' || net.runRoute) return;
  askedThisRun = true;
  const view = await refreshNet();
  if (!view?.via_vpn || !view.via || !view.direct) return;
  const label = (name: string) => view.interfaces.find((i) => i.name === name)?.label ?? name;
  const chosen = await new Promise<Route | null>((resolve) => {
    answer = resolve;
    net.ask = { via: label(view.via!), direct: label(view.direct!) };
  });
  if (chosen) net.runRoute = chosen;
}

/**
 * The answer to the VPN question. `bypass` with `remember` becomes the setting;
 * without it, this run only. `vpn` with `remember` stops the question; without
 * it, the question comes back next launch. Closing the dialog is "through the
 * VPN, this time" — the torrent is opened either way, since the question is
 * about how, not whether.
 */
export async function answerVpnAsk(choice: 'bypass' | 'vpn', remember: boolean) {
  const done = answer;
  answer = null;
  net.ask = null;
  if (choice === 'bypass') {
    if (remember) {
      await setRoute({ kind: 'direct' });
      done?.(null);
    } else {
      done?.({ kind: 'direct' });
    }
  } else {
    if (remember) setVpnAsk(false);
    done?.(null);
  }
}

/// Guards a probe that outlives the panel that asked for it — an SSDP search
/// plus a SOAP round trip is seconds, and the settings sheet can be closed and
/// reopened inside one.
const portProbes = latest();

/**
 * Ask the router whether the port is actually forwarded.
 *
 * The switch on its own would be a claim rather than a fact: librqbit's
 * forwarder is a fire-and-forget task that reports to nobody, and a router with
 * UPnP disabled — which is common — swallows the request in silence. So the
 * setting shows what the router itself answers, and "on" and "working" stay
 * separate things on screen.
 */
export async function refreshPortStatus() {
  const run = portProbes.begin();
  torrent.portChecking = torrentPrefs.portForward;
  try {
    const status = await invoke<PortStatus>('torrent_port_status', {
      on: torrentPrefs.portForward,
    });
    if (run.stale) return;
    torrent.portStatus = status;
  } catch {
    if (!run.stale) torrent.portStatus = null;
  } finally {
    if (!run.stale) torrent.portChecking = false;
  }
}
