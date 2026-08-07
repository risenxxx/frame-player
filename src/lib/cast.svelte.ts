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
import { parseTorrentUrl } from './source';

export interface CastDevice {
  id: string;
  name: string;
  model: string;
  ip: string;
  port: number;
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

class Cast {
  devices = $state<CastDevice[]>([]);
  discovering = $state(false);
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

export function startCastDiscovery() {
  if (devicetimerRunning()) return;
  cast.discovering = true;
  void invoke('cast_discover_start').catch((e) => {
    console.warn('cast_discover_start failed:', e);
    cast.discovering = false;
  });
  const pull = async () => {
    const list = await invoke<CastDevice[]>('cast_devices').catch(() => []);
    cast.devices = list;
  };
  void pull();
  deviceTimer = setInterval(() => void pull(), DEVICE_POLL_MS);
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
  void invoke('cast_discover_stop').catch(() => {});
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
async function resolveCastSource(
  path: string,
): Promise<{ src: string } | { refuse: string }> {
  const torrent = parseTorrentUrl(path);
  if (torrent) {
    const local = await invoke<{ path: string; complete: boolean } | null>(
      'torrent_local_path',
      { infoHash: torrent.infoHash, index: torrent.index },
    ).catch(() => null);
    if (local?.complete) return { src: local.path };
    return { refuse: t('cast.torrent_incomplete') };
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

export type CastMode = 'prepare' | 'hls';

export function castMode(): CastMode {
  return localStorage.getItem(MODE_KEY) === 'hls' ? 'hls' : 'prepare';
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
export async function castCurrentFile(device: CastDevice): Promise<boolean> {
  const path = player.filePath;
  if (!path) return false;
  const resolved = await resolveCastSource(path);
  if ('refuse' in resolved) {
    showOsd(resolved.refuse);
    return false;
  }
  const src = resolved.src;
  const verdict = await castVerdict(src);
  if (verdict.kind === 'refuse') {
    showOsd(verdict.reason);
    return false;
  }

  showOsd(t('cast.connecting', { name: device.name }), { sticky: true });
  try {
    await invoke('cast_connect', { device });
  } catch (e) {
    console.warn('cast_connect failed:', e);
    showOsd(t('cast.err_unreachable'));
    return false;
  }

  cast.active = true;
  cast.deviceName = device.name;
  cast.state = 'connecting';
  cast.time = player.timePos;
  cast.duration = player.duration;
  sawPlayback = false;
  resumeUnpaused = !player.paused;

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
  sessionHls = mode === 'hls' ? (await hlsVariant()) : null;
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

/// TS segments for H.264 (the receiver's zero-surprise default), fMP4 for HEVC
/// (out of spec in TS).
async function hlsVariant(): Promise<'ts' | 'fmp4'> {
  const video = (await getProperty('video-format', 'string').catch(() => null)) ?? '';
  return video === 'hevc' ? 'fmp4' : 'ts';
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
  if (!cast.active) return;
  const status = await invoke<CastStatusMsg>('cast_status').catch(() => null);
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
      await endCast({ osd: t('cast.ended'), resumeLocal: true, resumePaused: true });
      return;
    case 'stopped':
      // Stopped from the TV side (or another sender took the device over).
      await endCast({ osd: t('cast.stopped'), resumeLocal: true, resumePaused: true });
      return;
    case 'error': {
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
  // and still buffering is a different situation and stays untouched.
  if (!sawPlayback && status.fetches === 0 && Date.now() - loadStartedAt > FETCH_TIMEOUT_MS) {
    await endCast({ osd: t('cast.err_firewall'), resumeLocal: true });
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
  const lastTime = await invoke<number>('cast_disconnect').catch(() => 0);
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

// ---- Remote controls --------------------------------------------------------

export function castTogglePause() {
  void invoke('cast_control', { action: cast.paused ? 'play' : 'pause', value: null }).catch(
    () => {},
  );
  // Optimistic, like every local control: the next status report is up to
  // half a second away and a button that lags reads as broken.
  cast.state = cast.paused ? 'playing' : 'paused';
}

export function castSeek(time: number) {
  const clamped = Math.max(0, Math.min(cast.duration || time, time));
  cast.time = clamped;
  void invoke('cast_control', { action: 'seek', value: clamped }).catch(() => {});
}

export function castSeekBy(delta: number) {
  castSeek(cast.time + delta);
}

/// TV volume is the receiver's own 0..1. Silent — the volume bar shows itself.
export function castSetVolume(frac: number) {
  if (!cast.volumeAdjustable) return;
  const next = Math.max(0, Math.min(1, frac));
  cast.volume = next;
  void invoke('cast_control', { action: 'volume', value: next }).catch(() => {});
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
  const next = !cast.muted;
  cast.muted = next;
  void invoke('cast_control', { action: 'mute', value: next ? 1 : 0 }).catch(() => {});
  showOsd(t(next ? 'osd.sound_off' : 'osd.sound_on'));
}
