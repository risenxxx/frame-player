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

export function isBusy(s: Readiness): boolean {
  // Opening the room's content outranks everything: the film still on screen is
  // the previous one, and it is about to be replaced.
  if (s.opening) return true;
  if (!s.hasFile) {
    // Nothing open, and about to open what the room is watching — holding the
    // room until then is the whole point of readiness. Three things end that
    // wait: there is nothing to open, there is nothing we *can* open, or we
    // tried and could not. The last one is the same case as the second wearing
    // a different hat — the magnet will not resolve on the next tick either —
    // and leaving it out is what left a guest reporting "buffering" for the
    // rest of the session after a swarm timed out on them.
    return s.roomHasContent && !s.unopenable && !s.failed;
  }
  // A file is open: busy until frames are coming, and busy again whenever they
  // stop coming for the network.
  return !s.playing || s.stalled;
}
