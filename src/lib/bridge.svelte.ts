/**
 * The bridge between mpv's state and the interface: what happens in the rest
 * of the player when a property changes, a file finishes loading, a load is
 * about to start or playback (re)starts — the hooks `initPlayer` is given — and
 * the Windows taskbar's progress bar, which those reports drive.
 *
 * Out of the page's script, where this was "Bridge between mpv state and the
 * page". The player module knows nothing about the UI and takes these hooks
 * rather than reaching back; this is the one place that knows both, which is
 * why its import list is long. Effects are not a substitute for the `property`
 * hook — see the `$effect` rule in docs/rules/frontend.md.
 */

import { getCurrentWindow, ProgressBarStatus } from '@tauri-apps/api/window';
import { setProperty } from 'tauri-plugin-libmpv-api';
import { cast, castFollowing, endCast } from './cast.svelte';
import { restoreCrop } from './crop.svelte';
import { curtain, curtainFileLoaded, curtainFileStarted, raiseCurtain } from './curtain.svelte';
import { noteLocalPosition, onReachedEnd, resetEnd, resetSkipGuard } from './endscreen.svelte';
import {
  frameStep,
  frameStepFileChanged,
  frameStepNotePause,
  frameStepNoteRestart,
} from './frame-step.svelte';
import { flushPosition, maybeRecordPosition, rememberTitle } from './history.svelte';
import { t } from './i18n.svelte';
import {
  cancelLoadFailure,
  noteOpened,
  openTorrent,
  opening,
  reportLoadFailure,
  resolveTitleIfMissing,
} from './open.svelte';
import { restoreAdjust } from './picture-adjust.svelte';
import { IS_MAC } from './platform';
import { playback } from './playback.svelte';
import {
  isNetworkSource,
  notePlaybackRestart,
  player,
  pointAtExternalTracks,
  resetAbLoop,
  resetPicture,
  type ObservedName,
  type PlayerHooks,
} from './player.svelte';
import { dropPosters, loadPlaylist, queueFolder } from './playlist.svelte';
import { applyResume, prepareResume } from './resume.svelte';
import { resetSeekProbe, seek } from './seek.svelte';
import { backBufferFor } from './source';
import { armVideoReady, enterStartScreen, startScreen } from './start-screen.svelte';
import { cancelStep, schedulePrewarm, step, USE_STEP_ENGINE } from './step-engine.svelte';
import { syncNoteFileLoaded, syncNotePlaybackRestart } from './sync/apply.svelte';
import { maybeStartThumbs } from './thumbs.svelte';
import { slideTitle, title } from './title.svelte';
import { attachTorrentSubtitles, releaseTorrent, torrent } from './torrent.svelte';
import { applyTiming, restoreTrackChoice } from './tracks.svelte';
import { restoreUpscale, upscaleSourceChanged } from './upscale.svelte';
import { exitMini, scheduleShape } from './window-prefs.svelte';
import { resetZoom } from './zoom.svelte';

/// Throttles the Windows taskbar progress bar to about once a second.
let lastTaskbarUpdate = 0;

/// What `initPlayer` is handed: the page's side of every mpv event.
export const playerHooks: PlayerHooks = {
  beforeLoad: async (path) => {
    // Opening another file while casting ends the session first: mpv is
    // about to start playing locally, and two playbacks at once — one
    // here, one on the TV — is never what a viewer meant. The exception
    // is the session moving itself along the queue, which opens the next
    // episode on purpose and hands it over (`castFollowing`).
    if (cast.active && !castFollowing()) {
      await endCast({ osd: t('cast.stopped'), resumeLocal: false });
    }
    cancelStep();
    opening.attempting = path;
    cancelLoadFailure();
    opening.busy = isNetworkSource(path);
    // Primes the knob and points mpv's `start` at the resume position,
    // which is why this hook is awaited: `start` is a load-time option and
    // has to land before `loadfile`. The curtain is awaited beside it for
    // its own reason — the picture on screen has to be dark before mpv
    // is handed the file that replaces it — and the two wait together
    // rather than in turn.
    // The external-track folders join them: `sub-file-paths` and
    // `audio-file-paths` are read when the file opens, like `start`.
    await Promise.all([prepareResume(path), raiseCurtain(), pointAtExternalTracks(path)]);
  },
  property: onPlayerProperty,
  fileLoaded: onFileLoaded,
  // Opening ONE file queues the rest of its folder; opening several is an
  // explicit selection and is left exactly as it was given.
  filesOpened: (videos) => {
    if (videos.length === 1) void queueFolder(videos[0]);
    else void loadPlaylist();
  },
  loadFailed: reportLoadFailure,
  openTorrentFile: (path) => void openTorrent(path),
  playbackRestart: () => {
    armVideoReady();
    // Frames are being produced, which is the one thing the curtain is
    // waiting to hear and the picture's own properties may not say: a
    // file of the same shape as the last can change none of them. The
    // window looks again and reports (`settled`).
    if (curtain.on) scheduleShape();
    // Before the waiters are released: a restart nobody stepping asked
    // for is a seek that the frame stepper has to turn forward after.
    frameStepNoteRestart();
    notePlaybackRestart();
    // The one event that means frames are actually being produced, which
    // is what a room has to be told about — a file mpv has merely been
    // handed looks identical from every other angle.
    syncNotePlaybackRestart();
  },
};

/// Reactions to mpv property changes that are not the player module's own
/// business: the seekbar knob, the taskbar, the start screen, the title
/// slide. Runs after the mirror is updated, so `player.x` is already fresh.
function onPlayerProperty(name: ObservedName) {
  switch (name) {
    case 'pause':
      if (player.paused && USE_STEP_ENGINE && !step.on) schedulePrewarm();
      frameStepNotePause();
      updateTaskbarProgress();
      if (player.paused) maybeRecordPosition(true);
      break;
    case 'time-pos':
      // While a gesture owns the position, the gesture sets the knob.
      // Otherwise it jitters: mpv still reports the old position (a seek is
      // in flight) or one that landed on a keyframe, and the knob bounces
      // between the finger and the lagging value.
      if (!seek.dragging && !step.on && !seek.scrubbing && !seek.settling) seek.value = player.timePos;
      // Releases the skip button's mute and pre-decodes the next poster. In
      // the `property` hook rather than an $effect for the usual reason: both
      // must run on a position report, not whenever one of the values they
      // read happens to change.
      noteLocalPosition();
      if (performance.now() - lastTaskbarUpdate > 1000) updateTaskbarProgress();
      break;
    case 'duration':
      maybeStartThumbs();
      break;
    case 'video-params/w':
    case 'video-params/h':
      upscaleSourceChanged();
      break;
    case 'filename':
      clearTimeout(startScreen.emptyTimer);
      if (player.filename === null) {
        updateTaskbarProgress();
        // the file is gone (switched or closed) — commit the last position
        flushPosition();
        dropPosters();
        void exitMini();
        // Nothing is playing, so nothing may still be talking to a swarm:
        // uploading to strangers after the film is closed is not something a
        // video player gets to do quietly. The pieces stay on disk, so
        // re-opening the magnet continues rather than starting over.
        if (torrent.info) void releaseTorrent();
        // Not when the picker is already up: `backToStart` raises it before
        // issuing the stop, and re-running this would rebuild the recents
        // list a second time — visible, because the cards would reshuffle
        // under the cursor as the posters arrive again.
        if (!startScreen.showEmpty) startScreen.emptyTimer = setTimeout(enterStartScreen, 300);
      } else {
        startScreen.showEmpty = false;
        // the pause lets media-title update before the slide
        setTimeout(() => slideTitle(), 120);
      }
      break;
    case 'eof-reached':
      // Decoding backward, the start of the file is mpv's end: the frame
      // stepper reaching the first frame is not the film being over.
      if (frameStep.reverse) break;
      if (player.eofReached) onReachedEnd();
      else resetEnd();
      break;
    case 'playlist-count':
      void loadPlaylist();
      break;
    case 'media-title':
      // Free: yt-dlp resolved it, mpv passed it on. Recording it here is what
      // keeps the recent-links list readable without asking anyone anything.
      if (player.mediaTitle && player.filePath) rememberTitle(player.filePath, player.mediaTitle);
      break;
    case 'path':
      // The net under `raiseCurtain`: a file that reached mpv by a way that
      // does not pass through one of our verbs.
      if (player.filePath) curtainFileStarted();
      frameStepFileChanged();
      // The seek-back buffer for where this file's bytes come from — see
      // `backBufferFor`.
      void setProperty('demuxer-max-back-bytes', backBufferFor(player.filePath)).catch(() => {});
      resetSeekProbe();
      resetSkipGuard();
      maybeStartThumbs();
      break;
    case 'playlist-pos':
      if (player.playlistPos !== title.lastPlaylistPos) {
        title.slideDir = player.playlistPos > title.lastPlaylistPos ? 'next' : 'prev';
        // Consumed by the next slideTitle: mpv reports the new position
        // before the new filename, so the flag is always set in time.
        title.playlistStepped = true;
        title.lastPlaylistPos = player.playlistPos;
      }
      break;
  }
}

function onFileLoaded() {
  curtainFileLoaded();
  noteOpened();
  resetZoom(true);
  // Rotation and aspect are global mpv options: a phone clip turned upright
  // would still be turned for the next episode. Loop points and track delays
  // likewise, and the subtitle frame-rate stretch.
  resetPicture();
  restoreCrop();
  restoreAdjust();
  restoreUpscale();
  resetAbLoop();
  applyTiming();
  // Subtitles the torrent itself carries. `sub-auto` finds nothing here —
  // there is no local file to look beside — so without this they sit in the
  // torrent unreachable while the viewer searches OpenSubtitles for something
  // already on their own disk.
  void attachTorrentSubtitles();
  restoreTrackChoice();
  resetEnd();
  void loadPlaylist();
  resolveTitleIfMissing();
  // Releases the knob, clears mpv's sticky `start`, and seeks only if the
  // load did not already begin at the resume point.
  applyResume();
  // Last, and deliberately: it reads the title and the duration, which are
  // what the room shows, and both are worth more once the rest has run.
  void syncNoteFileLoaded();
}

function updateTaskbarProgress() {
  // On macOS this is a bar over the Dock icon — not done there, just noise.
  if (IS_MAC) return;
  lastTaskbarUpdate = performance.now();
  const win = getCurrentWindow();
  // **While casting the position lives on the television.** mpv sits paused
  // on the file it handed over, so reading its mirrors here pinned the bar to
  // the moment of the handoff and painted it "paused" for the whole session —
  // `player.paused` being true is what makes the handover work in the first
  // place. `playback` is the answer to that for every reader at once.
  const time = playback.position;
  const total = playback.duration;
  const paused = playback.paused;
  if (!player.hasFile || total <= 0) {
    void win.setProgressBar({ status: ProgressBarStatus.None }).catch(() => {});
    return;
  }
  void win
    .setProgressBar({
      status: paused ? ProgressBarStatus.Paused : ProgressBarStatus.Normal,
      progress: Math.min(100, Math.max(0, Math.round((time / total) * 100))),
    })
    .catch(() => {});
}

/// The seekbar follows the TV while casting. Guarded on `seek.dragging` so the
/// gesture owns the knob, exactly like the time-pos observer's guard — and
/// safe as an $effect (which the observer guard is not) because on a drag
/// release `cast.time` is already the optimistic release position, so the
/// re-run cannot yank the knob backwards. `remote`, not `active`: while the
/// prepare rung runs, local playback continues and time-pos owns the knob —
/// two writers on one seekbar is the jitter bug in a new costume.
export function initCastFollow() {
  $effect(() => {
    const time = cast.time;
    if (!cast.remote) return;
    if (!seek.dragging) seek.value = time;
    // **The taskbar has no other driver while casting.** It is refreshed from
    // mpv's property events, and with the local player parked `time-pos` never
    // arrives — so the bar was set once, at the handoff, and sat there for the
    // whole session. This poll is the only thing that knows the television
    // moved. Unthrottled on purpose: it runs at the cast poll's 2 Hz, which is
    // what the observer's 1 s throttle exists to cut a ~10 Hz local report
    // down to, and a skipped run would leave the bar the wrong colour across a
    // pause. Inside the `remote` guard for a reason that is easy to miss: the
    // update reads mpv's mirrors on the local branch, so calling it here
    // unconditionally would make this effect depend on `time-pos` and re-run
    // ten times a second all through ordinary playback. Leaving the session
    // hands the bar back to those same property events, which is where it
    // belongs — the handback's seek and unpause both fire one.
    updateTaskbarProgress();
  });
}

