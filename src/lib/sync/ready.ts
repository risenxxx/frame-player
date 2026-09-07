/**
 * Whether this player is holding the room up.
 *
 * A leaf with the arithmetic and none of the plumbing, for the same reason
 * `drift.ts` is one: **both ways of being wrong here are silent.** Report busy
 * when you are not and an evening stops for a wait that has already ended —
 * bounded only by the relay's 45-second grace, and reported as "the film keeps
 * pausing". Report ready when you are not and the room plays on without you,
 * which is the half a viewer cannot even diagnose: their own window is black and
 * everybody else's film is running.
 *
 * Both had happened, in the same feature, at the same time — which is what a
 * condition written inline in an effect and read by nobody gets you.
 */
import type { ReadyReason } from './protocol';

export interface Readiness {
  /// mpv has a file. **Set the moment a load is *issued*** — `filename` and
  /// `path` are filled in before the demuxer has opened anything — so this is
  /// "something was asked for", not "something is playing".
  hasFile: boolean;
  /// ...and mpv has actually started producing frames for the file it has. The
  /// distinction is the whole of the second bug above: a torrent whose first
  /// byte never arrives satisfies `hasFile` for ever.
  playing: boolean;
  /// Playback stopped waiting for the network (`paused-for-cache`).
  stalled: boolean;
  /// Fetching whatever the room switched to.
  opening: boolean;
  /// That attempt failed and nothing will retry it by itself.
  failed: boolean;
  /// The room is watching something this player cannot open at all — a local
  /// file, or one somebody has hidden.
  unopenable: boolean;
  /// The room is watching something. A viewer sitting on the start screen of a
  /// room where nothing is playing is not holding anybody up — there is nothing
  /// to be ready *for* — and reporting otherwise made creating a room from the
  /// start screen announce "waiting for you" about a wait that did not exist
  /// and could not end.
  roomHasContent: boolean;
}

/**
 * Whether the room may play, and what to tell it we are doing about it.
 *
 * The two answers come out of one function because they are one decision, and
 * because the second is what the first costs everybody else: a member who is
 * not ready freezes the film, and "loading" was all anybody else was told —
 * buffering, still looking for the torrent and having given up on it showed as
 * one word, which is the difference between waiting for somebody and waiting
 * for nothing. The reason travels even where we are ready, for the two cases
 * where the room should stop waiting *and* know why.
 */
export function readinessOf(s: Readiness): { ready: boolean; reason: ReadyReason } {
  // Opening the room's content outranks everything: the film still on screen is
  // the previous one, and it is about to be replaced. It also outranks the
  // previous attempt's verdict, which is what `failed` and `unopenable` are.
  if (s.opening) return { ready: false, reason: 'opening' };
  if (!s.hasFile) {
    // Nothing open. Holding the room until we have what it is watching is the
    // whole point of readiness — but only while that wait can end. Three things
    // end it: there is nothing to open, there is nothing we *can* open, or we
    // tried and could not. The last is the same case as the second wearing a
    // different hat — the magnet will not resolve on the next tick either — and
    // leaving it out is what left a guest reporting "buffering" for the rest of
    // the session after a swarm timed out on them.
    if (!s.roomHasContent) return { ready: true, reason: '' };
    if (s.unopenable) return { ready: true, reason: 'unopenable' };
    if (s.failed) return { ready: true, reason: 'failed' };
    return { ready: false, reason: 'opening' };
  }
  // A file is open: busy until frames are coming, and busy again whenever they
  // stop coming for the network. Nothing is said about the room's own content
  // here even after a failure — this viewer is watching something, and what the
  // others need from them is a position, not an excuse.
  if (!s.playing || s.stalled) return { ready: false, reason: 'buffering' };
  return { ready: true, reason: '' };
}

/// The half of `readinessOf` most of the tests are about.
export function isBusy(s: Readiness): boolean {
  return !readinessOf(s).ready;
}
