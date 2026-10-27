/**
 * The device list as the picker draws it: the profile remembered per
 * television, what a renderer says it takes, what kind of thing a row is, the
 * transport a click would use and the sentence each row says about it.
 */

import { t } from '../i18n.svelte';
import { isNetworkSource, player } from '../player.svelte';
import { castMode } from './prefs';
import { cast, type DeviceKind, type TvDevice, type Transport } from './state.svelte';

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
  // The id moved (a new UDN, a device that lost its Cast half): recognize the
  // television by what a person would recognize it by.
  const byName = Object.values(all).find(
    (p) => p.name === device.name && p.model === device.model,
  );
  return byName ?? { transport: 'auto' };
}

export function setDeviceTransport(device: TvDevice, transport: Transport | 'auto') {
  const all = loadProfiles();
  all[device.key] = { transport, name: device.name, model: device.model };
  localStorage.setItem(PROFILES_KEY, JSON.stringify(all));
  // The picker reads this through a plain function call, so nudge the store to
  // make the rows re-render with their new consequence line.
  cast.profileRevision++;
}

/// Containers this device took the URI for and then failed to play, within
/// this run. **Not persisted**: a renderer under-declaring its formats is a
/// guess we are allowed to retry next time the app starts, while repeating a
/// failure for every episode of a season is not. It also makes the picker's
/// line tell the truth from the second attempt onwards.
export const dlnaRefused = new Set<string>();

export function refusedKey(device: TvDevice, container: string): string {
  return `${device.key}|${container}`;
}

/// Container MIME as a DLNA renderer's Sink list spells it. Anything unlisted is
/// not offered to DLNA, which is the safe direction — the Cast rung still
/// applies.
///
/// **This table decides whether a device is offered; `mime_candidates` in
/// `src-tauri/src/dlna/metadata.rs` decides what is then put on the wire. Keep the two in
/// step** — a spelling missing here is a television that quietly reads as
/// incapable, which is the failure that never produces an error to look at. The
/// entries are per-vendor variants of one container, not different formats:
/// Matroska is advertised as three names in the field and MPEG-TS as four.
const DLNA_MIME: Record<string, string[]> = {
  mkv: ['video/x-matroska', 'video/x-mkv', 'video/mkv'],
  mp4: ['video/mp4'],
  m4v: ['video/mp4'],
  mov: ['video/quicktime', 'video/mp4'],
  avi: ['video/avi', 'video/x-msvideo', 'video/x-ms-avi', 'video/msvideo'],
  ts: ['video/mp2t', 'video/mp2ts', 'video/vnd.dlna.mpeg-tts', 'video/x-mpegts'],
  m2ts: ['video/mp2t', 'video/mp2ts', 'video/vnd.dlna.mpeg-tts', 'video/x-mpegts'],
  mts: ['video/mp2t', 'video/mp2ts', 'video/vnd.dlna.mpeg-tts', 'video/x-mpegts'],
  mpg: ['video/mpeg'],
  mpeg: ['video/mpeg'],
  vob: ['video/mpeg'],
  webm: ['video/webm'],
  wmv: ['video/x-ms-wmv'],
  asf: ['video/x-ms-asf'],
  flv: ['video/x-flv'],
  '3gp': ['video/3gpp'],
  ogv: ['video/ogg'],
};

export function extensionOf(path: string): string {
  // The basename first, or a dotted *directory* becomes the extension.
  const name = path.split(/[?#]/)[0].split(/[/\\]/).pop() ?? '';
  const dot = name.lastIndexOf('.');
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : '';
}

/// Whether this renderer says it takes the open file as it is.
///
/// Matched case-insensitively: a MIME type is case-insensitive by RFC and at
/// least the negotiation on the Rust side already treats it that way, so a
/// device that shouts its Sink list must not read as one that cannot play.
export function dlnaTakesFile(device: TvDevice, path: string | null): boolean {
  if (!device.dlna || !path) return false;
  return mimeAdvertised(device.dlna.mimes, extensionOf(path)) !== null;
}

/// The renderer's own spelling for this container, or `null` if it lists none.
export function mimeAdvertised(advertised: string[], ext: string): string | null {
  const wanted = DLNA_MIME[ext];
  if (!wanted) return null;
  const have = advertised.map((m) => m.toLowerCase());
  return wanted.find((m) => have.includes(m)) ?? null;
}

export function deviceIsVideoCapable(device: TvDevice): boolean {
  if (device.dlna?.mimes.some((m) => m.startsWith('video/'))) return true;
  // A Nest speaker answers on the Cast service like any Chromecast; only the
  // capability bit in its announcement says it has no picture. Unknown (the
  // sweep found it) counts as video, which is what every Cast row used to be.
  return !!device.cast && device.cast.video !== false;
}


/// A Cast dongle or streamer announces itself under Google's own name; a set
/// with Cast built in announces its own model.
const STREAMER_MODEL = /chromecast|google tv/i;

/**
 * What kind of thing a row is, for its icon — only as far as the device itself
 * says. Sound only is a speaker; a Cast receiver that calls itself a
 * Chromecast or a Google TV is a box behind somebody's television; everything
 * else with a picture is a screen. A computer running a DLNA renderer is a
 * screen too: nothing it announces tells it from a television, and a guess
 * from its model name would be wrong often enough to mislead.
 */
export function deviceKind(device: TvDevice): DeviceKind {
  if (!deviceIsVideoCapable(device)) return 'speaker';
  if (device.cast && STREAMER_MODEL.test(device.cast.model)) return 'box';
  return 'tv';
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

/**
 * What the session is doing, as a sentence.
 *
 * A function rather than a `$derived`: it goes through `t()`, and two
 * components show it — the picker's current-device row and the casting screen.
 */
export function castStateLabel(): string {
  return cast.state === 'connecting'
    ? t('cast.state_connecting')
    : cast.state === 'preparing'
      ? t('cast.state_preparing')
      : cast.state === 'loading' || cast.state === 'buffering'
        ? t('cast.state_loading')
        : cast.state === 'paused'
          ? t('cast.state_paused')
          : t('cast.state_playing');
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
    const dlnaTakes = !!device.dlna && mimeAdvertised(device.dlna.mimes, ext) !== null;
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
