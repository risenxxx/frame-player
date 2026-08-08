/**
 * Seekbar hover thumbnails.
 *
 * Rust owns the decoding and the disk cache (thumb_service.rs); this side only
 * asks for a position and keeps the blob URLs alive.
 *
 * Two kinds of request, for the same reason the seekbar drag has two kinds of
 * seek. While the cursor moves, frames come from the storyboard grid: instant,
 * but quantised to the grid step (10 s on anything longer than 40 minutes).
 * When it comes to rest, that position IS the moment being aimed at, so the
 * exact frame is fetched and replaces the grid one — the same trade
 * `scheduleDragSettle()` makes with the video itself.
 */

import { invoke } from '@tauri-apps/api/core';

import { isNetworkSource, player } from './player.svelte';
import { parseTorrentUrl } from './source';
import { positionBuffered } from './torrent.svelte';

class Thumbs {
  /// Previews come one at a time from a file that is still downloading: there
  /// is no grid, so nothing appears until the cursor rests, and only over a
  /// stretch that has arrived.
  partial = $state(false);
  /// A storyboard is running and the hover popup is worth showing a frame in.
  /// **Not** the same as "the source is local": a torrent turns this on only
  /// once its file is complete, which happens some way into playback.
  available = $state(false);
  src = $state<string | null>(null);
  /// The frame being replaced, layered *on top* of `src` and faded out. Null
  /// except for the length of that fade.
  fading = $state<string | null>(null);
  loading = $state(false);
}

export const thumbs = new Thumbs();

/// Grid cells, keyed by cell index. Exact frames are keyed by PTS and the two
/// spaces overlap numerically (cell 12 vs 12.0 s), hence two maps rather than
/// one — a collision would show a frame from elsewhere in the film.
const cache = new Map<number, string>();
const exactCache = new Map<number, string>();
/// Exact frames accumulate one per rest; the grid map is bounded by the file's
/// cell count, this one is not.
const EXACT_CACHE_MAX = 48;
let trailing: ReturnType<typeof setTimeout> | undefined;
let throttledAt = 0;
let reqSeq = 0;
let startedFor: string | null = null;
/// How long the cursor must sit still before the exact frame is fetched.
///
/// Shorter than scheduleDragSettle()'s 120 ms, deliberately: that timer gates a
/// seek of the video itself, this one only a 320px thumbnail. Pointer moves
/// arrive every 8–17 ms, so 70 ms of silence is already four frames of genuinely
/// not moving, and every millisecond here is paid in full before the user sees
/// the corrected frame.
const SETTLE_MS = 70;
let settleTimer: ReturnType<typeof setTimeout> | undefined;
let exactShownAt: number | null = null;
let exactShownSrc: string | null = null;
/// One exact request in flight at a time, as pumpScrubSeek does with seeks: each
/// one costs a GOP of software decode (measured 120 ms on 1080p, up to 1.9 s on
/// 4K60 VP9), so a slow sweep across the bar could otherwise queue several
/// seconds of decoding whose results are all stale on arrival.
let exactInFlight = false;
let exactPending = false;
/// The page's current hover position. Kept so a queued request can re-read where
/// the cursor is NOW rather than replay where it was when the request was made.
let hoverAt: (() => number | null) | null = null;
/// Backstop that drops the outgoing frame once its crossfade is over. Only a
/// backstop — the CSS animation has already held it at zero opacity by then —
/// so it merely has to outlast `.thumb.fading`'s duration in +page.svelte.
const FADE_HOLD_MS = 260;
let fadeTimer: ReturnType<typeof setTimeout> | undefined;

/**
 * Starts the background storyboard once both the path and the duration are
 * known — mpv's event order between the two is not guaranteed, so this is
 * called from both.
 */
/// While a television owns playback, the storyboard is a prefetch for a gesture
/// nobody is making. It decodes the **whole** file in software — three threads
/// on a duty cycle, which on a 4K release is the better part of two cores for
/// minutes — to make hovering instant, and during a cast the viewer is looking
/// at the television. Hovering still works meanwhile: a cell with no cached
/// frame is decoded on demand, one seek per hover instead of none.
///
/// Set by the cast session rather than read from it, so this module keeps
/// depending on nothing.
let suspended = false;

export function suspendThumbs(on: boolean) {
  suspended = on;
  // Ending a session is exactly when the prefetch becomes worth doing again,
  // and `startedFor` is only set once something actually started, so this
  // resumes rather than restarts.
  if (!on) maybeStartThumbs();
}

export function maybeStartThumbs() {
  const src = player.filePath;
  if (suspended || !src || player.duration <= 0 || startedFor === src) return;

  // Never for a stream. The storyboard decodes the WHOLE file in the background
  // to fill the hover previews — over a network source that means pulling the
  // entire thing a second time, in an order unrelated to playback, while the
  // player is trying to buffer the next thirty seconds of it.
  //
  // **A torrent is the exception, once the file has arrived.** Then it is an
  // ordinary file on this disk and decoding it costs nothing, so the reason for
  // the gate is gone. `torrent_local_path` answers only for a *complete* file
  // and that is load-bearing: the files are sparse, so a stretch that has not
  // arrived reads back as zeros rather than failing, and a storyboard built over
  // one would be black frames and mush with nothing to say it went wrong.
  if (isNetworkSource(src)) {
    const ref = parseTorrentUrl(src);
    // `startedFor` is set only once something actually starts, so this stays
    // retryable: a torrent's file completes some way *into* playback, and the
    // status poll calls back here when it does. The in-flight guard is what
    // keeps a once-a-second poll from stacking lookups.
    if (!ref || lookingUp === src) return;
    lookingUp = src;
    void (async () => {
      const local = await invoke<{ path: string; complete: boolean } | null>(
        'torrent_local_path',
        { infoHash: ref.infoHash, index: ref.index },
      ).catch(() => null);
      lookingUp = null;
      // Still the same video? This awaited a round trip, and a torrent file
      // completes long after it starts playing — by which time the viewer may
      // be two episodes on.
      if (!local || player.filePath !== src) return;

      if (local.complete) {
        startedFor = src;
        partialPath = null;
        resetThumbCache();
        startStoryboard(local.path, player.duration);
        return;
      }

      // Not finished, so the background grid pass must not run — it walks the
      // file start to end and would decode straight through a hole, producing
      // black frames with nothing to say they are wrong. Single frames are fine
      // where the data is, which is what `positionBuffered` decides.
      if (partialPath !== local.path) {
        partialPath = local.path;
        thumbPath = local.path;
        thumbs.available = true;
        thumbs.partial = true;
      }
    })();
    return;
  }

  startedFor = src;
  partialPath = null;
  resetThumbCache();
  startStoryboard(src, player.duration);
}

/// What the thumbnail service is working on, which is **not** `player.filePath`
/// for a torrent: there it is the file on disk behind the loopback URL.
let thumbPath: string | null = null;
/// A `torrent_local_path` round trip is in flight for this source.
let lookingUp: string | null = null;
/// The on-disk file of a torrent that is still downloading. Single frames may
/// be decoded from it; a storyboard may not.
let partialPath: string | null = null;

function startStoryboard(path: string, duration: number) {
  thumbPath = path;
  thumbs.available = true;
  thumbs.partial = false;
  void invoke('thumb_start', { path, duration }).catch((e) => {
    thumbs.available = false;
    console.warn('thumb_start failed:', e);
  });
}

export function requestThumb(pos: number, hoverTime: () => number | null) {
  if (!thumbPath) return;
  hoverAt = hoverTime;
  // Armed from the pointer move, NOT from every request: the throttle below
  // re-enters up to 85 ms later, and re-arming there pushed the exact frame a
  // further 85 ms out for nothing the user did.
  clearTimeout(settleTimer);
  settleTimer = setTimeout(fireExactThumb, SETTLE_MS);
  // No grid exists for a file that is still downloading — there is no
  // storyboard behind it — so the moving-cursor request would only ever miss.
  if (!thumbs.partial) pumpGridThumb(pos, hoverTime);
}

/// Grid frames while the cursor moves: throttled with a trailing call, because
/// the last cursor position always has to reach a request — otherwise the
/// preview sticks on the second-to-last frame.
function pumpGridThumb(pos: number, hoverTime: () => number | null) {
  const since = performance.now() - throttledAt;
  if (since < 80) {
    clearTimeout(trailing);
    trailing = setTimeout(() => {
      const t = hoverTime();
      if (t !== null) pumpGridThumb(t, hoverTime);
    }, 85 - since);
    return;
  }
  throttledAt = performance.now();
  void issueThumb(pos, false);
}

/// The cursor stopped — fetch the frame at exactly where it rests.
function fireExactThumb() {
  const t = hoverAt?.() ?? null;
  if (t === null) return;
  // Already showing this exact frame: re-resting on the same spot must not
  // re-decode a GOP. It has to be the frame ON SCREEN, not merely one fetched
  // for this position before — hover positions are pixel-quantised, so leaving
  // and returning to the same pixel is common, and by then the grid frame is
  // what is displayed.
  if (exactShownAt !== null && Math.abs(exactShownAt - t) < 1e-6 && thumbs.src === exactShownSrc)
    return;
  if (exactInFlight) {
    exactPending = true;
    return;
  }
  // The pending grid request is for this same resting position and is strictly
  // worse than what is now on its way. Dropping it also removes the only way it
  // could land last and overwrite the exact frame.
  clearTimeout(trailing);
  exactInFlight = true;
  void issueThumb(t, true).finally(() => {
    exactInFlight = false;
    if (exactPending) {
      exactPending = false;
      fireExactThumb();
    }
  });
}

async function issueThumb(pos: number, exact: boolean) {
  if (!thumbPath) return;
  // Decoding outside a downloaded stretch reads zeros out of the sparse file and
  // yields a black frame that looks like a real one. The gate is here rather
  // than at the call site so no future caller can skip it.
  if (thumbs.partial && !positionBuffered(pos / Math.max(1e-9, player.duration))) {
    thumbs.src = null;
    return;
  }
  const seq = ++reqSeq;
  // Only the grid request may raise the spinner. An exact request always has a
  // grid frame already on screen, and flashing a spinner over it every time the
  // cursor stops is noise, not feedback.
  if (!exact) thumbs.loading = true;
  try {
    const buf = await invoke<ArrayBuffer>('thumb_get', { path: thumbPath, pos, exact });
    // Past this guard the request is the newest one, so nothing else is coming
    // to lower the spinner — every exit below has to lower it itself, whether
    // or not this particular request was the one that raised it.
    if (seq !== reqSeq) return;
    if (buf.byteLength <= 8) {
      thumbs.loading = false;
      return;
    }
    // Cell index for a grid frame, PTS for an exact one — separate key spaces.
    const key = new DataView(buf).getFloat64(0, true);
    const store = exact ? exactCache : cache;
    let url = store.get(key);
    if (url) {
      // Move to the end. Pruning drops the oldest entry, and a hit means the
      // frame about to be shown is an old one — leaving it at the front would
      // let a later insert revoke the URL out from under the <img>.
      if (exact) {
        store.delete(key);
        store.set(key, url);
      }
    } else {
      url = URL.createObjectURL(new Blob([new Uint8Array(buf, 8)], { type: 'image/jpeg' }));
      store.set(key, url);
      if (exact) pruneExactCache();
    }
    // A refined frame replaces one the user has already been reading, and in
    // ~40 % of hovers it is a different scene — hard-cutting it in reads as a
    // glitch. What gets layered on top and faded out is the OUTGOING frame, so
    // that `src` underneath is always the newest one: the browser holds an
    // <img>'s previous content until the new src decodes, and putting the
    // *incoming* frame on top would let that stale content show through
    // underneath for a frame. Grid frames get no fade at all — those track a
    // moving cursor and have to be instant.
    if (exact) {
      if (thumbs.src && thumbs.src !== url) {
        thumbs.fading = thumbs.src;
        clearTimeout(fadeTimer);
        fadeTimer = setTimeout(() => (thumbs.fading = null), FADE_HOLD_MS);
      }
    } else {
      clearTimeout(fadeTimer);
      thumbs.fading = null;
    }
    thumbs.src = url;
    thumbs.loading = false;
    if (exact) {
      exactShownAt = pos;
      exactShownSrc = url;
    }
  } catch (e) {
    if (seq === reqSeq) thumbs.loading = false;
    // no thumbnail — the tooltip shows only the time
    console.warn('thumb_get failed:', e);
  }
}

/// Oldest first: Map preserves insertion order, and an entry the preview is
/// currently showing was inserted last, so it cannot be the one dropped.
function pruneExactCache() {
  while (exactCache.size > EXACT_CACHE_MAX) {
    const oldest = exactCache.keys().next();
    if (oldest.done) return;
    URL.revokeObjectURL(exactCache.get(oldest.value)!);
    exactCache.delete(oldest.value);
  }
}

export function resetThumbCache() {
  // A new file has no storyboard until one is started for it. Left set, the
  // popup would reserve its 180px frame over a torrent whose data has not
  // arrived — the "preview that broke rather than one that was never coming"
  // this already learned to avoid for streams.
  thumbs.available = false;
  thumbs.partial = false;
  thumbPath = null;
  partialPath = null;
  clearTimeout(settleTimer);
  clearTimeout(trailing);
  clearTimeout(fadeTimer);
  // Before the URLs are revoked below, or the fading layer would be left
  // pointing at a dead blob.
  thumbs.fading = null;
  for (const url of cache.values()) URL.revokeObjectURL(url);
  for (const url of exactCache.values()) URL.revokeObjectURL(url);
  cache.clear();
  exactCache.clear();
  exactShownAt = null;
  exactShownSrc = null;
  exactPending = false;
  thumbs.src = null;
}
