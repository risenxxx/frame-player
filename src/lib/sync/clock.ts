/**
 * How far this machine's clock is from the relay's.
 *
 * Everything about a shared timeline is stated in relay time — `at` is a relay
 * timestamp, and the position each viewer should be at is projected forward from
 * it — so this offset is the one number that makes the whole scheme work. Get it
 * wrong by 300 ms and every viewer is 300 ms out with nothing on screen to say
 * why: the drift correction will dutifully hold them there, because as far as it
 * can tell they are exactly where they should be.
 *
 * The estimator is the ordinary one (it is what NTP does in miniature, and what
 * every synchronised-playback tool ends up with), and its two decisions are both
 * about *asymmetry*. A round trip tells you `offset = s + rtt/2 - now` only if
 * the two legs took the same time; when they did not, the error is up to half
 * the difference. So:
 *
 *   - **keep several samples and prefer the fastest.** A slow round trip is
 *     asymmetric far more often than a fast one — congestion delays one direction
 *     — so averaging everything imports exactly the noise worth discarding.
 *   - **take the median rather than the mean** of what is left, because one
 *     retransmission is enough to drag a mean and not enough to move a median.
 *
 * Pure and testable on purpose: this is arithmetic whose wrong answer is silent,
 * which is the standard this project's test suite is chosen by.
 */

/** One completed round trip. */
export interface Sample {
  /** Milliseconds there and back, by our own clock. */
  rtt: number;
  /** Relay clock minus ours, in milliseconds, as this sample saw it. */
  offset: number;
}

/**
 * How many round trips are kept. Eight is enough for the median of the fastest
 * half to mean something, and short enough that the window follows a route that
 * changes — a laptop moving from Wi-Fi to a hotspot mid-film.
 */
export const SAMPLE_WINDOW = 8;

/**
 * Turn one ping/pong into a sample.
 *
 * `sentAt` is our own reading when the ping went out and comes back untouched in
 * the pong, which is why the relay has to remember nothing and a lost pong costs
 * one sample rather than a wrong offset.
 *
 * Returns null for a round trip that cannot be true — a negative one, which a
 * clock stepped mid-flight (NTP, a laptop waking) produces.
 */
export function sampleOf(sentAt: number, serverAt: number, now: number): Sample | null {
  const rtt = now - sentAt;
  if (!Number.isFinite(rtt) || rtt < 0) return null;
  return { rtt, offset: serverAt + rtt / 2 - now };
}

/** Add a sample, dropping the oldest past the window. */
export function pushSample(samples: readonly Sample[], next: Sample): Sample[] {
  const out = [...samples, next];
  return out.length > SAMPLE_WINDOW ? out.slice(out.length - SAMPLE_WINDOW) : out;
}

/**
 * The offset to use: the median of the fastest half of the samples.
 *
 * Zero for an empty window, which is the honest answer before the first round
 * trip completes — the caller has the relay's own `now` from the handshake by
 * then, and uses that until this has something to say.
 */
export function estimateOffset(samples: readonly Sample[]): number {
  if (samples.length === 0) return 0;
  const byRtt = [...samples].sort((a, b) => a.rtt - b.rtt);
  const best = byRtt.slice(0, Math.max(1, Math.floor(byRtt.length / 2)));
  const offsets = best.map((s) => s.offset).sort((a, b) => a - b);
  return median(offsets);
}

/**
 * How far the estimate can be wrong, as half the fastest round trip.
 *
 * Shown to the viewer rather than used in the arithmetic, and it earns its place
 * by answering the question people actually ask when a room feels out of step:
 * whether the problem is the film, the network, or somebody's finger. A relay
 * 200 ms away cannot hold a room tighter than about 100 ms, and saying so is the
 * difference between a known limit and a bug.
 */
export function offsetUncertainty(samples: readonly Sample[]): number {
  if (samples.length === 0) return Infinity;
  return Math.min(...samples.map((s) => s.rtt)) / 2;
}

/**
 * The relay's clock as this machine sees it, in **whole** milliseconds.
 *
 * The rounding is the whole point of this function existing. `at` is an `int64`
 * on the relay, and Go refuses to unmarshal `1786313164655.5` into one — the
 * *entire message* fails to decode and comes back as `bad_message`. The offset
 * is a median of estimates, so it is fractional almost always: half of all
 * round trips take an odd number of milliseconds, and an even sample window
 * averages its two middle values.
 *
 * It reached a viewer because it cannot happen on loopback, where the offset is
 * an integer zero — so it passed every local test and waited for a real
 * network, where it refused every publish and threw the viewer out of the room
 * mid-film.
 */
export function relayClock(localNow: number, offset: number): number {
  return Math.round(localNow + offset);
}

function median(sorted: readonly number[]): number {
  const n = sorted.length;
  if (n === 0) return 0;
  const mid = n >> 1;
  // An even window has no middle element, and picking either one makes the
  // estimate jump by the gap between them whenever a sample ages out.
  return n % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}
