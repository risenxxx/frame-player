/**
 * Starting a session on each transport, the status poll that runs it, and the
 * queue it follows. One module because these call one another in a circle —
 * a start begins the poll, and the poll's fallback and its advance to the next
 * episode start again.
 */

import { invoke } from '@tauri-apps/api/core';
import { setProperty } from 'tauri-plugin-libmpv-api';

import {
  airplayMode,
  airplayRefusals,
  hlsRefused,
  hlsWorthIt,
  pictureKind,
  tracksPlan,
  wantsHls,
} from '../airplay-choice';
import { isPrivatePath } from '../history.svelte';
import { t } from '../i18n.svelte';
import { showOsd } from '../osd.svelte';
import { isNetworkSource, player } from '../player.svelte';
import { suspendThumbs } from '../thumbs.svelte';
import { IS_MAC } from '../platform';
import { neighbour, playEntry, playlist, type PlaylistEntry } from '../playlist.svelte';
import { airplayFallback, airplayNeedsCopy, airplaySelection, prepareAirPlayCopy, prepareAirPlayHls } from './airplay';
import { transportCommand } from './controls';
import { deviceKind, dlnaRefused, extensionOf, mimeAdvertised, refusedKey, transportFor } from './devices';
import { endCast, stopPoll } from './handback';
import { airplayVerdict, castVerdict, resolveCastSource } from './ladder';
import { castMode } from './prefs';
import { dlnaSubtitle, hlsVariant, prepareForCast, prepareHls, selectedAudioParams } from './prepare';
import { noteSession } from './record';
import { AIRPLAY_DEVICE, cast, session, type Anchor, type CastStatusMsg, type TvDevice, type Transport } from './state.svelte';

/// The TV never fetched a byte this long after the LOAD was accepted: the
/// command channel works and the media channel does not, which is the firewall
/// signature (the Defender prompt was declined, or a router isolates clients).
const FETCH_TIMEOUT_MS = 8000;

const POLL_MS = 500;

let sawPlayback = false;

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
  if (device.airplay) return castOverAirPlay(null, keep);
  session.currentDevice = device;
  cast.kind = deviceKind(device);
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
    session.resumeUnpaused = !player.paused;
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
  session.castSrcPath = src;
  const hidden = isPrivatePath(src);
  const mode = castMode();
  // In HLS mode everything goes through the segmenter, direct-play files
  // included — the mode is the transport, and half its purpose today is
  // letting the receiver's HLS behavior be tested with known-good files.
  session.sessionHls = mode === 'hls' ? await hlsVariant() : null;
  let castPath = src;
  if (session.sessionHls || verdict.kind === 'prepare') {
    cast.state = 'preparing';
    try {
      const { audioIndex, channels } = await selectedAudioParams();
      castPath = session.sessionHls
        ? await prepareHls(src, verdict.transcodeAudio, audioIndex, channels, session.sessionHls === 'fmp4')
        : await prepareForCast(src, verdict.transcodeAudio, audioIndex, channels);
    } catch (e) {
      console.warn('cast prepare failed:', e);
      // Canceled by an endCast from elsewhere — that path already spoke.
      if (cast.active) await endCast({ osd: t('cast.prepare_failed'), resumeLocal: false });
      return false;
    }
    if (!cast.active) return false;
    if (!session.sessionHls) {
      session.preparedPath = castPath;
      session.preparedHidden = hidden;
    }
  }

  // The remote takes over: local playback stands down but keeps the file, so
  // tracks, chapters, duration and the storyboard keep feeding the UI and the
  // handback is a seek rather than a reopen.
  await setProperty('pause', true).catch(() => {});
  cast.state = 'loading';
  cast.time = player.timePos;
  session.loadStartedAt = Date.now();

  try {
    await invoke('cast_load', {
      path: castPath,
      position: player.timePos,
      title: hidden ? null : player.displayTitle,
      hidden,
      hls: session.sessionHls,
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
  const rendererMime = renderer ? mimeAdvertised(renderer.mimes, stream.ext) : null;
  const dlnaTakes = rendererMime !== null;
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
    session.resumeUnpaused = !player.paused;
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
  suspendThumbs(true);
  cast.active = true;
  cast.deviceName = device.name;
  cast.state = 'loading';
  cast.time = player.timePos;
  cast.duration = player.duration;
  sawPlayback = false;
  session.castSrcPath = null;
  session.sessionHls = null;

  await setProperty('pause', true).catch(() => {});
  session.loadStartedAt = Date.now();
  session.dlnaSubPath = dlnaTakes ? dlnaSubtitle() : null;
  try {
    if (dlnaTakes) {
      await invoke('dlna_load_url', {
        subtitle: session.dlnaSubPath,
        url,
        position: player.timePos,
        title: hidden ? null : player.displayTitle,
        duration: player.duration,
        // The renderer's own spelling, which is why `dlnaTakes` is true at all.
        // Rust negotiates again against the same Sink list, so this is the
        // agreed answer rather than a guess it has to correct.
        mime: rendererMime ?? 'video/mp4',
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
    session.resumeUnpaused = !player.paused;
  }

  cast.transport = 'dlna';
  suspendThumbs(true);
  cast.active = true;
  cast.deviceName = device.name;
  cast.state = 'loading';
  cast.time = player.timePos;
  cast.duration = player.duration;
  sawPlayback = false;
  session.castSrcPath = src;
  session.sessionHls = null;

  await setProperty('pause', true).catch(() => {});
  session.loadStartedAt = Date.now();
  const hidden = isPrivatePath(src);
  session.dlnaSubPath = dlnaSubtitle();
  try {
    await invoke('dlna_load', {
      path: src,
      position: player.timePos,
      title: hidden ? null : player.displayTitle,
      hidden,
      subtitle: session.dlnaSubPath,
    });
  } catch (e) {
    console.warn('dlna_load failed:', e);
    await endCast({ osd: t('cast.err_load'), resumeLocal: true });
    return false;
  }
  startPoll();
  return true;
}

/**
 * Cast over AirPlay: the system's player, the system's list of receivers.
 *
 * The session starts *before* there is a television. `airplay_open` gives an
 * `AVPlayer` the URL, muted and playing alongside mpv, and opens the system's
 * list beside the row (`anchor`); the session is 'connecting' while the viewer
 * chooses, so the window stays a player rather than a remote. The poll sees
 * the route go external and takes over from there — that moment, not this
 * call, is when mpv stands down. A list closed without a pick, and a
 * television whose own AirPlay switch is off, look identical from here and end
 * the same way: Rust's `not_started` after a grace period.
 *
 * `keep` is the queue following on a live route: a new item on the same
 * player, no list.
 */
export async function castOverAirPlay(anchor: Anchor | null, keep = false): Promise<boolean> {
  if (!IS_MAC) return false;
  if (!keep) fellBack = false;
  const path = player.filePath;
  if (!path) return false;
  const resolved = await resolveCastSource(path);
  if ('refuse' in resolved) {
    showOsd(resolved.refuse);
    return false;
  }
  // AVPlayer reads the file through our server like any receiver would, but a
  // stretch that has not arrived is a hole it would play as silence and
  // black: nothing here can turn Range requests into piece priority for it.
  if ('stream' in resolved) {
    showOsd(t('cast.airplay_stream'));
    return false;
  }
  const src = resolved.src;
  if (isNetworkSource(src)) {
    showOsd(t('cast.local_only'));
    return false;
  }
  const verdict = await airplayVerdict(src);
  if (verdict.kind === 'refuse') {
    showOsd(verdict.reason);
    return false;
  }

  session.currentDevice = AIRPLAY_DEVICE;
  if (!keep) {
    session.resumeUnpaused = !player.paused;
    session.airplayLive = false;
    cast.deviceName = t('cast.airplay_name');
    cast.kind = 'airplay';
  }
  cast.transport = 'airplay';
  cast.active = true;
  cast.state = keep ? 'loading' : 'connecting';
  cast.time = player.timePos;
  cast.duration = player.duration;
  sawPlayback = false;
  session.castSrcPath = src;
  session.sessionHls = null;

  const hidden = isPrivatePath(src);
  let castPath = src;
  session.airplayHls = null;
  cast.airplayTracks = false;
  session.airplayFellBack = false;
  session.airplayKind = pictureKind(player.videoW, player.videoH, player.sourceTransfer);
  // HLS where it buys tracks or subtitles and the receiver is not known to
  // refuse this kind of picture; the copy otherwise (see airplay-choice.ts).
  const worth = hlsWorthIt(tracksPlan(player.audioTracks, player.subTracks));
  const hls = wantsHls(airplayMode(), worth, airplayRefusals(), session.airplayReceiver, session.airplayKind);
  // Going to the copy because this receiver refused this kind before is the
  // same loss as the fallback, and is said the same way.
  cast.notice = !hls && worth && airplayMode() === 'auto' ? t('cast.airplay_original_note') : null;
  if (hls || verdict.kind === 'prepare' || airplayNeedsCopy()) {
    if (!keep) cast.state = 'preparing';
    try {
      castPath = hls ? await prepareAirPlayHls(src) : await prepareAirPlayCopy(src);
    } catch (e) {
      console.warn('airplay prepare failed:', e);
      if (cast.active) await endCast({ osd: t('cast.prepare_failed'), resumeLocal: false });
      return false;
    }
    if (!cast.active) return false;
    if (!hls) {
      session.preparedPath = castPath;
      session.preparedHidden = hidden;
    }
    if (!keep) cast.state = 'connecting';
  }

  try {
    if (keep) {
      await invoke('airplay_replace', { path: castPath, position: player.timePos, hidden });
      session.loadStartedAt = Date.now();
    } else {
      // Up until the television takes over — replaced by "playing on" from
      // the poll, or by whatever ends the session.
      showOsd(t('cast.airplay_pick'), { sticky: true });
      await invoke('airplay_open', { path: castPath, position: player.timePos, hidden, anchor });
    }
    if (session.airplayHls) await invoke('airplay_select', airplaySelection());
  } catch (e) {
    console.warn('airplay open failed:', e);
    const key = String(e) === 'picker_unavailable' ? 'cast.airplay_unavailable' : 'cast.err_load';
    await endCast({ osd: t(key), resumeLocal: true });
    return false;
  }
  startPoll();
  return true;
}

function startPoll() {
  stopPoll();
  session.pollTimer = setInterval(() => void poll(), POLL_MS);
}

async function poll() {
  // A handover takes a second or two, and the television keeps reporting the
  // finished file as ended throughout — without this the queue would jump
  // several episodes at once.
  if (!cast.active || session.advancing) return;
  const status = await invoke<CastStatusMsg>(transportCommand('status')).catch(() => null);
  if (!status || !cast.active) return;
  noteSession(status);

  cast.state = status.state;
  cast.time = status.time;
  if (status.duration > 0) cast.duration = status.duration;
  cast.volume = status.volume;
  cast.muted = status.muted;
  cast.volumeKnown = status.volume_known;
  cast.volumeFixed = status.volume_fixed;
  if (status.device) cast.deviceName = status.device;
  if (cast.transport === 'airplay') {
    // The handover: the route is external now, so the television plays and
    // the window becomes its remote — the moment `castCurrentFile` reaches by
    // calling LOAD, reached here by the viewer's pick.
    if (!session.airplayLive && ['playing', 'paused', 'buffering'].includes(status.state)) {
      session.airplayLive = true;
      session.loadStartedAt = Date.now();
      suspendThumbs(true);
      await setProperty('pause', true).catch(() => {});
    }
    // macOS does not say which receiver was chosen, but the receiver says it
    // by fetching, and the address it fetches from has a name.
    // Rust names it from the set's own Bonjour announcement; a row with the
    // same address is the fallback for a set that has not announced yet.
    const tv = status.peer ? cast.tvs.find((d) => d.ip === status.peer) : undefined;
    if (tv && !status.device) cast.deviceName = tv.name;
    if (tv) cast.kind = deviceKind(tv);
    const receiver = status.device ?? tv?.name ?? null;
    if (receiver) session.airplayReceiver = receiver;
    // A receiver that refuses the rendition fetches its master and nothing
    // after it — or is already known to refuse this kind of picture.
    if (session.airplayLive && session.airplayHls && !session.airplayFellBack) {
      const known = !!(session.airplayReceiver && airplayRefusals()[session.airplayReceiver]?.[session.airplayKind]);
      if (known || hlsRefused(Date.now() - session.loadStartedAt, status.fetches)) {
        await airplayFallback(known);
        return;
      }
    }
  }

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
      // The Mac's own player could not open the rendition — nothing about the
      // receiver, so nothing is remembered, but the copy may still play.
      if (cast.transport === 'airplay' && session.airplayHls && !session.airplayFellBack && status.error === 'load_failed') {
        await airplayFallback(true);
        return;
      }
      const kind = status.error ?? 'closed';
      const key =
        kind === 'unreachable' ? 'cast.err_unreachable'
        : kind === 'launch_failed' ? 'cast.err_launch'
        : kind === 'load_failed' ? 'cast.err_load'
        : kind === 'not_started' ? 'cast.airplay_not_started'
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
  // AirPlay's clock starts at the handover, not at the click: before it the
  // viewer is still choosing, and nothing is late.
  if (cast.transport === 'airplay' && !session.airplayLive) return;
  if (!sawPlayback && status.fetches === 0 && Date.now() - session.loadStartedAt > FETCH_TIMEOUT_MS) {
    await endCast({ osd: t('cast.err_firewall'), resumeLocal: true });
    return;
  }
  // **Fetched and still not playing**: the device is reading the file and
  // showing nothing, which is the failure that used to be perfectly silent —
  // no error frame, no state change, a black screen and a player convinced all
  // is well. Both transports can end up here, and neither reports it.
  if (!sawPlayback && status.fetches > 0 && Date.now() - session.loadStartedAt > START_TIMEOUT_MS) {
    if (cast.transport === 'dlna' && (await fallbackToCast('silent'))) return;
    await endCast({ osd: t('cast.stuck'), resumeLocal: true });
  }
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
  const device = session.currentDevice;
  if (!device || !cast.active) return false;

  const before = player.filePath;
  session.advancing = true;
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
    session.advancing = false;
  }
}


/// True while the session is moving itself to another queue entry.
///
/// The page ends a cast whenever a file is opened, because two playbacks at
/// once is never meant — and following the queue opens a file, so without this
/// the advance would kill the very session it is trying to carry forward.
export function castFollowing(): boolean {
  return session.advancing;
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
  const device = session.currentDevice;
  const src = session.castSrcPath;
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
