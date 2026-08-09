/**
 * The other half of the bus: what arrives from the room, and what this player
 * does about it.
 *
 * **This module sits above everything and is imported by nothing but the page.**
 * `wire.svelte.ts` is a leaf so that `playback` and `seek` can publish into it;
 * applying has to reach in the opposite direction — mpv, the torrent client, the
 * queue — so it lives here, and the two halves meet only through the callbacks
 * `initSync` registers. That is what keeps `npm run check-imports` quiet, and
 * the cycle it prevents is the silent kind: a bundler resolves it and leaves a
 * module-evaluation order nobody chose.
 *
 * Three rules run through everything below.
 *
 * **A remote timeline is not a gesture.** It goes to mpv directly rather than
 * through `playback`'s verbs, and it must: a verb raises an OSD, pauses a drag,
 * and — the one that would actually break — publishes. Routing arrivals back
 * through the verbs would have every peer re-broadcasting every peer, which is a
 * loop that ends with the relay's rate limit rather than with a correct film.
 * The player already draws this distinction: `applyProperty` sweeping a stale
 * mirror against the `property` hook acting on a real event.
 *
 * **Nothing has to arrive.** What travels is a snapshot, so a message dropped
 * during a drag, a stall or a reconnect costs at most one tick of the
 * reconciler below — there is no state machine to fall out of, and no event to
 * replay. That is why the guards can simply *return*: a moment when this player
 * must not be touched is a moment it will be corrected shortly after.
 *
 * **Never fight the viewer's own hands.** While a seek gesture owns the
 * position, while this player's own change is still in flight, and while it is
 * stalled on the network, the reconciler stands down entirely.
 */

import { setProperty } from 'tauri-plugin-libmpv-api';

import { t } from '../i18n.svelte';
import { latest } from '../latest';
import { showOsd } from '../osd.svelte';
import { loadFiles, player } from '../player.svelte';
import { queueTorrent } from '../playlist.svelte';
import { issueSeek, seek, wantExact } from '../seek.svelte';
import { addTorrent, torrent, torrentVideos } from '../torrent.svelte';
import { compareLocal, contentOf, sameContent, type MatchVerdict } from './content';
import { correctionFor, speedChanged } from './drift';
import type { ContentRef, Timeline } from './protocol';
import {
  initWire,
  publishContent,
  publishSettling,
  reportReady,
  serverNow,
  targetPosition,
  wire,
} from './wire.svelte';

/// How often the reconciler runs. A second is the granularity of the drift
/// correction's own arithmetic (it aims to erase a difference over ten of
/// these) and is far below anything a viewer perceives as lag.
const TICK_MS = 1000;

/// After a file loads, playback has not settled: `time-pos` reports zero for a
/// moment and the demuxer is still working. Correcting against that would seek
/// to the start and then back.
const SETTLE_AFTER_LOAD_MS = 1500;

class Sync {
  /// What the room is watching that this player cannot open by itself: a local
  /// file, or one somebody has hidden. Read by the panel, which is the only
  /// place that can say anything useful about it.
  unopenable = $state<ContentRef | null>(null);
  /// How the file this viewer has open compares with the room's.
  match = $state<MatchVerdict>('unknown');
  /// Fetching whatever the room switched to.
  opening = $state(false);
  /// We tried to open what the room is watching and could not — a magnet that
  /// would not resolve, a file that is not in the torrent. Kept as state rather
  /// than announced once and forgotten: an OSD fades, and the viewer is left
  /// looking at a room that is playing something while their own window sits
  /// empty with nothing to explain it. The panel and the chip both read this.
  failed = $state(false);

  /// This player is holding the room up.
  get holdingUp(): boolean {
    return wire.on && wire.waiting.includes(wire.me);
  }
}

export const sync = new Sync();

/// Only the newest attempt at opening may finish: a room that switches episode
/// twice in a second would otherwise land on whichever resolve happened to be
/// slower. The same guard the track list, the torrent poll and the recents rail
/// carry, and for the same reason.
const opens = latest();

/// The content this player last acted on, so an echo of our own publish — or a
/// re-statement of the same film with a title that arrived late — does not
/// reopen anything.
let openedFor: ContentRef | null = null;
let loadedAt = 0;
/// The speed last written for drift correction, so the reconciler does not
/// rewrite mpv's `speed` every second with a value a hair from the last.
let correctedSpeed = 0;

// ---- wiring -----------------------------------------------------------------

/**
 * Start the standing effects and the reconciler.
 *
 * Called from the page, never at this module's top level: a `$effect` written
 * there throws `effect_orphan` the moment the module is imported, and for a
 * module the page imports that is a window which never paints. `npm run
 * check-runes` is the gate that says so.
 */
export function initSync() {
  initWire({ timeline: onTimeline, room: onRoom });

  // Readiness. The relay freezes the room while anybody is not ready, which is
  // what makes watching a torrent together bearable — so this has to be honest
  // in both directions: too eager and the others watch ahead of a viewer who is
  // still buffering, too shy and one slow machine holds an evening.
  $effect(() => {
    // Nothing open yet counts as busy — we are about to open what the room is
    // watching, and holding it until we have is the whole point. Except when
    // there is nothing we *can* open: for a local file or a hidden one, waiting
    // changes nothing, and making an evening stop for somebody who is off
    // looking for their own copy is worse than letting it run. They are told
    // what to open; the panel says so.
    const willOpen = !player.hasFile && !sync.unopenable;
    const busy = sync.opening || player.stalled || willOpen;
    reportReady(!busy, busy ? 'buffering' : '');
  });

  // Keeping the verdict fresh is an effect rather than part of the reconciler
  // because it is about what is *shown*, and the panel should not wait a second
  // to stop saying the wrong thing about a file that has just been opened.
  $effect(() => {
    sync.match = compareLocal(wire.timeline.content, currentLocal());
  });

  // Inside an effect rather than beside one, so the interval is torn down with
  // the component instead of outliving it — which under HMR means one extra
  // reconciler per edit, each writing mpv's `speed` on its own schedule.
  $effect(() => {
    const timer = setInterval(reconcile, TICK_MS);
    return () => clearInterval(timer);
  });
}

function currentLocal() {
  if (!player.hasFile || !player.filePath) return null;
  return { src: player.filePath, duration: player.duration };
}

// ---- what arrives -----------------------------------------------------------

function onTimeline(next: Timeline, fromSelf: boolean) {
  // A content change is the one thing that cannot wait for the reconciler: it
  // means a different film, and every second of delay is a second of the wrong
  // one. Handled before the self-check, because our own publish is exactly what
  // tells us the open we started is the one the room settled on.
  if (!sameContent(next.content, openedFor)) void openContent(next.content);
  if (fromSelf) return;
  // Everything else goes through the ordinary path, so there is one place that
  // decides whether this player may be touched at all.
  reconcile();
}

function onRoom() {
  // **Joining a room while something is already playing has to say so**, and
  // the absence of this was the bug that made the feature look broken end to
  // end: `syncNoteFileLoaded` only fires when a file *loads*, so somebody who
  // opened a film and then created a room published nothing at all. Everybody
  // who joined landed in a room watching nothing, with no way to guess that the
  // host was three minutes into an episode — and, because a guest with no file
  // open is deliberately not-ready, they also held the room frozen for ever
  // while doing it.
  //
  // Only when the room has nothing: if it is already watching something, that
  // is what this player should be opening, not overwriting.
  if (wire.on && !wire.timeline.content && player.hasFile) void shareWhatIsPlaying();
  if (!wire.on) {
    // Leaving a room must not leave the film running at a corrected speed.
    restoreSpeed();
    sync.unopenable = null;
    sync.opening = false;
    sync.failed = false;
    openedFor = null;
  }
}

/**
 * Open what the room is watching.
 *
 * A torrent is the case this is good at, and the reason the roadmap paired this
 * feature with torrents: the magnet plus an index names the file exactly, the
 * metadata is cached beside the data, and switching episode is the same info
 * hash with another index. A guest joining mid-season is a `torrent_add` that
 * costs milliseconds and a `loadfile`.
 *
 * A local file is the case it cannot do anything about. Saying so is the whole
 * of the response — the panel names the film and its length, and the viewer
 * opens their own copy.
 */
async function openContent(ref: ContentRef | null) {
  const run = opens.begin();
  openedFor = ref;
  sync.unopenable = null;
  sync.failed = false;

  if (!ref) return;
  if (ref.kind === 'file' || ref.kind === 'hidden') {
    sync.unopenable = ref;
    return;
  }

  sync.opening = true;
  try {
    if (ref.kind === 'url') {
      await loadFiles([ref.url]);
      if (run.stale) return;
    } else {
      const info = await addTorrent(ref.magnet);
      if (run.stale) return;
      const videos = torrentVideos(info);
      // By index first, because that is the identity. By name as a fallback: a
      // re-uploaded torrent renumbers its files, and landing on the right
      // episode by name beats landing on the wrong one by number.
      const file =
        info.files.find((f) => f.index === ref.index) ??
        videos.find((f) => f.path === ref.file) ??
        null;
      if (!file) {
        sync.failed = true;
        showOsd(t('sync.no_such_file'));
        return;
      }
      await loadFiles([file.url]);
      if (run.stale) return;
      await queueTorrent(videos, file.url);
    }
    loadedAt = performance.now();
  } catch (e) {
    if (!run.stale) {
      sync.failed = true;
      showOsd(t('sync.open_failed'));
    }
    console.warn('[sync] could not open what the room is watching:', e);
  } finally {
    if (!run.stale) sync.opening = false;
  }
}

// ---- keeping in step --------------------------------------------------------

/**
 * Bring this player to where the room is, if it may be touched at all.
 *
 * Runs once a second and on every arrival. Every early return below is a moment
 * when acting would be worse than being a second late — and being a second late
 * costs nothing here, because the next tick recomputes from a snapshot rather
 * than replaying anything.
 */
function reconcile() {
  if (!wire.on || !player.hasFile) return;
  const tl = wire.timeline;
  if (!tl.content) return;
  // Our own last change has not come back yet, so the authoritative timeline
  // still describes where the film *was*. Correcting against it would haul
  // playback back and the echo would send it forward again.
  if (publishSettling()) return;
  // The viewer's own hands own the position.
  if (seek.dragging || seek.scrubbing || seek.settling) return;
  // Stalled on the network. We are the reason the room is frozen, and seeking
  // now would ask the swarm for a different piece than the one being waited on.
  if (player.stalled) return;
  // Still opening, or just opened: `time-pos` reports zero for a moment after a
  // load and correcting against that seeks to the start and back.
  if (sync.opening || performance.now() - loadedAt < SETTLE_AFTER_LOAD_MS) return;
  // On a different film — either one we cannot open, or one the viewer chose.
  // The timeline is about something else, so applying it would move this file
  // to a position that means nothing in it.
  if (sync.unopenable && sync.match === 'unknown') return;
  if (sync.match === 'mismatch') return;

  if (player.paused !== tl.paused) {
    // An absolute set rather than `cycle`: a toggle against a mirror that has
    // gone stale (an mpv event-queue overflow drops property changes) would put
    // this player in the opposite state to the room and keep it there.
    void setProperty('pause', tl.paused);
  }

  const drift = player.timePos - targetPosition();
  const plan = correctionFor(drift, tl.speed, tl.paused);
  switch (plan.do) {
    case 'seek':
      restoreSpeed();
      // The file's own exact/keyframe verdict, the same one every other seek in
      // the player obeys — a room is not a reason to pay 1.9 s for a frame.
      void issueSeek(targetPosition(), wantExact(false));
      break;
    case 'speed':
      writeSpeed(plan.speed);
      break;
    case 'nothing':
      restoreSpeed();
      break;
  }
}

/**
 * Write a corrected speed, and remember it.
 *
 * The room's own speed is the base and is re-read every tick rather than kept,
 * so a viewer changing the speed mid-correction is not fought and not undone:
 * the next tick simply corrects around the new base.
 */
function writeSpeed(next: number) {
  if (!speedChanged(player.speed, next)) return;
  correctedSpeed = next;
  void setProperty('speed', next);
}

/// Back to what the room is running at. Only when we are the ones who moved it —
/// otherwise leaving a room, or drifting back into the deadband, would overwrite
/// a speed the viewer chose.
function restoreSpeed() {
  if (!correctedSpeed) return;
  correctedSpeed = 0;
  const base = wire.on ? wire.timeline.speed || 1 : 1;
  if (speedChanged(player.speed, base)) void setProperty('speed', base);
}

// ---- what this player is watching -------------------------------------------

/**
 * Tell the room what has just been opened here.
 *
 * Called from `onFileLoaded`, which is the composition root's job — this module
 * has no business observing mpv directly, and the page is already where every
 * module is told a file arrived.
 *
 * The `sameContent` check is what makes this safe to call unconditionally: when
 * the file was opened *because* the room asked for it, there is nothing to say,
 * and saying it anyway would be a second revision for one change.
 */
export async function syncNoteFileLoaded() {
  loadedAt = performance.now();
  await shareWhatIsPlaying();
}

/**
 * Tell the room what this player has open.
 *
 * Separate from `syncNoteFileLoaded` because the other caller — joining a room
 * that is watching nothing — must not stamp `loadedAt`: that suppresses the
 * reconciler for a second and a half, and a `members` message is not a reason
 * to stop keeping in step.
 */
async function shareWhatIsPlaying() {
  if (!wire.on) return;
  const src = player.filePath;
  const ref = await contentOf(src, {
    title: player.mediaTitle,
    duration: player.duration,
    torrent: torrent.info,
  });
  // The file changed again while the hash was being read.
  if (!wire.on || player.filePath !== src) return;
  if (sameContent(ref, wire.timeline.content)) {
    openedFor = ref;
    return;
  }
  // A guest in a host-only room is simply watching something else; `publish`
  // refuses and says so rather than pretending.
  if (!wire.mayDrive) return;
  openedFor = ref;
  publishContent(ref, player.timePos, player.paused);
}
