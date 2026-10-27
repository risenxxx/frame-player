/**
 * mpv state mirrors and the commands that drive them.
 *
 * Everything here is "what mpv is doing"; nothing here knows about the seekbar,
 * the OSC or any gesture. The page wires the two together through `initPlayer`
 * hooks rather than this module reaching back into it — the observer callback
 * used to be the place where every concern in the app met.
 *
 * **The mirrors go stale.** Heavy seeking overflows mpv's client event queue and
 * property-change events are dropped (`queue-overflow`). Two rules follow, and
 * both are load-bearing: the target of a toggle is never computed from a mirror
 * (mpv inverts its own state via `cycle`), and the mirrors are resynced on a
 * timer the page owns.
 *
 * This file is the public face and only re-exports: every importer goes through
 * it, so the code behind it can be arranged without touching them. The code
 * lives under `player/`, one module per seam, and the dependencies run one way —
 * `state` at the bottom, `session` at the top, nothing in `player/` importing
 * this file:
 *
 * - `state.svelte.ts` — `player`, and the types its fields are made of
 * - `files.ts` — the extensions a drop is sorted by, network sources, the
 *   folders mpv searches for external tracks
 * - `filters.ts` — volume leveling, the SDR colour mode, the viewer's mpv.conf keys
 * - `tracks.ts` — reading list properties, tracks, matching a remembered one
 * - `chapters.ts` — chapters and the skippable ones
 * - `mirrors.ts` — `OBSERVED`, `RESYNC`, `applyProperty`, `resyncState`, and
 *   the waits released from the event path
 * - `commands.ts` — playback, repeat, speed, delays, the A–B loop, picture geometry
 * - `ytdlp.svelte.ts` — where yt-dlp is
 * - `session.ts` — `initPlayer`, the hooks it is given, `loadFiles`
 */

export {
  AUDIO_EXTENSIONS,
  isNetworkSource,
  pointAtExternalTracks,
  SUBTITLE_EXTENSIONS,
  VIDEO_EXTENSIONS,
} from './player/files';
export { player, type Chapter, type LoopMode, type SdrColor, type Track } from './player/state.svelte';
export {
  notePlaybackRestart,
  positionNow,
  resyncState,
  waitPlaybackSettled,
  waitTimePosChange,
  type ObservedName,
} from './player/mirrors';
export { applyYtdlpPath, hasYtdlp, refreshYtdlp, ytdlp } from './player/ytdlp.svelte';
export { initPlayer, loadFiles, pickAndLoadFiles, type PlayerHooks } from './player/session';
export {
  applyLoopMode,
  ASPECT_AUTO,
  changeSpeed,
  clearAbLoop,
  cycleAbLoop,
  cycleLoop,
  DELAY_STEP,
  delayIsZero,
  formatDelay,
  LOOP_LABEL,
  nudgeDelay,
  resetAbLoop,
  resetDelay,
  resetPicture,
  roundDelay,
  setPicture,
  setVolume,
  toggleMute,
  togglePause,
  unmute,
  VOLUME_MAX,
  type PictureProp,
} from './player/commands';
export {
  attachTrack,
  describeTrack,
  loadTracks,
  matchTrack,
  pickAndAttachTrack,
  readList,
  removeSubTrack,
  selectTrack,
  type TrackDesc,
  type TrackWish,
} from './player/tracks';
export {
  applyNormalize,
  applySdrColor,
  isUserConfKey,
  loadNormalize,
  loadSdrColor,
  userConfOptionNames,
} from './player/filters';
export {
  chapterAt,
  chapterTitle,
  jumpChapter,
  loadChapters,
  seekChapter,
  skipKind,
  type SkipKind,
} from './player/chapters';
