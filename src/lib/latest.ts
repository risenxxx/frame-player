/**
 * "Only the newest attempt may write."
 *
 * The player is full of reads that outlive what they were about. `loadTracks`
 * is dozens of sequential round-trips to mpv with several reasons to start one;
 * the torrent poll is a round trip fired every second while the file it asks
 * about can close underneath it; the recents list decodes a poster per card
 * while the viewer opens something. In every one of them the same thing goes
 * wrong the same way: **a slower earlier attempt finishes last and overwrites
 * what the newer one already wrote.**
 *
 * That is a permanent wrong answer rather than a flicker, and clearing the
 * timer or the interval does nothing about it — a promise already in flight
 * resolves regardless, and by then nothing is left to correct it. The measured
 * cases: a stale track list leaving the subtitles button missing, and a
 * `torrent_status` poll caught in the air by closing a film, writing a download
 * readout over the start screen where it then stood for the rest of the run.
 *
 * The fix has always been a counter, and it had been written out by hand six
 * times. It is small enough that copying it looks free, which is exactly why it
 * is worth having once: what a copy loses is never the counter, it is a check
 * after the *second* await — the shape below makes each check one word, so
 * adding the missing one is cheaper than reasoning about whether it is needed.
 *
 * ```ts
 * const trackReads = latest();
 *
 * async function loadTracks() {
 *   const run = trackReads.begin();
 *   const count = await getProperty(…);
 *   if (run.stale) return;          // after every await, not only the last
 *   …
 * }
 * ```
 *
 * It orders attempts and nothing more: the loser is not cancelled, and its work
 * up to the point of the check still happens. What it guarantees is only that
 * the loser does not *publish*.
 */

/** One attempt, which knows whether a newer one has started since. */
export interface Attempt {
  /** A newer attempt has begun — this one must not write anything. */
  readonly stale: boolean;
}

/** A family of attempts at one job. Usually a module-level constant. */
export interface Latest {
  begin(): Attempt;
}

export function latest(): Latest {
  let newest = 0;
  return {
    begin(): Attempt {
      const mine = ++newest;
      // A getter rather than a captured boolean: the whole point is that it is
      // re-read after each await, and a value copied at `begin` would answer
      // the question as it stood before any of the waiting.
      return {
        get stale() {
          return mine !== newest;
        },
      };
    },
  };
}
