/**
 * The end of a file, and the offers the player makes around it.
 *
 * `keep-open=always` parks mpv on the last frame of *every* entry rather than
 * only the last one, so the moment a file ends belongs to us: the end screen,
 * the countdown, and the wrap of a repeating queue — which mpv's own
 * `loop-playlist` can no longer perform, because mpv never advances at all.
 * Repeat-one never gets here: mpv loops the file internally and eof is never
 * reached.
 *
 * The skip button lives here too, and not because it is the end of the file —
 * an opening is at the beginning. It is here because it is the same kind of
 * thing: something the player offers and the viewer may take, `Enter` answers
 * either of them, and on the last chapter the skip offer *becomes* the end
 * screen's next entry. Splitting them would put that hand-off across a module
 * boundary.
 *
 * Like the window shell, this takes its inputs rather than reaching for them:
 * `chrome.overlayOpen` is how it knows a menu is covering the video, so it never
 * has to import a dialog.
 */

import { cast, castFollow, castSeek } from './cast.svelte';
import { chrome } from './chrome.svelte';
import { chapterAt, player, seekChapter, skipKind } from './player.svelte';
import { ADVANCE_MS, ensurePoster, neighbour, playEntry, playlist } from './playlist.svelte';
import { step } from './step-engine.svelte';

/// Fetch the poster this many seconds before the end, so the card is not blank
/// at the moment it appears. One decode, and only when a queue exists.
const POSTER_LEAD = 20;

/// How long a skip offer stays up, in seconds — bounded at both ends and never
/// a bare constant. Longer than the few seconds it started at, because the
/// decision lands when you recognise the music rather than at the first frame,
/// and a window that short took the offer away while its reason was still on
/// screen. Shorter than the chapter itself, because an offer that sits over the
/// picture for a whole minute stops reading as an offer. And never longer than
/// the chapter: a five-second bumper must not leave a button hanging for three
/// times as long as the thing it offers to skip — which also keeps a title the
/// detector misreads ("Introduction" on a two-hour lecture) from producing
/// anything worse than a brief button.
const SKIP_MAX_WINDOW = 20;

/// The last moments are faded out rather than cut, so the button leaves the way
/// it arrived. In seconds, matched by the CSS transition.
const SKIP_FADE = 0.6;

class EndOfFile {
  /// The countdown to the next entry is running.
  advancing = $state(false);
  /// Bumped per countdown so the CSS bar restarts instead of inheriting a
  /// finished animation.
  seq = $state(0);

  /// Chapter whose button has already been used, muted until playback is
  /// genuinely elsewhere. Without it the button lingers for the ~100 ms until
  /// mpv reports the new position, which is long enough for a second click to
  /// skip a chapter the viewer never saw. It is a race guard and nothing more:
  /// coming back to an opening on purpose has to offer the button again.
  skipUsed = $state(-1);

  ended = $derived(player.hasFile && player.eofReached);
  next = $derived(this.ended ? neighbour(1) : null);
  prev = $derived(this.ended ? neighbour(-1) : null);

  hint = $derived.by(() => {
    if (!player.hasFile || !player.hasChapters || step.on) return null;
    if (chrome.overlayOpen) return null;
    // **Whose clock.** While casting, the local player is parked paused on the
    // frame it handed over, so every one of these readings is frozen and the
    // button never appears — until something moves mpv, and then it appears at
    // the wrong moment. The television's position is the one the viewer is
    // watching, so it is the one the offer is measured against.
    const now = cast.remote ? cast.time : player.timePos;
    // From the position rather than the `chapter` mirror: the mirror lags a
    // seek, and the elapsed time below has to be measured against the same
    // chapter the button would skip.
    const here = chapterAt(now);
    if (!here || here.index === this.skipUsed) return null;
    const kind = skipKind(here);
    if (!kind) return null;
    // Where the offer leads. Inside the file it is the next chapter; on the
    // LAST chapter — closing credits, nearly always — there is nowhere left to
    // seek, so the offer becomes the next entry in the queue. Without one there
    // is nothing to propose and no button.
    //
    // **And the next entry has to be a different file.** `neighbour` answers
    // "what plays next", which under repeat is legitimately *this* file again:
    // with one entry and repeat-all it wraps straight back to index 0, and it
    // does the same from a stale `playlist-pos` of -1 (mirrors go stale, see
    // gotcha 3). Either way the credits of a lone episode grew a "Следующая
    // серия" button that restarted what was already playing. The two questions
    // are genuinely different — repeating one file is what "repeat all" on a
    // one-entry queue means, so the fix belongs here and not in `neighbour` —
    // and it is settled two ways because they fail differently: the index is
    // the thing that is stale in the second case, and `filePath` is mpv's own
    // answer to "what is open". Either of them calling it the same file is
    // enough to withdraw the offer — a queue holding one file twice is the only
    // thing that costs, and suppressing a button that would reopen an identical
    // file is not a loss.
    const chapter = player.chapters[here.index + 1] ?? null;
    const after = chapter ? null : neighbour(1);
    const sameFile =
      after && (after.path === player.filePath || after.index === player.playlistPos);
    const entry = after && !sameFile ? after : null;
    if (!chapter && !entry) return null;
    // The last chapter ends where the file does.
    const ends = chapter ? chapter.time : cast.remote ? cast.duration : player.duration;
    if (ends <= here.time) return null;
    const span = Math.min(ends - here.time, SKIP_MAX_WINDOW);
    const left = here.time + span - now;
    if (left <= 0 || left > span) return null;
    // Both readouts come from the position rather than from a CSS clock, so
    // they survive a seek into the middle of the chapter: the bar shows what is
    // actually left, and pausing freezes it for free (time-pos simply stops).
    return {
      kind,
      from: here.index,
      chapter,
      entry,
      left: left / span,
      fade: Math.min(1, left / SKIP_FADE),
    };
  });
}

export const endOfFile = new EndOfFile();

let advanceTimer: ReturnType<typeof setTimeout> | undefined;
/// This end has been dealt with. `eof-reached` can surface more than once for
/// one ending — the property event and `resyncState` both write the mirror —
/// and a second pass would silently restart the countdown from five.
let endHandled = false;

/// Take the skip offer — from the button or from Enter, which is why it is a
/// function and not an inline handler.
///
/// The hint is read ONCE, into a local, before anything it depends on is
/// touched. A $derived is re-evaluated the moment it is read again rather than
/// at the end of the tick, so `skipUsed = hint.from` followed by `hint.chapter`
/// threw on that second read: the button vanished (its block had unmounted) and
/// nothing else happened at all.
export function takeSkip() {
  const hint = endOfFile.hint;
  if (!hint) return;
  endOfFile.skipUsed = hint.from;
  // Both halves follow the session: a chapter jump is a remote seek, and the
  // next episode goes to the television rather than starting here.
  if (hint.chapter) {
    if (cast.remote) castSeek(hint.chapter.time);
    else seekChapter(hint.chapter.index);
  } else if (hint.entry) {
    if (cast.remote) void castFollow(hint.entry);
    else void playEntry(hint.entry);
  }
}

export function cancelAdvance() {
  clearTimeout(advanceTimer);
  endOfFile.advancing = false;
}

/// The file ended. Decide between waiting and rolling on, and get the cards
/// something to show either way.
export function onReachedEnd() {
  if (endHandled) return;
  endHandled = true;
  const next = neighbour(1);
  const prev = neighbour(-1);
  // Sequential, because a poster is a keyframe decode and two at once is the
  // greed already cured in the storyboard.
  void (async () => {
    if (next) await ensurePoster(next.path);
    if (prev) await ensurePoster(prev.path);
  })();
  if (!next || !playlist.autoAdvance) return;
  endOfFile.advancing = true;
  endOfFile.seq++;
  clearTimeout(advanceTimer);
  advanceTimer = setTimeout(() => {
    endOfFile.advancing = false;
    // Re-read rather than closing over `next`: the countdown outlives several
    // frames, and the queue can be edited from the panel while it runs.
    const target = neighbour(1);
    if (target) void playEntry(target);
  }, ADVANCE_MS);
}

/// A new file, or a rewind out of the ending. Both mean the next end has not
/// been handled yet.
export function resetEnd() {
  endHandled = false;
  cancelAdvance();
}

/// Chapter indices mean nothing across files — a stale one would silently
/// suppress the skip button in the new file.
export function resetSkipGuard() {
  endOfFile.skipUsed = -1;
}

/// Release the skip button's mute once the position has actually left the
/// chapter it was used on. Called from the `time-pos` handler rather than an
/// $effect for the usual reason: this must run on a position report, not
/// whenever one of the values it reads happens to change.
export function noteLocalPosition() {
  if (endOfFile.skipUsed >= 0 && chapterAt(player.timePos)?.index !== endOfFile.skipUsed) {
    endOfFile.skipUsed = -1;
  }
  // Decode the next entry's poster shortly before it is needed: it is a
  // keyframe decode, and starting it when the end screen is already up means
  // the card appears empty and fills in afterwards. `ensurePoster` is a no-op
  // once cached, so calling it per report costs nothing.
  if (player.duration > 0 && player.duration - player.timePos < POSTER_LEAD) {
    const soon = neighbour(1);
    if (soon) void ensurePoster(soon.path);
  }
}

/**
 * The casting twin of `noteLocalPosition`'s first half.
 *
 * Its local counterpart cannot serve here: while casting, mpv is paused, so no
 * position report ever arrives and the guard would hold for the rest of the
 * session — one skip per episode, then nothing.
 *
 * Started from the page rather than left at this module's top level, for the
 * reason spelled out on `initChrome`: a `$effect` written at module scope
 * throws `effect_orphan` on import, and nothing in the toolchain notices.
 */
export function initEndScreen() {
  $effect(() => {
    if (!cast.remote) return;
    if (endOfFile.skipUsed >= 0 && chapterAt(cast.time)?.index !== endOfFile.skipUsed) {
      endOfFile.skipUsed = -1;
    }
  });
}
