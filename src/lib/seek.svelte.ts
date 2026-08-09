/**
 * Everything that moves the playhead: the seekbar drag, the wheel scrub, the
 * arrows and the digit jumps — plus the one measurement they all share.
 *
 * This is the most invariant-dense code in the player and the least covered by
 * tests, so it was moved here as a unit rather than split by gesture. The
 * reasoning is kept verbatim from where it was written; architecture.md and the
 * "Seek flags are a performance contract" section of CLAUDE.md are the long
 * form. The rules that must not be broken, in one place:
 *
 *   - **Exactly one seek in flight**, never a timer. A fixed interval builds a
 *     queue that keeps playing out after the gesture stops.
 *   - **The mode is probed once per file**, not chosen per jump: whether an
 *     exact seek is expensive is a property of the file, and switching modes
 *     mid-gesture is visible as a jerk.
 *   - **While a gesture owns the position, `time-pos` must not write `value`** —
 *     and that block has to outlive pointer-up (`settling`) until the final
 *     seek lands, or the knob springs back and then forward again.
 *   - The drag pauses and the scrub does not. They are different gestures: see
 *     `beginDragPause` and `scrubBy`.
 *   - **A shared room hears about a gesture when it ends, never while it runs.**
 *     Exactly the release-only rule the cast seekbar already keeps, and for a
 *     related reason: a drag is a stream of positions, and every one of them
 *     would be a seek on somebody else's machine — dozens of them, each costing
 *     that machine a decode from a keyframe. The two places a gesture *ends* are
 *     `onSeekUp` and `endScrub`, and they are the only two that publish.
 *
 * The room check is written out here rather than borrowed from `playback`'s
 * `refusedByRoom`, and that is the import graph rather than taste: `playback`
 * imports this module, so reaching back for it would be a cycle.
 */

import { command, getProperty, setProperty } from 'tauri-plugin-libmpv-api';

import { cast, castSeek } from './cast.svelte';
import { formatTime } from './format';
import { t } from './i18n.svelte';
import { osdSeq, showOsd } from './osd.svelte';
import { player, waitPlaybackSettled } from './player.svelte';
import { publishState, wire } from './sync/wire.svelte';
import { requestThumb, thumbs } from './thumbs.svelte';

/**
 * What the seek code needs from the frame-stepper, which lives in the page.
 *
 * StepEngine is off by default (`USE_STEP_ENGINE`), so in practice these are
 * inert — but a seek has to cancel a step that is in progress, and wiring that
 * through a hook keeps this module from knowing the sidecar exists.
 */
export interface SeekHooks {
  stepMode: () => boolean;
  cancelStep: () => void;
  flushStepThenCancel: () => void;
  schedulePrewarm: () => void;
  useStepEngine: boolean;
}

let hooks: SeekHooks = {
  stepMode: () => false,
  cancelStep: () => {},
  flushStepThenCancel: () => {},
  schedulePrewarm: () => {},
  useStepEngine: false,
};

export function initSeek(config: SeekHooks) {
  hooks = config;
}

class Seek {
  /// Where the knob sits. Driven by the gesture while one is running, and by
  /// `time-pos` the rest of the time — never by both, which is what `dragging`,
  /// `scrubbing` and `settling` are for.
  value = $state(0);
  dragging = $state(false);
  hoverTime = $state<number | null>(null);
  hoverX = $state(0);
  wrapEl = $state<HTMLElement | null>(null);

  /// Holds the knob at the released position until the final seek lands.
  /// Deliberately not reactive: it gates a write inside the `time-pos` handler
  /// and nothing renders from it.
  settling = false;
  /// A wheel scrub is running.
  scrubbing = false;
  /// Fingers on the trackpad (from NSEvent.phase in Rust, see macos_chrome.rs).
  /// While true the gesture cannot end by timeout: stopping the fingers
  /// mid-seek must not resume playback.
  fingersDown = false;
}

export const seek = new Seek();

/// Set once the drag paused playback, so releasing knows to resume.
let dragResume = false;
/// The drag has moved at all — a plain click must not touch playback.
let seekMoved = false;

function seekFrac(e: PointerEvent): number {
  const el = seek.wrapEl ?? (e.currentTarget as HTMLElement);
  const rect = el.getBoundingClientRect();
  return Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width));
}

// ---- Seek mode selection (shared by the seekbar and the wheel scrub) -----
// Whether an exact seek is expensive is a property of the FILE (codec, GOP
// length, resolution), not of the jump distance. Measured over mpv IPC,
// 30 steps of 2 s:
//
//   HEVC 4K 25fps, GOP 2 s   exact 25 ms    keyframe 25 ms  ← no difference
//   h264 4K 60fps, GOP 2 s   exact 36 ms    keyframe 37 ms  ← none here either
//   VP9  4K 60fps, GOP 5 s   exact 1219 ms  keyframe 50 ms  ← 24x
//
// On a normal file keyframe mode buys nothing, while SWITCHING modes between
// the previews and the final seek is plainly visible as a jerk: the preview
// sits on a keyframe, the final frame on the exact position. So the mode is
// chosen once per file: a fast file gets exact seeks everywhere (no jerks), a
// slow one gets keyframes for previews and exactness only where a frame is
// actually being asked for. IINA works the same way
// (useExactSeekForCurrentFile, 50 ms threshold).
const SLOW_SEEK_MS = 250;
/// The file is deemed "slow" for exact seeks. Reset when the file changes.
let fileSlowSeek = false;
let fileSeekProbed = false;

export function resetSeekProbe() {
  fileSlowSeek = false;
  fileSeekProbed = false;
}

/// Seek with measurement: the first exact seek on a file also measures
/// whether it is affordable here. Returns the completion promise — the
/// follow-up actions (unpausing, unfreezing the knob) hang off it.
/// Record what an exact seek cost on this file. The first measurement sets
/// the verdict; after that it only tightens — one fast seek (a cache hit)
/// must not undo a proven expense.
function noteSeekCost(startedAt: number) {
  const slow = performance.now() - startedAt > SLOW_SEEK_MS;
  if (!fileSeekProbed) {
    fileSeekProbed = true;
    fileSlowSeek = slow;
  } else if (slow) {
    fileSlowSeek = true;
  }
}

export function issueSeek(pos: number, exact: boolean): Promise<unknown> {
  const startedAt = performance.now();
  return command('seek', [pos, exact ? 'absolute+exact' : 'absolute+keyframes'])
    .catch(() => {})
    .finally(() => {
      if (exact) noteSeekCost(startedAt);
    });
}

/// Whether an exact seek is appropriate right now. On a fast file, always; on
/// a slow one only for "show me this frame", never for a preview mid-gesture.
export function wantExact(isPreview: boolean): boolean {
  return !fileSlowSeek || !isPreview;
}

/// The room has handed control to its host and this viewer is not it. Refused at
/// the *start* of the gesture: letting the drag run and then not telling anybody
/// would leave this player somewhere else in the film with nothing to say why.
function roomRefuses(): boolean {
  if (!wire.on || wire.mayDrive) return false;
  showOsd(t('sync.host_only'));
  return true;
}

/// Tell the room where a finished gesture left the film.
function shareSeek(position: number) {
  if (!wire.on) return;
  publishState(player.paused, Math.max(0, position), player.speed);
}

export function onSeekDown(e: PointerEvent) {
  if (!player.hasFile || player.duration <= 0) return;
  if (roomRefuses()) return;
  if (hooks.stepMode()) hooks.cancelStep();
  (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  seek.dragging = true;
  seekMoved = false;
  dragShownAt = null;
  dragShownExact = false;
  seek.value = seekFrac(e) * player.duration;
}

/// Pause for the duration of the drag. Playing video drifts forward between
/// throttled preview seeks and the next preview yanks it back — visible as
/// forward/backward judder, especially on files with sparse keyframes (where
/// the preview keeps returning to the same frame).
///
/// Armed on the first move rather than on pointer-down: a plain click on the
/// knob must not touch playback at all. The wheel scrub, by contrast, does
/// not pause — that gesture self-terminates after 180 ms, and continuing
/// playback is exactly what adds motion between seek landings.
function beginDragPause() {
  // While casting, local playback is already parked; the drag drives the TV
  // and there is nothing here to pause.
  if (cast.remote) return;
  // A fresh value, not the mirror: paused may have gone stale after an mpv
  // event queue overflow.
  void getProperty('pause', 'flag')
    .then((p) => {
      if (!p && seek.dragging) {
        dragResume = true;
        void setProperty('pause', true);
      }
    })
    .catch(() => {});
}

// ---- Drag previews ------------------------------------------------------
// One seek in flight (as in the wheel scrub), not "once every 80 ms". A time
// throttle left the shown frame behind the cursor: on a 57-minute video a
// ~900 px seekbar is 3.8 s per pixel, so lagging even a few pixels means
// seconds, and releasing then jumped the video by that gap. With a pump the
// preview keeps up with the cursor exactly as well as the file allows (25 ms
// on HEVC 4K, 50 ms on "slow" VP9).
let dragInFlight = false;
let dragPending = false;
/// Where the displayed frame actually sits, and how exact it is.
let dragShownAt: number | null = null;
let dragShownExact = false;
let dragSettleTimer: ReturnType<typeof setTimeout> | undefined;
/// Positions equal to within float jitter.
const SAME_POS = 1e-6;

function pumpDragSeek(exact: boolean) {
  // No previews on a cast drag: every preview would be a seek on the TV,
  // seconds of buffering each. The remote gesture is release-only.
  if (cast.remote) return;
  if (dragInFlight) {
    dragPending = true;
    return;
  }
  const target = seek.value;
  // The frame is already here and exact enough — a repeat seek would only blink.
  if (dragShownAt !== null && Math.abs(dragShownAt - target) < SAME_POS && (dragShownExact || !exact)) {
    return;
  }
  dragInFlight = true;
  void issueSeek(target, exact).finally(() => {
    dragShownAt = target;
    dragShownExact = exact;
    dragInFlight = false;
    const again = dragPending;
    dragPending = false;
    if (again && seek.dragging) pumpDragSeek(wantExact(true));
  });
}

/// The cursor stopped — show the EXACT frame of the position it rests on.
/// While the cursor moves, a "slow" file cannot afford this (seconds per
/// seek), but stopping is precisely "I am aiming here": this is the frame the
/// user is studying and expects to see after releasing. It also removes the
/// jump on release — there will be nothing left to settle.
function scheduleDragSettle() {
  clearTimeout(dragSettleTimer);
  dragSettleTimer = setTimeout(() => {
    if (seek.dragging) pumpDragSeek(true);
  }, 120);
}

export function onSeekMove(e: PointerEvent) {
  onSeekHover(e);
  if (!seek.dragging) return;
  if (!seekMoved) beginDragPause();
  seekMoved = true;
  seek.value = seekFrac(e) * player.duration;
  pumpDragSeek(wantExact(true));
  scheduleDragSettle();
}

export function onSeekUp(e: PointerEvent) {
  if (!seek.dragging) return;
  (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
  seek.value = seekFrac(e) * player.duration;
  // The remote path: one SEEK to the TV on release, none of the local
  // machinery — the probe, the exact/keyframe split and the settle are all
  // about local decode cost. cast.time is set optimistically on send, so the
  // knob holds without a seek.settling equivalent.
  if (cast.remote) {
    seek.dragging = false;
    clearTimeout(dragSettleTimer);
    castSeek(seek.value);
    shareSeek(seek.value);
    if (seek.wrapEl && !seek.wrapEl.matches(':hover')) seek.hoverTime = null;
    return;
  }
  // Releasing means "I want this timestamp", so the final seek is always
  // exact — same as a single click. On a fast file the previews were exact
  // too, so the frame is already there and nothing jumps; on a slow one this
  // is the single keyframe → exact transition of the whole gesture.
  //
  // NOTE (architecture.md, the seekbar lesson): it was exactly this trailing
  // exact seek after keyframe previews that once caused a frame blink, and
  // the lesson read "drag is keyframes-only, including the final one". It is
  // deliberately reinstated here: on fast files there is no mode mixing left
  // at all, and on slow ones the price is one transition instead of the knob
  // bouncing backwards.
  seek.dragging = false;
  clearTimeout(dragSettleTimer);
  // Hold the knob at the released position until the seek lands: otherwise
  // time-pos writes the old (preview) position into it and the knob jitters
  // back and forth.
  seek.settling = true;
  // If the exact frame of this position is already shown (the cursor rested
  // before release, so scheduleDragSettle fired), there is nothing to settle:
  // a superfluous seek right here is what looked like "jumping to a different
  // frame".
  const alreadyThere =
    dragShownExact && dragShownAt !== null && Math.abs(dragShownAt - seek.value) < SAME_POS;
  // The room hears the released position, not the landed one, and it hears it
  // now rather than in the `finally` below: `seek.settling` is still true there,
  // and the reconciler stands down while it is — so publishing inside it would
  // announce the seek and then immediately refuse to act on the answer.
  shareSeek(seek.value);
  void (alreadyThere ? Promise.resolve() : issueSeek(seek.value, true)).finally(() => {
    seek.settling = false;
    if (dragResume) {
      dragResume = false;
      void setProperty('pause', false);
    }
  });
  if (player.paused && hooks.useStepEngine) hooks.schedulePrewarm();
  if (seek.wrapEl && !seek.wrapEl.matches(':hover')) seek.hoverTime = null;
}

export function onSeekHover(e: MouseEvent) {
  if (player.duration <= 0) return;
  const wrap = e.currentTarget as HTMLElement;
  const rect = wrap.getBoundingClientRect();
  const frac = Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width));
  seek.hoverTime = frac * player.duration;
  // Clamped by half the popup's width, which depends on whether it carries a
  // thumbnail: keeping the 92px margin for a bare timestamp would stop it
  // following the cursor well before either end of the bar.
  const half = thumbs.available ? 92 : 30;
  seek.hoverX = Math.min(rect.width - half, Math.max(half, e.clientX - rect.left));
  if (thumbs.available) requestThumb(seek.hoverTime, () => seek.hoverTime);
}

export function showSeekOsd(target: number) {
  if (player.duration <= 0) return;
  const pos = Math.min(player.duration, Math.max(0, target));
  showOsd(`${formatTime(pos)} / ${formatTime(player.duration)}`, { progress: pos / player.duration });
}

/// Relative seek with a popup — the shared entry point for arrows and trackpad.
///
/// The coarse step (arrows) goes by keyframes deliberately. A bare `relative`
/// flag obeys `hr-seek=yes` from initialOptions, i.e. it was exact, and an
/// exact seek costs decoding a whole GOP: on 4K60 VP9 with a 5 s keyframe
/// interval that is 1.9 s against 0.05 s (measured over IPC) — the arrow key
/// responded two seconds later. Precision where it is asked for:
/// Shift+arrow (a 1 s step) stays exact.
/**
 * Where a relative seek lands when an A–B loop is running.
 *
 * mpv disarms the loop while the position is past B and re-arms it when the
 * position comes back inside (measured — the marks stay set the whole time),
 * so an arrow key near the end of a segment quietly drops out of it. That is
 * right for a destination ("take me to 42:00") and wrong for a nudge, which
 * means "a bit further along from here" — and here is inside a loop.
 *
 * Modulo rather than a clamp to the start, because a loop is a circle: moving
 * five seconds forward through it should still move five seconds, however
 * short the segment is.
 */
/**
 * Bounds for the wheel scrub: the file, or the A–B segment while one is set.
 *
 * A continuous gesture is CLAMPED where a key press wraps, and the difference
 * is not arbitrary — it is what the two gestures are. Fingers traveling right
 * mean "later", and wrapping would break that correspondence: a steady swipe
 * would spin the picture around a short loop instead of moving through it. A
 * key press has no such correspondence to keep; it is a step, and a step that
 * clamped would go dead at the edge under repeats.
 *
 * Clamping is also what this gesture already does at the ends of the file, so
 * a segment is the same behavior with tighter bounds rather than a new rule
 * to learn.
 *
 * The upper bound stops just short of B: landing exactly on it disarms the
 * loop (mpv arms on `position < B` at seek time), and the gesture would end by
 * silently switching the loop off. A frame short is invisible, and playback
 * wraps from there on its own.
 */
function scrubRange(): { min: number; max: number } {
  if (player.loopA !== null && player.loopB !== null && player.loopB > player.loopA) {
    return { min: player.loopA, max: Math.max(player.loopA, player.loopB - 0.05) };
  }
  return { min: 0, max: player.duration };
}

function wrapInAbLoop(target: number): number | null {
  if (player.loopA === null || player.loopB === null) return null;
  const span = player.loopB - player.loopA;
  if (span <= 0) return null;
  if (target >= player.loopA && target < player.loopB) return null;
  return player.loopA + (((target - player.loopA) % span) + span) % span;
}

export function seekRelative(delta: number, exact = false) {
  if (!player.hasFile) return;
  if (hooks.stepMode()) hooks.flushStepThenCancel();
  const wrapped = wrapInAbLoop(player.timePos + delta);
  if (wrapped !== null) {
    // An absolute seek: the point is precisely that it is no longer `delta`
    // away from here. Exact, like every other "put me on this frame".
    void issueSeek(wrapped, true);
    showSeekOsd(wrapped);
    return;
  }
  // The coarse step used to be keyframes ALWAYS, which is right on a file
  // where an exact seek costs seconds and wrong everywhere else: a keyframe
  // landing is up to a GOP away, so a 5 s arrow moved 6 on an ordinary file
  // and 10 on a network stream with long fragments. It was always doing that
  // — only the popup, which reported the promise rather than the landing,
  // hid it. Now it follows the same probe as the seekbar and the scrub: exact
  // where that is free, keyframes where it is not.
  const wantsExact = exact || !fileSlowSeek;
  const startedAt = performance.now();
  const seek = command('seek', [delta, wantsExact ? 'relative+exact' : 'relative+keyframes'])
    .catch(() => {})
    .finally(() => {
      // Arrows are often the first seek on a file, so they carry the probe
      // too — otherwise a slow file is never diagnosed until the seekbar is
      // touched.
      if (wantsExact) noteSeekCost(startedAt);
    });
  showSeekOsd(player.timePos + delta);
  // A keyframe seek does not land where the optimistic popup promised — fix
  // it up with the actual position once mpv reports it. If the popup changed
  // meanwhile (another key press), the refinement is dropped.
  if (!wantsExact) refineSeekOsd(seek);
}

/**
 * Correct the optimistic popup once the seek has actually landed.
 *
 * A keyframe seek does not stop where the popup promised, so the figure has
 * to be read back — but `command('seek', …)` resolves when mpv ACCEPTS the
 * command, not when it has performed it, and `time-pos` at that moment is
 * still the position before the jump. Reading there made the popup show the
 * target, then flick back to where it started, and stay there: three numbers
 * for one key press. `playback-restart` is the event that means the seek is
 * done, with a timeout so a seek that never completes cannot leave the popup
 * waiting on a promise for ever.
 *
 * The sequence guard stays: with the key held down, an earlier correction
 * must not overwrite a newer press's popup.
 */
function refineSeekOsd(seek: Promise<unknown>) {
  const shown = osdSeq();
  void seek
    .then(() => waitPlaybackSettled(500))
    .then(() => getProperty('time-pos', 'double'))
    .then((pos) => {
      if (typeof pos === 'number' && osdSeq() === shown) showSeekOsd(pos);
    })
    .catch(() => {});
}

/// Jump to a percentage of the file (digit keys, Home/End). Same seek-mode
/// contract as the arrows: exact where it is free, keyframes on a file where
/// an exact seek costs seconds.
export function seekPercent(percent: number) {
  if (!player.hasFile || player.duration <= 0) return;
  if (hooks.stepMode()) hooks.flushStepThenCancel();
  const target = (player.duration * Math.min(100, Math.max(0, percent))) / 100;
  const exact = wantExact(true);
  const seek = issueSeek(target, exact);
  showSeekOsd(target);
  if (!exact) refineSeekOsd(seek);
}

// ---- Horizontal-gesture scrub -------------------------------------------
// The rate is not set by a timer: the next seek only goes out after the
// previous one finished (scrubInFlight). A fixed interval on an expensive
// file built a queue of seeks, and the gesture kept "playing out" after the
// fingers had stopped.
//
// The seek mode is shared with the seekbar (wantExact/issueSeek). On a slow
// file previews go by keyframes: exact ones cost seconds there and turn the
// gesture into 0.8 frames per second. On a fast file there is no difference
// in cost, so everything is exact.
let scrubTarget: number | null = null;
/// We muted the sound for the gesture — restore it afterwards.
let scrubUnmute = false;
let scrubInFlight = false;
let scrubPendingSeek = false;
/// Target of the last seek sent, so the same one is not sent twice.
let scrubSeekedTo: number | null = null;

let scrubEndTimer: ReturnType<typeof setTimeout> | undefined;
// Fingers on the trackpad (from NSEvent.phase in Rust, see macos_chrome.rs).
// While true the gesture cannot end by timeout: stopping the fingers mid-seek
// must not resume playback.

export function scheduleScrubEnd() {
  clearTimeout(scrubEndTimer);
  if (seek.fingersDown) return;
  // The timeout is needed after lifting the fingers too: inertial scrolling
  // follows, and cutting the seek off halfway through it would be wrong.
  scrubEndTimer = setTimeout(endScrub, 180);
}

export function scrubBy(deltaSeconds: number) {
  if (!player.hasFile || player.duration <= 0) return;
  if (roomRefuses()) return;
  const base = scrubTarget ?? player.timePos;
  const range = scrubRange();
  scrubTarget = Math.min(range.max, Math.max(range.min, base + deltaSeconds));

  if (!seek.scrubbing) {
    seek.scrubbing = true;
    // Playback is NOT stopped. Pausing for the gesture was the cause of the
    // "slideshow": the picture froze between seek landings, and on a file
    // with sparse keyframes the gesture came down to a dozen static frames.
    // With the video playing, frames between seeks keep coming and the motion
    // reads as continuous — which is apparently the difference from IINA,
    // which seeks by keyframes on such a file in exactly the same way.
    //
    // The sound is muted for the gesture: 10 seeks per second machine-gun it.
    // A fresh value, not the mirror: mute may have drifted after an mpv event
    // queue overflow.
    void getProperty('mute', 'flag')
      .then((m) => {
        if (!m && seek.scrubbing) {
          scrubUnmute = true;
          void setProperty('mute', true);
        }
      })
      .catch(() => {});
  }

  showSeekOsd(scrubTarget);
  // The gesture drives the knob, not the lagging time-pos (see the 'time-pos'
  // handler).
  seek.value = scrubTarget;
  pumpScrubSeek();
  scheduleScrubEnd();
}

/// Keeps exactly one seek in flight, always aimed at the freshest target.
function pumpScrubSeek() {
  if (scrubInFlight) {
    scrubPendingSeek = true;
    return;
  }
  if (scrubTarget === null || scrubTarget === scrubSeekedTo) {
    // Nothing to catch up with. If the gesture is over, the target is no
    // longer needed: the next one starts from the actual position.
    if (!seek.scrubbing) scrubTarget = null;
    return;
  }
  scrubInFlight = true;
  scrubSeekedTo = scrubTarget;
  // While the gesture runs these are previews (keyframes on a slow file); the
  // last seek after it ends must be exact, since that is the frame that stays.
  void issueSeek(scrubTarget, wantExact(seek.scrubbing))
    .finally(() => {
      scrubInFlight = false;
      scrubPendingSeek = false;
      // The target may have moved while the seek was in flight — and this is
      // also what finishes the last position if the gesture ended mid-seek.
      pumpScrubSeek();
    });
}

export function endScrub() {
  if (!seek.scrubbing) return;
  // The other of the two places a gesture ends. Read before `scrubTarget` is
  // cleared below, and before `scrubbing` goes false — the fingers stopping is
  // what the room is being told about.
  if (scrubTarget !== null) shareSeek(scrubTarget);
  // The frame blink from architecture.md came from MIXING modes (keyframe
  // previews plus an exact tail); here the whole gesture is uniform, and
  // pumpScrubSeek does not replay a target already reached.
  seek.scrubbing = false;
  pumpScrubSeek();
  scrubSeekedTo = null;
  if (scrubUnmute) {
    scrubUnmute = false;
    void setProperty('mute', false);
  }
}
