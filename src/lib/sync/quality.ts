/**
 * How well each member is keeping up with the room, as one word.
 *
 * Two different things can make a member fall out of step, and the dot has to
 * say which, because they have different answers:
 *
 *   - **the network** — the round trip to the relay is long, uneven, or losing
 *     pings. That limits how tightly the room *can* be held (the deadband is
 *     derived from it, see `deadbandFor`) and it is nobody's fault in the room;
 *   - **playback** — the network to the relay is fine, but this player keeps
 *     having to be hauled back: it stalls on its source, or drifts so far that
 *     the reconciler gives up on bending the speed and seeks. That is a torrent
 *     starving or a machine that cannot decode the file in real time.
 *
 * Both are judged here, by the member about themselves, and only the verdict
 * travels: the relay stores the word without reading it (the rule `reason` and
 * `ContentRef` follow), so a new word is a frontend change and never a
 * redeploy, and a word this build has never heard of shows no dot rather than a
 * wrong one.
 *
 * A leaf with no plumbing, because every mistake here is a plausible colour:
 * a dot that is yellow for two minutes after every remote seek looks exactly
 * like a dot that works.
 */

/** Green, yellow, red. */
export type QualityLevel = 'good' | 'fair' | 'poor';
/** Which of the two things is the trouble, when there is any. */
export type QualityCause = 'net' | 'play';

/**
 * What travels in `Member.link`. One word rather than two fields, because the
 * cause means nothing without a level and the level alone cannot be acted on.
 */
export type LinkWord = 'good' | 'fair-net' | 'fair-play' | 'poor-net' | 'poor-play';

const WORDS: ReadonlySet<string> = new Set<LinkWord>([
  'good',
  'fair-net',
  'fair-play',
  'poor-net',
  'poor-play',
]);

/** A word off the wire, or null for one this build does not know. */
export function parseLink(word: string | undefined): LinkWord | null {
  return word && WORDS.has(word) ? (word as LinkWord) : null;
}

export function levelOf(word: LinkWord): QualityLevel {
  return word === 'good' ? 'good' : word.startsWith('poor') ? 'poor' : 'fair';
}

export function causeOf(word: LinkWord): QualityCause | null {
  return word === 'good' ? null : word.endsWith('net') ? 'net' : 'play';
}

// ---- the network ------------------------------------------------------------

/**
 * Fewer samples than this and there is nothing to say yet. The clock estimator
 * sends eight pings half a second apart on joining, so this is the first two
 * seconds of a session — a dot that appears then is honest, one that appears
 * after a single round trip is a guess.
 */
export const MIN_SAMPLES = 3;

/**
 * The thresholds, and where they come from.
 *
 * **The round trip** is tied to the drift band rather than picked: the band is
 * twice the clock uncertainty, which is half the fastest round trip, clamped at
 * 150 ms (`MAX_DEADBAND_S`). So up to a 150 ms round trip the room is held as
 * tightly as the arithmetic allows — that is "good" — and past it the band is
 * at its ceiling and the network is what limits the room. 400 ms is where a
 * pause pressed by somebody else is visibly late on this screen.
 *
 * **The spread** (median round trip minus the fastest) is the jitter. It does
 * not move the estimate much — the estimator takes the fastest half precisely
 * to ignore it — but it is what a connection about to drop looks like, and
 * Wi-Fi in the next room looks like it first.
 *
 * **A lost ping** is one with no answer after `LOST_AFTER_MS`. Over TCP nothing
 * is lost outright; an unanswered ping is a connection stalled for seconds,
 * which is worse news than any round trip.
 *
 * These are chosen, not measured against a population of networks — they are
 * where the consequences above begin, which is the part that matters.
 */
export const NET = {
  fairRtt: 150,
  poorRtt: 400,
  fairSpread: 100,
  poorSpread: 300,
  fairLost: 1,
  poorLost: 2,
} as const;

/// How long a ping may go unanswered before it counts as lost.
export const LOST_AFTER_MS = 5000;

export interface NetStats {
  /** Recent round trips, milliseconds, oldest first. */
  rtts: readonly number[];
  /** Pings in the same window that were never answered. */
  lost: number;
}

/** The network's verdict, or null while there are too few samples. */
export function netLevel(s: NetStats): QualityLevel | null {
  if (s.rtts.length < MIN_SAMPLES) return null;
  const sorted = [...s.rtts].sort((a, b) => a - b);
  const best = sorted[0];
  const spread = sorted[sorted.length >> 1] - best;
  if (best > NET.poorRtt || spread > NET.poorSpread || s.lost >= NET.poorLost) return 'poor';
  if (best > NET.fairRtt || spread > NET.fairSpread || s.lost >= NET.fairLost) return 'fair';
  return 'good';
}

/** The figures a tooltip shows: the fastest round trip and the spread. */
export function netFigures(rtts: readonly number[]): { rtt: number; spread: number } | null {
  if (rtts.length === 0) return null;
  const sorted = [...rtts].sort((a, b) => a - b);
  return { rtt: Math.round(sorted[0]), spread: Math.round(sorted[sorted.length >> 1] - sorted[0]) };
}

// ---- playback ---------------------------------------------------------------

/**
 * How far back a disruption still counts. Two minutes: long enough that a
 * torrent stalling every minute reads as the problem it is, short enough that
 * one bad moment at the start of an episode is forgotten by the middle.
 */
export const DISRUPTION_WINDOW_MS = 120_000;

/**
 * The verdict from disruptions — a stall on the source, or a seek the
 * reconciler had to make because bending the speed was not enough.
 *
 * **What counts as a disruption is the caller's to filter, and the filter is
 * the hard part**: the reconciler also seeks every time *somebody else* seeks,
 * and right after every file opens — both are the room working, not this
 * member failing. See `noteDisruption` in `apply.svelte.ts`.
 */
export function playLevel(disruptions: readonly number[], now: number): QualityLevel {
  const recent = disruptions.filter((at) => now - at <= DISRUPTION_WINDOW_MS).length;
  if (recent >= 2) return 'poor';
  if (recent === 1) return 'fair';
  return 'good';
}

/** Drop what has aged out, so the list cannot grow for the length of a film. */
export function pruneDisruptions(disruptions: readonly number[], now: number): number[] {
  return disruptions.filter((at) => now - at <= DISRUPTION_WINDOW_MS);
}

// ---- together ---------------------------------------------------------------

const RANK: Record<QualityLevel, number> = { good: 0, fair: 1, poor: 2 };

/**
 * The word for both halves: the worse of the two, and its cause.
 *
 * Network first on a tie, because a bad network is the likelier reason for the
 * playback trouble than the other way round — a member on a 600 ms round trip
 * who also seeks is seeking *because* of it.
 */
export function combine(net: QualityLevel | null, play: QualityLevel): LinkWord | null {
  if (net === null) return null;
  if (net === 'good' && play === 'good') return 'good';
  return RANK[net] >= RANK[play] ? (`${net}-net` as LinkWord) : (`${play}-play` as LinkWord);
}

/**
 * How long a better word has to hold before it is reported.
 *
 * Worse is reported at once — that is news — and better waits, because a value
 * sitting on a threshold would otherwise flip the dot (and broadcast a member
 * list to the whole room) every few seconds.
 */
export const UPGRADE_HOLD_MS = 20_000;

export interface Settled {
  /** What is being shown. */
  word: LinkWord | null;
  /** A better word waiting out `UPGRADE_HOLD_MS`, and since when. */
  pending: LinkWord | null;
  since: number;
}

export const UNSETTLED: Settled = { word: null, pending: null, since: 0 };

/** Fold one fresh verdict into what is shown. */
export function settle(prev: Settled, next: LinkWord | null, now: number): Settled {
  // Nothing to say yet, or the session started again: no hysteresis to keep.
  if (next === null || prev.word === null) return { word: next, pending: null, since: now };
  if (next === prev.word) return { word: next, pending: null, since: now };
  const worse = RANK[levelOf(next)] > RANK[levelOf(prev.word)];
  // A different cause at the same level is news too: it changes what the
  // tooltip tells somebody to do.
  const sameLevel = levelOf(next) === levelOf(prev.word);
  if (worse || sameLevel) return { word: next, pending: null, since: now };
  if (prev.pending !== next) return { word: prev.word, pending: next, since: now };
  if (now - prev.since >= UPGRADE_HOLD_MS) return { word: next, pending: null, since: now };
  return prev;
}
