/**
 * Casting to TV (Google Cast): the frontend half of cast.rs.
 *
 * While a session is active the player window is a remote control: mpv sits
 * paused on the file it already has, the position flows *back* from the TV
 * (polled at 2 Hz; Rust extrapolates between the receiver's 1 Hz reports), and
 * disconnecting hands playback to mpv at the TV's position. None of the local
 * seek machinery applies here — the probe, the exact/keyframe split and the
 * drag settle are all about local decode cost, and the TV has its own.
 *
 * Depends on `player` the way `history` does: reads its mirrors, never the
 * other way round.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { command, getProperty, setProperty } from 'tauri-plugin-libmpv-api';

import { isPrivatePath } from './history.svelte';
import { t } from './i18n.svelte';
import { showOsd } from './osd.svelte';
import { isNetworkSource, player, type Track } from './player.svelte';
import { neighbour, playEntry, playlist, type PlaylistEntry } from './playlist.svelte';
import { parseTorrentUrl } from './source';

export interface CastDevice {
  id: string;
  name: string;
  model: string;
  ip: string;
  port: number;
}

export interface DlnaDevice {
  id: string;
  name: string;
  model: string;
  ip: string;
  control_url: string;
  rendering_url: string | null;
  /// What the renderer said it accepts (`ConnectionManager::GetProtocolInfo`).
  mimes: string[];
}

export type Transport = 'cast' | 'dlna';

/**
 * One television, however many ways there are to reach it.
 *
 * The same set answers on two protocols and **cannot be joined by identifier**
 * — measured on the LG, its Cast id and its renderer's UDN are different UUIDs
 * — so the merge key is the address, which both discoveries report. The
 * remembered profile is keyed differently on purpose: an address is a DHCP
 * lease and would lose the setting silently, so a profile is filed under the
 * most stable id the device has and matched by name+model as a fallback.
 */
export interface TvDevice {
  key: string;
  name: string;
  model: string;
  ip: string;
  cast?: CastDevice;
  dlna?: DlnaDevice;
}

interface CastStatusMsg {
  state: string;
  error: string | null;
  time: number;
  duration: number;
  volume: number;
  muted: boolean;
  volume_known: boolean;
  volume_fixed: boolean;
  fetches: number;
  device: string | null;
}

/// The TV never fetched a byte this long after the LOAD was accepted: the
/// command channel works and the media channel does not, which is the firewall
/// signature (the Defender prompt was declined, or a router isolates clients).
const FETCH_TIMEOUT_MS = 8000;

const POLL_MS = 500;
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
const REBUILD_AFTER_EMPTY_POLLS = 9;

class Cast {
  devices = $state<CastDevice[]>([]);
  dlnaDevices = $state<DlnaDevice[]>([]);
  discovering = $state(false);
  /// What the transport chosen for a device would mean for the file that is
  /// open — computed once when the picker opens, so each row can state its
  /// consequence without every row running an async probe.
  plan = $state<{
    /// **The resolved source, not `player.filePath`.** For a completed torrent
    /// those differ: the player holds a loopback URL and the cast would be
    /// pointed at the file on disk. Reasoning about the URL made
    /// `isNetworkSource` true and auto answered "Chromecast" for a file that
    /// actually goes over DLNA — the row contradicting what the click does.
    src: string;
    container: string;
    verdict: CastVerdict;
    streaming: boolean;
  } | null>(null);
  /// Which transport the live session is using; the controls, the poll and the
  /// disconnect all route on it.
  transport = $state<Transport>('cast');
  /// Bumped when a device profile changes, so rows recompute their line.
  profileRevision = $state(0);
  /// How many times discovery has been rebuilt during this search. The picker
  /// uses it to stop claiming that nothing is there: after a rebuild or two the
  /// honest statement is "still looking", not "not found".
  rebuilds = $state(0);
  /// The second discovery has not finished its first sweep, so a row that shows
  /// only Cast today may still gain DLNA. SSDP is a UDP broadcast answered at
  /// leisure, where mDNS answers at once — without saying so, the gear (and the
  /// consequence line under the name) appears seconds later for no visible
  /// reason, which reads as the panel changing its mind.
  dlnaSweeping = $state(false);

  /// One row per television: the two discoveries merged by address.
  get tvs(): TvDevice[] {
    const byIp = new Map<string, TvDevice>();
    for (const c of this.devices) {
      byIp.set(c.ip, {
        key: deviceKey(c.id, c.name, c.model),
        name: c.name,
        model: c.model,
        ip: c.ip,
        cast: c,
      });
    }
    for (const d of this.dlnaDevices) {
      const existing = byIp.get(d.ip);
      if (existing) {
        existing.dlna = d;
        continue;
      }
      byIp.set(d.ip, {
        key: deviceKey(d.id, d.name, d.model),
        name: d.name,
        model: d.model,
        ip: d.ip,
        dlna: d,
      });
    }
    return [...byIp.values()];
  }

  /// A session exists, from connect until disconnect — the window is a remote.
  active = $state(false);
  deviceName = $state<string | null>(null);
  /// Mirror of cast.rs's state string: connecting | connected | loading |
  /// buffering | playing | paused | ended | stopped | error.
  state = $state('idle');
  time = $state(0);
  duration = $state(0);
  volume = $state(1);
  muted = $state(false);
  /// Whether the receiver has reported a volume at all, and whether it
  /// declared it un-adjustable (`controlType: "fixed"`). Either negative
  /// disables the slider — a control that silently does nothing is worse
  /// than one that says why.
  volumeKnown = $state(false);
  volumeFixed = $state(false);

  get volumeAdjustable(): boolean {
    return this.volumeKnown && !this.volumeFixed;
  }

  get paused(): boolean {
    return this.state === 'paused';
  }
  get busy(): boolean {
    return (
      this.state === 'connecting' ||
      this.state === 'preparing' ||
      this.state === 'loading' ||
      this.state === 'buffering'
    );
  }
  /// The TV owns playback and this window is a remote. False while connecting
  /// or preparing, when local playback deliberately keeps running — the
  /// controls, wheel, keys and seekbar stay local until the handoff moment.
  get remote(): boolean {
    return this.active && this.state !== 'connecting' && this.state !== 'preparing';
  }
}

export const cast = new Cast();

let deviceTimer: ReturnType<typeof setInterval> | null = null;
let pollTimer: ReturnType<typeof setInterval> | null = null;
let loadStartedAt = 0;
let sawPlayback = false;
/// Whether local playback was running when the cast started — what the
/// handback resumes to.
let resumeUnpaused = false;
/// Containers this device took the URI for and then failed to play, within
/// this run. **Not persisted**: a renderer under-declaring its formats is a
/// guess we are allowed to retry next time the app starts, while repeating a
/// failure for every episode of a season is not. It also makes the picker's
/// line tell the truth from the second attempt onwards.
const dlnaRefused = new Set<string>();

function refusedKey(device: TvDevice, container: string): string {
  return `${device.key}|${container}`;
}

/// The device this session belongs to. **The session is to the device, not to
/// one file** — the queue moves under it, so advancing an episode is a re-LOAD
/// rather than a disconnect and a fresh connect.
let currentDevice: TvDevice | null = null;

// ---- Per-device profile ------------------------------------------------------

const PROFILES_KEY = 'frameplayer.tv-devices';

interface DeviceProfile {
  /// 'auto' resolves per file; a pinned transport is the viewer overriding
  /// that, and it survives sessions.
  transport: Transport | 'auto';
  /// Kept so a profile can be found again when the primary id changes — an
  /// LG renderer's UDN looks service-scoped and may not survive a reboot.
  name?: string;
  model?: string;
}

function deviceKey(id: string, name: string, model: string): string {
  return id || `${name}|${model}`;
}

function loadProfiles(): Record<string, DeviceProfile> {
  try {
    return JSON.parse(localStorage.getItem(PROFILES_KEY) ?? '{}');
  } catch {
    return {};
  }
}

export function deviceProfile(device: TvDevice): DeviceProfile {
  const all = loadProfiles();
  const direct = all[device.key];
  if (direct) return direct;
  // The id moved (a new UDN, a device that lost its Cast half): recognise the
  // television by what a person would recognise it by.
  const byName = Object.values(all).find(
    (p) => p.name === device.name && p.model === device.model,
  );
  return byName ?? { transport: 'auto' };
}

export interface CheckLine {
  /// A stable name for the check; the label and the explanation are this
  /// side's, because Rust cannot reach the dictionary.
  id: string;
  state: 'ok' | 'warn' | 'fail' | 'info' | 'timeout';
  /// What only Rust knows: addresses, lists, the device's own error text.
  detail: string;
}

/// The label and, when there is one to give, the sentence that says what the
/// answer means. A check with nothing to explain shows only its data.
export function checkLabel(line: CheckLine): string {
  return t(`diag.${line.id}` as Parameters<typeof t>[0]);
}

export function checkNote(line: CheckLine): string {
  const key = `diag.${line.id}_${line.state}`;
  const note = t(key as Parameters<typeof t>[0]);
  // `t` echoes an unknown key, which is the signal that this state needs no
  // sentence — the data speaks for itself.
  return note === key ? '' : note;
}

/**
 * Everything the player can learn about a device without playing anything.
 *
 * The case it exists for is somebody else's television, where a cast fails and
 * the difference between "the network cannot reach it", "the receiver refused
 * the file" and "this device does not do that at all" is invisible from the
 * outside and decides everything. Each check is one the normal flow already
 * makes; here their answers are written down instead of consumed.
 */
export async function diagnoseDevice(device: TvDevice): Promise<CheckLine[]> {
  const lines = await invoke<CheckLine[]>('cast_diagnose', {
    ip: device.ip,
    castPort: device.cast?.port ?? null,
    // The description URL, not the control URL: the command re-reads the whole
    // description, which is where the model, the services and their own
    // descriptions come from.
    dlnaLocation: device.dlna ? dlnaDescriptionUrl(device.dlna) : null,
  }).catch((e) => [{ id: 'diagnosis', state: 'fail' as const, detail: String(e) }]);
  return lines;
}

function dlnaDescriptionUrl(d: DlnaDevice): string {
  const [origin] = d.control_url.split('/AVTransport/');
  return origin ? `${origin}/` : d.control_url;
}

/// The report as one block of text — what actually reaches a bug report.
export function diagnosisText(device: TvDevice, lines: CheckLine[]): string {
  const head = [
    `Frame Player — device check`,
    `${device.name}${device.model && device.model !== device.name ? ` (${device.model})` : ''}`,
    `${device.ip} · transports: ${[device.cast && 'Chromecast', device.dlna && 'DLNA']
      .filter(Boolean)
      .join(', ')}`,
    '',
  ];
  return [
    ...head,
    ...lines.map((l) => `[${l.state}] ${l.id}${l.detail ? `: ${l.detail}` : ''}`),
  ].join('\n');
}

export function setDeviceTransport(device: TvDevice, transport: Transport | 'auto') {
  const all = loadProfiles();
  all[device.key] = { transport, name: device.name, model: device.model };
  localStorage.setItem(PROFILES_KEY, JSON.stringify(all));
  // The picker reads this through a plain function call, so nudge the store to
  // make the rows re-render with their new consequence line.
  cast.profileRevision++;
}

/// Container MIME as a DLNA renderer's Sink list spells it. Only what the
/// prepare ladder already knows about; anything unlisted is not offered to
/// DLNA, which is the safe direction — the Cast rung still applies.
const DLNA_MIME: Record<string, string[]> = {
  mkv: ['video/x-matroska'],
  mp4: ['video/mp4'],
  m4v: ['video/mp4'],
  mov: ['video/quicktime', 'video/mp4'],
  avi: ['video/avi', 'video/x-msvideo', 'video/x-ms-avi'],
  ts: ['video/mp2t', 'video/mp2ts', 'video/vnd.dlna.mpeg-tts'],
  m2ts: ['video/mp2t', 'video/mp2ts', 'video/vnd.dlna.mpeg-tts'],
  mpg: ['video/mpeg'],
  mpeg: ['video/mpeg'],
  webm: ['video/webm'],
  wmv: ['video/x-ms-wmv'],
};

function extensionOf(path: string): string {
  return path.split(/[?#]/)[0].split('.').pop()?.toLowerCase() ?? '';
}

/// Whether this renderer says it takes the open file as it is.
export function dlnaTakesFile(device: TvDevice, path: string | null): boolean {
  if (!device.dlna || !path) return false;
  const wanted = DLNA_MIME[extensionOf(path)];
  if (!wanted) return false;
  return wanted.some((m) => device.dlna!.mimes.includes(m));
}

export function deviceIsVideoCapable(device: TvDevice): boolean {
  if (device.cast) return true;
  return device.dlna?.mimes.some((m) => m.startsWith('video/')) ?? false;
}

/**
 * Which transport this device will use for the file that is open.
 *
 * Auto prefers DLNA whenever the renderer declares the container, because that
 * path reaches the television's own decoder: no repacking, surround and HEVC
 * intact, and seeking done by the TV itself. Cast is the answer for everything
 * else — a container the renderer does not list, a network source, or a device
 * with no renderer at all.
 */
export function transportFor(device: TvDevice, path: string | null): Transport {
  const pinned = deviceProfile(device).transport;
  if (pinned === 'dlna' && device.dlna) return 'dlna';
  if (pinned === 'cast' && device.cast) return 'cast';
  if (!device.cast) return 'dlna';
  if (!device.dlna) return 'cast';
  if (dlnaRefused.has(refusedKey(device, extensionOf(path ?? '')))) return 'cast';
  return dlnaTakesFile(device, path) && !isNetworkSource(path ?? '') ? 'dlna' : 'cast';
}

/// Whether the viewer's pinned choice cannot be honoured this session — said
/// out loud in the row rather than silently ignored.
export function pinnedUnavailable(device: TvDevice): boolean {
  const pinned = deviceProfile(device).transport;
  return (pinned === 'dlna' && !device.dlna) || (pinned === 'cast' && !device.cast);
}

/**
 * What will happen if this row is clicked, in one short sentence — the thing a
 * viewer actually decides on. It is deliberately about consequences (wait,
 * sound, whether it plays at all) rather than protocol names, which live under
 * the gear for whoever went looking for them.
 */
export function plannedTransport(device: TvDevice): Transport {
  return transportFor(device, cast.plan?.src ?? player.filePath);
}

export function deviceSummary(device: TvDevice): string {
  const path = cast.plan?.src ?? player.filePath;
  if (!deviceIsVideoCapable(device)) return t('cast.sum_audio_only');
  // **A row must not pass judgement on what it does not know yet.** SSDP is
  // still sweeping for the first seconds, so a device whose DLNA half has not
  // arrived is not a device without DLNA — and for a torrent still downloading
  // the difference is the whole answer, which is how a half-downloaded film
  // came to be told to finish first. The gear slot already shows a spinner
  // through this; the line says the same thing in words.
  // ...but only where the missing half could change the answer. A file the
  // Cast rung already plays untouched cannot be improved by finding a renderer,
  // so making the common case wait 4.5 s to say "plays as it is" would be
  // caution costing honesty in the other direction.
  if (cast.dlnaSweeping && !device.dlna) {
    const settled = !cast.plan?.streaming && cast.plan?.verdict.kind === 'direct';
    if (!settled) return t('cast.sum_checking');
  }
  // A torrent that is still downloading has one path and one refusal, and the
  // row must not offer the prepare its verdict would otherwise claim.
  if (cast.plan?.streaming) {
    const ext = cast.plan.container;
    const dlnaTakes = device.dlna?.mimes.some((m) => (DLNA_MIME[ext] ?? []).includes(m)) ?? false;
    // A Chromecast can stream one too, but only a file that needs no repacking
    // — the verdict is already computed for this file, so no second probe.
    const castTakes = !!device.cast && cast.plan.verdict.kind === 'direct';
    return dlnaTakes || castTakes ? t('cast.sum_stream') : t('cast.sum_stream_wait');
  }
  const transport = transportFor(device, path);
  if (transport === 'dlna') {
    return dlnaTakesFile(device, path) ? t('cast.sum_direct') : t('cast.sum_dlna_unlisted');
  }
  const verdict = cast.plan?.verdict;
  if (verdict?.kind === 'refuse') return t('cast.sum_refuse');
  if (castMode() === 'hls') return t('cast.sum_hls');
  return verdict?.kind === 'prepare' ? t('cast.sum_prepare') : t('cast.sum_direct');
}

/// Read once when the picker opens: the rows need the verdict, and it is the
/// same for every row (it is about the file, not the device).
export async function refreshCastPlan() {
  const path = player.filePath;
  if (!path) {
    cast.plan = null;
    return;
  }
  const resolved = await resolveCastSource(path);
  const src = 'src' in resolved ? resolved.src : path;
  cast.plan = {
    src,
    container: extensionOf(src),
    verdict: await castVerdict(src),
    streaming: 'stream' in resolved,
  };
}

export function startCastDiscovery() {
  if (devicetimerRunning()) return;
  cast.discovering = true;
  emptyPolls = 0;
  cast.rebuilds = 0;
  void invoke('cast_discover_start').catch((e) => {
    console.warn('cast_discover_start failed:', e);
    cast.discovering = false;
  });
  void invoke('dlna_discover_start').catch((e) => console.warn('dlna_discover_start failed:', e));
  cast.dlnaSweeping = true;
  // Cleared by the first sweep that finds anything, or by this deadline — a
  // spinner that never stops is worse than one that gives up.
  setTimeout(() => (cast.dlnaSweeping = false), DLNA_SWEEP_MS);
  void refreshCastPlan();
  const pull = async () => {
    const [castList, dlnaList] = await Promise.all([
      invoke<CastDevice[]>('cast_devices').catch(() => []),
      invoke<DlnaDevice[]>('dlna_devices').catch(() => []),
    ]);
    cast.devices = castList;
    cast.dlnaDevices = dlnaList;
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

/// Tear both discoveries down and start them again. Cheap: one mDNS daemon and
/// a handful of UDP sockets.
async function rebuildDiscovery() {
  emptyPolls = 0;
  cast.rebuilds++;
  await Promise.all([
    invoke('cast_discover_stop').catch(() => {}),
    invoke('dlna_discover_stop').catch(() => {}),
  ]);
  await Promise.all([
    invoke('cast_discover_start').catch(() => {}),
    invoke('dlna_discover_start').catch(() => {}),
  ]);
  cast.dlnaSweeping = true;
  setTimeout(() => (cast.dlnaSweeping = false), DLNA_SWEEP_MS);
}

let emptyPolls = 0;

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
}

/// Containers the receiver takes as-is over HTTP.
const DIRECT_CONTAINERS = ['mp4', 'm4v', 'webm'];
/// Containers the prepare step can losslessly repack into MP4 — anything whose
/// video/audio pass the codec gates below.
const REMUX_CONTAINERS = ['mkv', 'mov', 'avi', 'ts', 'm2ts', 'mts', 'mpg', 'mpeg', 'vob', 'flv', '3gp'];
/// HEVC/VP9/AV1 pass deliberately: whether the device decodes them depends on
/// its class, we cannot ask from the sender side, and the honest answer is to
/// try and report the TV's own refusal.
const CAST_VIDEO = ['h264', 'hevc', 'vp9', 'av1'];
/// AC-3/E-AC-3 ride along for the same reason (passthrough — measured working
/// on the real chain here). Anything else the platform never decodes — DTS,
/// TrueHD, PCM — becomes the transcode rung, not a refusal.
const CAST_AUDIO_COPY = ['aac', 'mp3', 'opus', 'vorbis', 'flac', 'ac3', 'eac3'];

export type CastVerdict =
  | { kind: 'direct'; transcodeAudio: boolean }
  | { kind: 'prepare'; transcodeAudio: boolean }
  | { kind: 'refuse'; reason: string };

/**
 * Which rung of the compatibility ladder this source takes (casting.md):
 * direct play → remux → remux + audio transcode → refuse naming the offender.
 * Only incompatible *video* refuses now — audio is never a dead end, because
 * the E-AC-3 transcode rung swallows DTS/TrueHD/PCM. Codecs are read from mpv
 * (it is playing this same content); the container comes from the resolved
 * source path.
 */
export async function castVerdict(src: string): Promise<CastVerdict> {
  const ext = src.split('.').pop()?.toLowerCase() ?? '';
  const video = (await getProperty('video-format', 'string').catch(() => null)) ?? '';
  const audio = (await getProperty('audio-codec-name', 'string').catch(() => null)) ?? '';
  if (video && !CAST_VIDEO.includes(video)) {
    return { kind: 'refuse', reason: t('cast.err_format', { what: video }) };
  }
  const audioCopy = !audio || CAST_AUDIO_COPY.includes(audio);
  if (DIRECT_CONTAINERS.includes(ext) && audioCopy) {
    return { kind: 'direct', transcodeAudio: false };
  }
  if (DIRECT_CONTAINERS.includes(ext) || REMUX_CONTAINERS.includes(ext)) {
    return { kind: 'prepare', transcodeAudio: !audioCopy };
  }
  return { kind: 'refuse', reason: t('cast.err_format', { what: ext || '?' }) };
}

/**
 * What actually gets fed to the TV / ffmpeg for the current mpv source.
 *
 * A local path answers itself. A torrent stream resolves to its file on disk —
 * complete files only: the pieces live in sparse files where a hole reads as
 * zeros, so an incomplete file must never reach a decoder directly. Streaming
 * an incomplete torrent is the planned HLS-over-the-swarm rung (casting.md);
 * until it exists the refusal says to wait for the download.
 */
/// What the transports can be pointed at: a file on disk, or a torrent still
/// downloading (which only the DLNA rung can take — see `castOverDlna`).
type CastSource =
  | { src: string }
  | { stream: { infoHash: string; index: number; name: string; ext: string } }
  | { refuse: string };

async function resolveCastSource(path: string): Promise<CastSource> {
  const torrent = parseTorrentUrl(path);
  if (torrent) {
    const local = await invoke<{ path: string; complete: boolean } | null>(
      'torrent_local_path',
      { infoHash: torrent.infoHash, index: torrent.index },
    ).catch(() => null);
    // A complete file is an ordinary file and takes every rung, prepare
    // included; an incomplete one can only be streamed, because nothing can
    // repack bytes that have not arrived.
    if (local?.complete) return { src: local.path };
    const name = decodeURIComponent(path.split('/').pop() ?? 'video');
    return {
      stream: {
        infoHash: torrent.infoHash,
        index: torrent.index,
        name,
        ext: name.split('.').pop()?.toLowerCase() ?? '',
      },
    };
  }
  if (isNetworkSource(path)) return { refuse: t('cast.local_only') };
  return { src: path };
}

// ---- Mode and cache preferences ---------------------------------------------
// localStorage like every player-owned preference: nothing here is an mpv
// option, and the settings tab reads/writes through these helpers.

const MODE_KEY = 'frameplayer.castMode';
const CAP_KEY = 'frameplayer.castCacheCap';
const CAP_CHOICES = [0, 5, 20, 50];

export type CastMode = 'auto' | 'prepare' | 'hls';

/**
 * How a file reaches a **Chromecast**. DLNA is chosen per device in the picker,
 * because that choice depends on the device; this one is now internal.
 *
 * **HLS is no longer offered as a setting**, and that is a conclusion rather
 * than a simplification. It was built to stream an incomplete torrent, and both
 * halves of that job went elsewhere — DLNA carries the release untouched and a
 * direct-play file streams over Cast as it is — while measurement showed HLS
 * itself carries only H.264 with stereo on this receiver. So as a *choice* it
 * was strictly worse on everything it could carry and unable to carry the rest:
 * a menu entry whose every selection makes the result worse. What survives is
 * the code path, for the one case neither of the others reaches (a receiver
 * with no DLNA, an incomplete torrent that would need repacking) — a decision
 * the player makes from facts, not a question for the viewer — and as a
 * debugging knob: `localStorage.setItem('frameplayer.castMode', 'hls')` still
 * forces it, which is how the receiver's HLS behaviour gets tested.
 */
export function castMode(): CastMode {
  const stored = localStorage.getItem(MODE_KEY);
  return stored === 'hls' || stored === 'prepare' ? stored : 'auto';
}

export function setCastMode(mode: CastMode) {
  try {
    localStorage.setItem(MODE_KEY, mode);
  } catch {
    // not critical: the choice simply will not survive a restart
  }
}

/// Cap in whole gigabytes; 0 means "keep nothing past the session".
export function castCacheCapGb(): number {
  const value = Number(localStorage.getItem(CAP_KEY) ?? '20');
  return CAP_CHOICES.includes(value) ? value : 20;
}

export function setCastCacheCapGb(gb: number) {
  try {
    localStorage.setItem(CAP_KEY, String(gb));
  } catch {
    // not critical
  }
}

const PREPARE_EVENT = 'frameplayer://cast-prepare';

/// The source the running cast was started from — what a mid-session audio
/// switch re-prepares. The *served* path may be a prepared copy; this is the
/// original (for a torrent: its resolved file on disk).
let castSrcPath: string | null = null;
/// The session's transport: null = progressive MP4, else the HLS segment
/// format ('ts' | 'fmp4'). Fixed at cast start from the mode setting; the
/// audio switch follows it.
let sessionHls: 'ts' | 'fmp4' | null = null;
/// The prepared copy served instead of the original this session, and whether
/// its source is under a private root — a prepared copy of a private file must
/// not outlive the session (casting.md, privacy).
let preparedPath: string | null = null;
let preparedHidden = false;

/**
 * Run the prepare rung with its progress on the sticky OSD.
 *
 * The announcement obeys the slow-operation rule: the popup is up before
 * ffmpeg starts, carries the real figure (ffmpeg's own out_time against the
 * known duration), and every exit path of the caller replaces it.
 */
async function prepareForCast(
  path: string,
  transcodeAudio: boolean,
  audioIndex: number,
  channels: number,
): Promise<string> {
  // A variant already in the cache needs no popup at all: "preparing 0%" over
  // work that takes no time reads as a glitch, and anything that delays its
  // replacement makes it read as a hang.
  const cached = await invoke<string | null>('cast_prepare_cached', {
    path,
    audioIndex,
    transcodeAudio,
  }).catch(() => null);
  if (cached) return cached;

  const video = (await getProperty('video-format', 'string').catch(() => null)) ?? '';
  showOsd(t('cast.preparing', { pct: 0 }), { sticky: true, progress: 0 });
  // `live` guards against a progress event already dispatched into the JS
  // queue landing AFTER the caller's final popup: a resurrected sticky has no
  // owner left to replace it, which is a popup that hangs for good.
  let live = true;
  const unlisten = await listen<number>(PREPARE_EVENT, (event) => {
    if (!live) return;
    const frac = Math.max(0, Math.min(1, event.payload));
    showOsd(t('cast.preparing', { pct: Math.round(frac * 100) }), {
      sticky: true,
      progress: frac,
    });
  });
  try {
    return await invoke<string>('cast_prepare', {
      path,
      audioIndex,
      transcodeAudio,
      channels,
      hevcTag: video === 'hevc',
      duration: player.duration,
      capBytes: castCacheCapGb() * 1024 ** 3,
    });
  } finally {
    live = false;
    unlisten();
  }
}

/**
 * The HLS twin of `prepareForCast`: a full VOD rendition (playlist + segments)
 * in a transient session directory. No cache probe — HLS sessions are
 * regenerated each time by contract, which is the mode's storage story.
 */
async function prepareHls(
  path: string,
  transcodeAudio: boolean,
  audioIndex: number,
  channels: number,
  fmp4: boolean,
): Promise<string> {
  showOsd(t('cast.preparing', { pct: 0 }), { sticky: true, progress: 0 });
  let live = true;
  const unlisten = await listen<number>(PREPARE_EVENT, (event) => {
    if (!live) return;
    const frac = Math.max(0, Math.min(1, event.payload));
    showOsd(t('cast.preparing', { pct: Math.round(frac * 100) }), {
      sticky: true,
      progress: frac,
    });
  });
  try {
    return await invoke<string>('cast_hls_prepare', {
      path,
      audioIndex,
      transcodeAudio,
      channels,
      fmp4,
      duration: player.duration,
    });
  } finally {
    live = false;
    unlisten();
  }
}

/// The selected audio track as the prepare rung needs it: its index among the
/// audio streams and its channel count (falling back to the live decoder's).
async function selectedAudioParams(): Promise<{ audioIndex: number; channels: number }> {
  const selected = player.audioTracks.find((track) => track.selected) ?? null;
  const audioIndex = selected ? Math.max(0, player.audioTracks.indexOf(selected)) : 0;
  const channels =
    selected?.channels ??
    (await getProperty('audio-params/channel-count', 'int64').catch(() => null)) ??
    6;
  return { audioIndex, channels };
}

/**
 * Connect to a device and hand it the current file.
 *
 * Slow-operation rule: the sticky "connecting" popup goes up before anything
 * starts, and every exit path below replaces it — success (the poll's first
 * "playing"), each distinct failure, and the firewall timeout.
 */
export async function castCurrentFile(
  device: TvDevice,
  opts: { keepSession?: boolean; forceTransport?: Transport } = {},
): Promise<boolean> {
  const keep = opts.keepSession === true;
  // A fresh session may fall back again; a re-load inside one may not, or a
  // transport that keeps refusing would ping-pong.
  if (!keep && !opts.forceTransport) fellBack = false;
  currentDevice = device;
  const path = player.filePath;
  if (!path) return false;
  const resolved = await resolveCastSource(path);
  if ('refuse' in resolved) {
    showOsd(resolved.refuse);
    return false;
  }
  // A torrent that is still downloading has exactly one path to a television,
  // and it is not a choice: nothing can be prepared from bytes that have not
  // arrived, and the segmenter would cost the release its codecs even if it
  // could. So it is DLNA or an honest refusal.
  if ('stream' in resolved) return castTorrentStream(device, resolved.stream, keep);
  const src = resolved.src;
  const transport = opts.forceTransport ?? transportFor(device, src);
  if (transport === 'dlna') return castOverDlna(device, src, keep);

  const verdict = await castVerdict(src);
  if (verdict.kind === 'refuse') {
    showOsd(verdict.reason);
    return false;
  }

  if (!keep) {
    showOsd(t('cast.connecting', { name: device.name }), { sticky: true });
    try {
      await invoke('cast_connect', { device: device.cast });
    } catch (e) {
      console.warn('cast_connect failed:', e);
      showOsd(t('cast.err_unreachable'));
      return false;
    }
    // Only on a fresh session: following the queue happens with the local
    // player already paused, so reading it here would record "was paused" and
    // the handback at the end would leave the film standing still.
    resumeUnpaused = !player.paused;
  }
  cast.transport = 'cast';

  cast.active = true;
  cast.deviceName = device.name;
  cast.state = keep ? 'loading' : 'connecting';
  cast.time = player.timePos;
  cast.duration = player.duration;
  sawPlayback = false;

  // The prepare rung runs while local playback continues — the wait is
  // seconds-to-half-a-minute and there is no reason to sit on a frozen frame
  // through it. The casting screen stays down during 'preparing' for the same
  // reason (the page gates on it).
  castSrcPath = src;
  const hidden = isPrivatePath(src);
  const mode = castMode();
  // In HLS mode everything goes through the segmenter, direct-play files
  // included — the mode is the transport, and half its purpose today is
  // letting the receiver's HLS behaviour be tested with known-good files.
  sessionHls = mode === 'hls' ? await hlsVariant() : null;
  let castPath = src;
  if (sessionHls || verdict.kind === 'prepare') {
    cast.state = 'preparing';
    try {
      const { audioIndex, channels } = await selectedAudioParams();
      castPath = sessionHls
        ? await prepareHls(src, verdict.transcodeAudio, audioIndex, channels, sessionHls === 'fmp4')
        : await prepareForCast(src, verdict.transcodeAudio, audioIndex, channels);
    } catch (e) {
      console.warn('cast prepare failed:', e);
      // Cancelled by an endCast from elsewhere — that path already spoke.
      if (cast.active) await endCast({ osd: t('cast.prepare_failed'), resumeLocal: false });
      return false;
    }
    if (!cast.active) return false;
    if (!sessionHls) {
      preparedPath = castPath;
      preparedHidden = hidden;
    }
  }

  // The remote takes over: local playback stands down but keeps the file, so
  // tracks, chapters, duration and the storyboard keep feeding the UI and the
  // handback is a seek rather than a reopen.
  await setProperty('pause', true).catch(() => {});
  cast.state = 'loading';
  cast.time = player.timePos;
  loadStartedAt = Date.now();

  try {
    await invoke('cast_load', {
      path: castPath,
      position: player.timePos,
      title: hidden ? null : player.displayTitle,
      hidden,
      hls: sessionHls,
    });
  } catch (e) {
    console.warn('cast_load failed:', e);
    await endCast({ osd: t('cast.err_load'), resumeLocal: true });
    return false;
  }

  startPoll();
  return true;
}

/// Bytes of lead the television gets before it is told to start.
///
/// Not a guess about the swarm but about the receiver: it has its own HTTP
/// timeouts, and a read that blocks for a minute on a dry swarm may end the
/// media session rather than show "buffering" — the one thing about this rung
/// that cannot be known from the sender side. Small enough not to be a second
/// wait on a healthy swarm, big enough that the renderer's opening probes (the
/// header, and for MKV the cues at the *end*) do not all land on missing
/// pieces at once.
const STREAM_LEAD_BYTES = 24 * 1024 * 1024;
/// Give up leading and hand it over anyway: a slow swarm is the viewer's to
/// judge, and the readout under the top bar already says peers and rate.
const STREAM_LEAD_TIMEOUT_MS = 45_000;

/**
 * Cast a torrent that is still downloading.
 *
 * The bytes come from librqbit's blocking stream — the same mechanism that
 * feeds mpv, so the television's Range requests become piece priority — routed
 * through the cast server's one-source-behind-a-token rule rather than the
 * loopback torrent server, which a TV cannot reach and which must not be put on
 * the LAN (it would publish every torrent in the session under a guessable
 * path). Reading the sparse file off disk is not an alternative: a stretch that
 * has not arrived reads back as zeros, and the TV would play them silently.
 */
async function castTorrentStream(
  device: TvDevice,
  stream: { infoHash: string; index: number; name: string; ext: string },
  keep = false,
): Promise<boolean> {
  // Two ways in, and neither is a preference. A renderer that lists the
  // container takes the release untouched — the wide door, since torrents are
  // MKV. A Chromecast takes it only if the file needs no repacking at all,
  // because there is nothing to repack: half a film remuxed is half a film.
  const renderer = device.dlna;
  const dlnaTakes =
    !!renderer && (DLNA_MIME[stream.ext] ?? []).some((m) => renderer.mimes.includes(m));
  const castTakes =
    !dlnaTakes && !!device.cast && (await castVerdict(stream.name)).kind === 'direct';
  if (!dlnaTakes && !castTakes) {
    showOsd(renderer ? t('cast.stream_format', { what: stream.ext || '?' }) : t('cast.stream_needs_dlna'));
    return false;
  }
  const deviceIp = dlnaTakes ? renderer!.ip : device.cast!.ip;

  // The lead, announced: this is a wait the viewer is owed an explanation for,
  // and the figure moves, so it is a sticky popup replaced on every exit path.
  showOsd(t('cast.stream_buffering', { percent: 0 }), { sticky: true });
  const until = Date.now() + STREAM_LEAD_TIMEOUT_MS;
  let fileSize = 0;
  for (;;) {
    const status = await invoke<{ file_done: number; file_size: number }>('torrent_status', {
      infoHash: stream.infoHash,
      index: stream.index,
    }).catch(() => null);
    const done = status?.file_done ?? 0;
    if (status?.file_size) fileSize = status.file_size;
    const target = Math.min(STREAM_LEAD_BYTES, status?.file_size ?? STREAM_LEAD_BYTES);
    if (done >= target || Date.now() > until) break;
    showOsd(t('cast.stream_buffering', { percent: Math.round((done / target) * 100) }), {
      sticky: true,
    });
    await new Promise((r) => setTimeout(r, 500));
  }

  if (!keep) {
    showOsd(t('cast.connecting', { name: device.name }), { sticky: true });
    try {
      if (dlnaTakes) {
        await invoke('dlna_connect', { device: renderer });
      } else {
        await invoke('cast_connect', { device: device.cast });
      }
    } catch (e) {
      console.warn('stream connect failed:', e);
      showOsd(t('cast.err_unreachable'));
      return false;
    }
    resumeUnpaused = !player.paused;
  }
  const hidden = isPrivatePath(player.filePath ?? '');
  let url: string;
  try {
    url = await invoke<string>('cast_serve_torrent', {
      infoHash: stream.infoHash,
      index: stream.index,
      name: stream.name,
      deviceIp,
      hidden,
    });
  } catch (e) {
    console.warn('cast_serve_torrent failed:', e);
    showOsd(t('cast.err_load'));
    return false;
  }

  cast.transport = dlnaTakes ? 'dlna' : 'cast';
  cast.active = true;
  cast.deviceName = device.name;
  cast.state = 'loading';
  cast.time = player.timePos;
  cast.duration = player.duration;
  sawPlayback = false;
  castSrcPath = null;
  sessionHls = null;

  await setProperty('pause', true).catch(() => {});
  loadStartedAt = Date.now();
  try {
    if (dlnaTakes) {
      await invoke('dlna_load_url', {
        url,
        position: player.timePos,
        title: hidden ? null : player.displayTitle,
        duration: player.duration,
        mime: DLNA_MIME[stream.ext]?.[0] ?? 'video/mp4',
        // The renderer decides seekability from the metadata before it fetches
        // a byte, and size is one of the three things it reads.
        size: fileSize,
      });
    } else {
      await invoke('cast_load_url', {
        url,
        name: stream.name,
        position: player.timePos,
        title: hidden ? null : player.displayTitle,
      });
    }
  } catch (e) {
    console.warn('stream load failed:', e);
    await endCast({ osd: t('cast.err_load'), resumeLocal: true });
    return false;
  }
  startPoll();
  return true;
}

/**
 * Cast over DLNA: hand the renderer the file itself.
 *
 * There is no ladder here and no prepare step — that machinery exists because
 * the Cast receiver decodes in a browser pipeline, while a DLNA renderer *is*
 * the television's own player. Measured on the real set: an untouched 4K HEVC
 * Main-10 HDR10 MKV with E-AC-3 5.1 plays, seeks (the TV reads the MKV's own
 * cues and range-requests the offset itself) and keeps its surround.
 */
async function castOverDlna(device: TvDevice, src: string, keep = false): Promise<boolean> {
  const renderer = device.dlna;
  if (!renderer) return false;
  if (isNetworkSource(src)) {
    showOsd(t('cast.local_only'));
    return false;
  }

  if (!keep) {
    showOsd(t('cast.connecting', { name: device.name }), { sticky: true });
    try {
      await invoke('dlna_connect', { device: renderer });
    } catch (e) {
      console.warn('dlna_connect failed:', e);
      showOsd(t('cast.err_unreachable'));
      return false;
    }
    resumeUnpaused = !player.paused;
  }

  cast.transport = 'dlna';
  cast.active = true;
  cast.deviceName = device.name;
  cast.state = 'loading';
  cast.time = player.timePos;
  cast.duration = player.duration;
  sawPlayback = false;
  castSrcPath = src;
  sessionHls = null;

  await setProperty('pause', true).catch(() => {});
  loadStartedAt = Date.now();
  const hidden = isPrivatePath(src);
  try {
    await invoke('dlna_load', {
      path: src,
      position: player.timePos,
      title: hidden ? null : player.displayTitle,
      hidden,
    });
  } catch (e) {
    console.warn('dlna_load failed:', e);
    await endCast({ osd: t('cast.err_load'), resumeLocal: true });
    return false;
  }
  startPoll();
  return true;
}

/// Which HLS rendition to build — or `null` for "this file cannot go over HLS
/// at all", which sends it down the progressive path instead.
///
/// **HLS is H.264 only on the receiver this was measured against.** Measured against a real television with a
/// rendition per cell: H.264 plays in TS and (once the LOAD stopped carrying
/// `hlsSegmentFormat`) in fMP4, while HEVC is refused in fMP4 — with the tag
/// corrected to `hvc1`, and refused a beat earlier when a master playlist
/// declares `CODECS="hvc1…"`, i.e. it is the codec being turned down and not
/// the packaging. The same TV plays 4K HEVC HDR perfectly over progressive
/// MP4, because that path reaches its own decoder rather than the receiver's
/// browser pipeline. Carrying HEVC over HLS would need a video transcode rung,
/// which is out of scope; so in HLS mode a HEVC file quietly gets the
/// progressive rung, which for a local file is the better answer anyway.
///
/// TS rather than fMP4 for the H.264 case: both play, and TS is the format the
/// Default Media Receiver has always taken.
async function hlsVariant(): Promise<'ts' | 'fmp4' | null> {
  const video = (await getProperty('video-format', 'string').catch(() => null)) ?? '';
  return video === 'h264' ? 'ts' : null;
}

function startPoll() {
  stopPoll();
  pollTimer = setInterval(() => void poll(), POLL_MS);
}

function stopPoll() {
  if (pollTimer !== null) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
}

async function poll() {
  // A handover takes a second or two, and the television keeps reporting the
  // finished file as ended throughout — without this the queue would jump
  // several episodes at once.
  if (!cast.active || advancing) return;
  const status = await invoke<CastStatusMsg>(
    cast.transport === 'dlna' ? 'dlna_status' : 'cast_status',
  ).catch(() => null);
  if (!status || !cast.active) return;

  cast.state = status.state;
  cast.time = status.time;
  if (status.duration > 0) cast.duration = status.duration;
  cast.volume = status.volume;
  cast.muted = status.muted;
  cast.volumeKnown = status.volume_known;
  cast.volumeFixed = status.volume_fixed;
  if (status.device) cast.deviceName = status.device;

  switch (status.state) {
    case 'playing':
    case 'paused':
      if (!sawPlayback) {
        sawPlayback = true;
        // Replaces the sticky "connecting" popup — the success exit path.
        showOsd(t('cast.casting_on', { name: cast.deviceName ?? '' }));
      }
      break;
    case 'ended':
      // A DLNA renderer that stops before it ever played did not finish the
      // film — it failed to start it, and "ended" is simply the only state it
      // has for both.
      if (cast.transport === 'dlna' && !sawPlayback && (await fallbackToCast('refused'))) return;
      // The queue is the player's, not the television's: mpv sits paused on
      // the file it handed over, so nothing advances unless we advance it.
      if (await castAdvance(1, { auto: true })) return;
      await endCast({ osd: t('cast.ended'), resumeLocal: true, resumePaused: true });
      return;
    case 'stopped':
      if (cast.transport === 'dlna' && !sawPlayback && (await fallbackToCast('refused'))) return;
      // Stopped from the TV side (or another sender took the device over).
      await endCast({ osd: t('cast.stopped'), resumeLocal: true, resumePaused: true });
      return;
    case 'error': {
      if (cast.transport === 'dlna' && !sawPlayback && (await fallbackToCast('refused'))) return;
      const kind = status.error ?? 'closed';
      const key =
        kind === 'unreachable' ? 'cast.err_unreachable'
        : kind === 'launch_failed' ? 'cast.err_launch'
        : kind === 'load_failed' ? 'cast.err_load'
        : 'cast.err_closed';
      await endCast({ osd: t(key), resumeLocal: true });
      return;
    }
    default:
      break;
  }

  // The firewall verdict: the TV accepted the LOAD (or is still claiming to
  // buffer) but never opened a connection to our server. A TV that IS fetching
  // and still buffering is a different situation and stays untouched. Note the
  // fallback is deliberately NOT taken here: nothing was fetched, so the file
  // is not the problem and Chromecast would be served by the same blocked
  // server.
  if (!sawPlayback && status.fetches === 0 && Date.now() - loadStartedAt > FETCH_TIMEOUT_MS) {
    await endCast({ osd: t('cast.err_firewall'), resumeLocal: true });
    return;
  }
  // **Fetched and still not playing**: the device is reading the file and
  // showing nothing, which is the failure that used to be perfectly silent —
  // no error frame, no state change, a black screen and a player convinced all
  // is well. Both transports can end up here, and neither reports it.
  if (!sawPlayback && status.fetches > 0 && Date.now() - loadStartedAt > START_TIMEOUT_MS) {
    if (cast.transport === 'dlna' && (await fallbackToCast('silent'))) return;
    await endCast({ osd: t('cast.stuck'), resumeLocal: true });
  }
}

/**
 * End the session and hand playback back to mpv.
 *
 * Every path through here raises a popup — this is what closes the sticky
 * "connecting" one on failures, per the OSD contract.
 */
export async function endCast(opts: {
  osd: string;
  resumeLocal: boolean;
  resumePaused?: boolean;
}): Promise<void> {
  stopPoll();
  const wasActive = cast.active;
  cast.active = false;
  cast.state = 'idle';
  cast.deviceName = null;
  // A prepare still running dies with the session; harmless when none is.
  void invoke('cast_prepare_cancel').catch(() => {});
  const lastTime = await invoke<number>(
    cast.transport === 'dlna' ? 'dlna_disconnect' : 'cast_disconnect',
  ).catch(() => 0);
  // The prepared copy of a private file must not outlive its session, and
  // with the cache set to "keep nothing" neither does anyone else's; a public
  // one otherwise stays cached so re-casting it is instant. (HLS sessions are
  // cleaned Rust-side with the server.)
  if (preparedPath && (preparedHidden || castCacheCapGb() === 0)) {
    void invoke('cast_forget_prepared', { path: preparedPath }).catch(() => {});
  }
  preparedPath = null;
  preparedHidden = false;
  castSrcPath = null;
  sessionHls = null;
  currentDevice = null;
  cast.transport = 'cast';
  showOsd(opts.osd);
  if (!wasActive || !opts.resumeLocal || !player.hasFile) return;

  const target = lastTime > 0 ? lastTime : cast.time;
  if (target > 0 && Number.isFinite(target)) {
    await command('seek', [target, 'absolute']).catch(() => {});
  }
  const unpause = resumeUnpaused && !opts.resumePaused;
  if (unpause) {
    await setProperty('pause', false).catch(() => {});
  }
}

/** The user's own "continue here" action. */
export async function disconnectCast(): Promise<void> {
  await endCast({ osd: t('cast.stopped'), resumeLocal: true });
}

/**
 * Move the session to the neighbouring queue entry and keep casting.
 *
 * The local player opens the file first and stays paused on it — the same
 * arrangement as the initial cast, and the reason the UI keeps working: tracks,
 * chapters, duration, the storyboard and the handback all read mpv, which has
 * to be on the file the television is playing. Only then is the new source
 * resolved and handed over, on the **live** session: a disconnect and a fresh
 * connect between episodes would blank the TV and re-launch the receiver.
 *
 * `auto` is the end-of-file caller, which respects the queue's auto-advance
 * preference; a button press does not.
 */
export async function castAdvance(
  offset: number,
  opts: { auto?: boolean } = {},
): Promise<boolean> {
  if (opts.auto && !playlist.autoAdvance) return false;
  const entry = neighbour(offset);
  // `neighbour` wraps under repeat-all, which on a one-entry queue answers with
  // the file that just finished; re-casting it is a legitimate repeat, so only
  // an empty answer stops us.
  if (!entry) return false;
  return castFollow(entry);
}

/// Hand a specific queue entry to the live session — the queue panel's click,
/// and what `castAdvance` resolves to.
export async function castFollow(entry: PlaylistEntry): Promise<boolean> {
  const device = currentDevice;
  if (!device || !cast.active) return false;

  const before = player.filePath;
  advancing = true;
  try {
    await playEntry(entry);
    // Wait for mpv to actually be on the new file: everything below reads its
    // properties, and reading them a beat early describes the previous episode.
    const deadline = Date.now() + FOLLOW_TIMEOUT_MS;
    while (Date.now() < deadline) {
      if (player.filePath && player.filePath !== before && player.duration > 0) break;
      await new Promise((r) => setTimeout(r, 60));
    }
    await setProperty('pause', true).catch(() => {});
    return await castCurrentFile(device, { keepSession: true });
  } finally {
    advancing = false;
  }
}

/// Guard against the poll starting a second advance while one is in flight —
/// the TV keeps reporting the old session as ended for the second or two the
/// handover takes.
let advancing = false;

/// True while the session is moving itself to another queue entry.
///
/// The page ends a cast whenever a file is opened, because two playbacks at
/// once is never meant — and following the queue opens a file, so without this
/// the advance would kill the very session it is trying to carry forward.
export function castFollowing(): boolean {
  return advancing;
}

/// How long to wait for mpv to land on the next entry before handing whatever
/// it has to the television.
const FOLLOW_TIMEOUT_MS = 8000;

/**
 * The renderer took the file and could not play it — go round by the Cast
 * ladder instead of leaving a black screen.
 *
 * This is the answer to the three ways `auto` can be wrong about DLNA, all of
 * which look the same from here: the container is listed but the codec inside
 * is not decodable, the device advertises less than it can and was picked for
 * the wrong reason, or a renderer-only device simply cannot take this file.
 * Rather than reason about which, the failure itself is the evidence — the
 * container is remembered as refused for this device *for this run*, so the
 * rest of the season does not repeat the attempt, and the ladder that names
 * its own refusals takes over.
 */
async function fallbackToCast(reason: 'refused' | 'silent'): Promise<boolean> {
  const device = currentDevice;
  const src = castSrcPath;
  if (!device?.cast || !src || fellBack) return false;
  fellBack = true;
  dlnaRefused.add(refusedKey(device, extensionOf(src)));
  console.warn(`cast: DLNA ${reason}, falling back to Chromecast`);

  stopPoll();
  cast.active = false;
  await invoke('dlna_disconnect').catch(() => {});
  showOsd(t(reason === 'silent' ? 'cast.dlna_silent' : 'cast.dlna_refused'), { sticky: true });
  const ok = await castCurrentFile(device, { forceTransport: 'cast' });
  if (!ok) showOsd(t('cast.dlna_refused_failed'));
  return true;
}

/// One fallback per session: if the Cast ladder fails too, the viewer gets its
/// reason rather than a loop between two transports.
let fellBack = false;

/// A session that has not reported playback by now is not starting. Renderers
/// answer `TRANSITIONING` for a few polls on a good load and a Cast receiver
/// buffers, so this sits comfortably past both — it is the guard against the
/// failure with no error at all: the file accepted, fetched, and nothing on
/// screen.
const START_TIMEOUT_MS = 20_000;

// ---- Remote controls --------------------------------------------------------

/// Both transports answer the same four actions under different command names,
/// which is what lets one set of controls, one seekbar and one casting screen
/// serve either.
function controlCommand(): string {
  return cast.transport === 'dlna' ? 'dlna_control' : 'cast_control';
}

/// A refused command used to vanish into an empty catch, which is how a control
/// that does nothing looks exactly like a control that worked. A renderer
/// answers refusals in earnest (UPnP 606 "not authorized", 701 "transition not
/// available"), and the viewer is owed the difference.
function reportControlFailure(e: unknown) {
  console.warn('cast control failed:', e);
  showOsd(t('cast.command_failed'));
}

export function castTogglePause() {
  void invoke(controlCommand(), { action: cast.paused ? 'play' : 'pause', value: null }).catch(
    reportControlFailure,
  );
  // Optimistic, like every local control: the next status report is up to
  // half a second away and a button that lags reads as broken.
  cast.state = cast.paused ? 'playing' : 'paused';
}

export function castSeek(time: number) {
  const clamped = Math.max(0, Math.min(cast.duration || time, time));
  cast.time = clamped;
  void invoke(controlCommand(), { action: 'seek', value: clamped }).catch(reportControlFailure);
}

export function castSeekBy(delta: number) {
  castSeek(cast.time + delta);
}

/// TV volume is the receiver's own 0..1. Silent — the volume bar shows itself.
export function castSetVolume(frac: number) {
  if (!cast.volumeAdjustable) return;
  const next = Math.max(0, Math.min(1, frac));
  cast.volume = next;
  void invoke(controlCommand(), { action: 'volume', value: next }).catch(() => {});
}

/// The wheel/keys path: same value, plus the OSD (shown as percent). A device
/// whose volume is fixed (or never reported) gets the explanation instead of a
/// control that silently does nothing.
export function castNudgeVolume(delta: number) {
  if (!cast.volumeAdjustable) {
    showOsd(t('cast.volume_fixed'));
    return;
  }
  castSetVolume(cast.volume + delta);
  showOsd(t('osd.volume', { value: Math.round(cast.volume * 100) }), { progress: cast.volume });
}

/**
 * Switch the TV to another audio track mid-session.
 *
 * The prepared file carries exactly one audio track, so this is a re-prepare
 * from the original source with the new track (instant when that variant is
 * already cached) and a LOAD at the TV's current position. `cast.state` is
 * deliberately left alone: the TV keeps playing the old track while ffmpeg
 * runs, and flipping to 'preparing' would hand the whole UI back to the paused
 * local player mid-cast.
 */
export async function castSwitchAudio(track: Track): Promise<void> {
  const src = castSrcPath;
  if (!cast.remote || !src) return;
  // Over DLNA the renderer got the original file with every track in it, so the
  // choice is the television's own and ours to stay out of — re-sending the
  // file to change it would restart playback for nothing.
  if (cast.transport === 'dlna') {
    showOsd(t('cast.track_on_tv'));
    return;
  }
  const codec = track.codec?.toLowerCase() ?? null;
  const transcode = !(codec && CAST_AUDIO_COPY.includes(codec));
  const audioIndex = Math.max(0, player.audioTracks.indexOf(track));
  const channels = track.channels ?? 6;
  const hidden = isPrivatePath(src);
  const previous = preparedPath;
  try {
    // The switch follows the session's transport: an HLS session gets a fresh
    // rendition (the displaced session dir is cleaned by cast_load Rust-side),
    // a progressive one re-prepares the MP4 (cached per track).
    const prepared = sessionHls
      ? await prepareHls(src, transcode, audioIndex, channels, sessionHls === 'fmp4')
      : await prepareForCast(src, transcode, audioIndex, channels);
    if (!cast.active) return;
    await invoke('cast_load', {
      path: prepared,
      position: cast.time,
      title: hidden ? null : player.displayTitle,
      hidden,
      hls: sessionHls,
    });
    if (!sessionHls) {
      preparedPath = prepared;
      preparedHidden = hidden;
      // The variant just replaced must not linger for a private file; a public
      // one stays cached for switching back.
      if (hidden && previous && previous !== prepared) {
        void invoke('cast_forget_prepared', { path: previous }).catch(() => {});
      }
    }
    showOsd(track.label);
  } catch (e) {
    console.warn('castSwitchAudio failed:', e);
    // The session survives on the old track; the popup must still be replaced.
    if (cast.active) showOsd(t('cast.prepare_failed'));
  }
}

export function castToggleMute() {
  // Mute is part of the volume control, not a separate capability: a receiver
  // that declares its volume fixed (or, over DLNA, never reports a usable one)
  // ignores this too, and the icon would flip and flip back on its own. The
  // button is disabled for the same reason the slider is; this is the keyboard
  // path, which gets the explanation instead.
  if (!cast.volumeAdjustable) {
    showOsd(t('cast.volume_fixed'));
    return;
  }
  const next = !cast.muted;
  cast.muted = next;
  void invoke(controlCommand(), { action: 'mute', value: next ? 1 : 0 }).catch(() => {});
  showOsd(t(next ? 'osd.sound_off' : 'osd.sound_on'));
}
