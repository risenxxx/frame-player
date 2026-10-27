/**
 * Casting to TV (Google Cast): the frontend half of src-tauri/src/cast/.
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
 *
 * This file is the public face; the code is in `cast/`, one module per seam,
 * each importing only the ones listed before it:
 *
 * - `state.svelte.ts` — the types, the reactive `cast` mirror, the session's bookkeeping
 * - `prefs.ts` — the mode and the cache cap
 * - `record.ts` — the session as an issue report describes it
 * - `devices.ts` — profiles, what a renderer takes, the rows' kind, transport and summary
 * - `diagnosis.ts` — the device check
 * - `ladder.ts` — the Cast and AirPlay ladders, the source, the picker's plan
 * - `discovery.ts` — both discoveries and the remembered addresses
 * - `prepare.ts` — the prepare rung and the HLS rendition for a Chromecast
 * - `controls.ts` — the remote's verbs
 * - `handback.ts` — the end of a session
 * - `airplay.ts` — AirPlay's copy or rendition, its fallback and its track switch
 * - `session.ts` — starting a session on each transport, the poll, the queue
 * - `tracks.ts` — a track picked while a television plays
 */

export {
  AIRPLAY_DEVICE,
  cast,
  type Anchor,
  type CastDevice,
  type CastVerdict,
  type DeviceKind,
  type DlnaDevice,
  type Transport,
  type TvDevice,
} from './cast/state.svelte';
export { castCacheCapGb, castMode, setCastCacheCapGb, setCastMode, type CastMode } from './cast/prefs';
export { lastCastSession, type CastRecord } from './cast/record';
export {
  castStateLabel,
  deviceIsVideoCapable,
  deviceKind,
  deviceProfile,
  deviceSummary,
  dlnaTakesFile,
  pinnedUnavailable,
  plannedTransport,
  setDeviceTransport,
  transportFor,
} from './cast/devices';
export { checkLabel, checkNote, diagnoseDevice, diagnosisText, type CheckLine } from './cast/diagnosis';
export { airplaySummary, airplayVerdict, castVerdict, refreshCastPlan } from './cast/ladder';
export { startCastDiscovery, stopCastDiscovery } from './cast/discovery';
export {
  castNudgeVolume,
  castSeek,
  castSeekBy,
  castSetVolume,
  castToggleMute,
  castTogglePause,
  castUnmute,
} from './cast/controls';
export { disconnectCast, endCast } from './cast/handback';
export { airplayAudioSource } from './cast/airplay';
export { castAdvance, castCurrentFile, castFollow, castFollowing, castOverAirPlay } from './cast/session';
export { castSwitchAudio, castSwitchSub } from './cast/tracks';
