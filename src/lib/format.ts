/** Pure formatting and path helpers. No state, no imports, no side effects. */

/** `1:02:03` / `2:03` — hours only when there are any. */
export function formatTime(seconds: number): string {
  let s = seconds;
  if (!Number.isFinite(s) || s < 0) s = 0;
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = Math.floor(s % 60);
  const mm = String(m).padStart(2, '0');
  const ss = String(sec).padStart(2, '0');
  return h > 0 ? `${h}:${mm}:${ss}` : `${m}:${ss}`;
}

/** Same, plus milliseconds — the frame-step badge needs to name a single frame. */
export function formatTimeMs(seconds: number): string {
  const ms = Math.floor((Math.max(0, seconds) % 1) * 1000);
  return `${formatTime(seconds)}.${String(ms).padStart(3, '0')}`;
}

/** Last path component. Both separators, because paths arrive from either OS. */
export function baseName(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

/**
 * One spelling of a path, for comparing two of them.
 *
 * Case and separator direction are both normalized, and neither is optional.
 * The two sides of any comparison in this player come from *different places* —
 * a folder chosen in the OS dialog, whatever mpv reports as `path`, a directory
 * read in Rust — and on Windows they disagree about the slash: one mixed
 * `E:/Videos\ep1.mkv` is enough to lose a whole queue.
 *
 * Deliberately not `resolve`/`realpath`: this is a string comparison and must
 * stay one. Both callers compare identifiers they were given rather than asking
 * the file system anything, and a symlink two viewers reach by different names
 * is not a case either of them has to answer.
 *
 * The rule is mirrored in Rust (`path_under` in thumb_service.rs). The two are
 * pinned to one another by `shared/path-under.txt`, which both test suites read.
 */
export function normalizePath(path: string): string {
  return path.toLowerCase().replace(/\\/g, '/');
}

/**
 * The same file, however the two names were spelled?
 *
 * Used to find an opened file inside a directory listing. Getting it wrong is
 * silent: `findIndex` answers −1 and the folder queue is simply never built.
 */
export function samePath(a: string, b: string): boolean {
  return normalizePath(a) === normalizePath(b);
}

/**
 * Is `path` inside `root`?
 *
 * Matching is on a **component boundary**, so `/Movies` does not swallow
 * `/Movies2` — this decides whether a privacy root applies, and a root that
 * matches too much hides files the viewer never excluded while one that matches
 * too little is a leak. An empty root (which is what `/` collapses to once its
 * trailing separators are trimmed) matches nothing at all: "everything is
 * private" is a separate flag, never a root shaped like one.
 */
export function pathUnder(path: string, root: string): boolean {
  const r = normalizePath(root).replace(/\/+$/, '');
  if (r === '') return false;
  const p = normalizePath(path);
  return p === r || p.startsWith(`${r}/`);
}

/** Lower-case extension without the dot; empty for a dotfile or no extension. */
export function extensionOf(path: string): string {
  const name = baseName(path);
  const dot = name.lastIndexOf('.');
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : '';
}

/** File-system-safe stem of a file name, for naming exported frames. */
export function fileStem(name: string): string {
  return name.replace(/\.[^.]+$/, '').replace(/[\\/:*?"<>|]/g, '_').slice(0, 80) || 'frame';
}

/// Percent-decoding that never throws. A lone `%` in a file name is legal on
/// disk and fatal to `decodeURIComponent`, and a name is not worth an exception.
function decodeMaybe(s: string): string {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
}

/**
 * A file name fit to read, for a list that has only the path to go on.
 *
 * Release names are written for sorting, not for people — dots where the spaces
 * belong. This is the fallback, though: anything actually watched carries the
 * title the player itself showed (mpv's `media-title`, which comes from the
 * container's metadata, or from yt-dlp for a link), and that is always better
 * than anything guessable from a file name.
 *
 * A URL loses its query and fragment, or a shared YouTube link shows up as
 * `TfUP5fQcbCM?si=ykE0V6YNxD9bQ3bz`, and it is percent-decoded — a torrent
 * stream is served from a URL we build ourselves out of the file's name, so
 * without this a perfectly ordinary film reads as `The%20Movie%202024`.
 *
 * A magnet is the one input with no path worth showing at all: what it has is a
 * 40-character hash, and what it carries for exactly this purpose is `dn`.
 */
export function displayName(path: string): string {
  const trimmed = path.trim();
  if (/^magnet:/i.test(trimmed)) {
    const dn = /[?&]dn=([^&]*)/i.exec(trimmed)?.[1];
    // `+` for a space is the form-encoding convention magnet links inherit; a
    // real plus in a title arrives as %2B and survives the replacement.
    if (dn) return decodeMaybe(dn.replace(/\+/g, ' ')).trim() || trimmed;
    return trimmed;
  }
  const clean = trimmed.split(/[?#]/)[0];
  const base = decodeMaybe(baseName(clean));
  const name = base.replace(/\.[^.]+$/, '');
  // Only between non-space characters: a name that already uses spaces is
  // left alone, and a leading dot is not a separator.
  return name.replace(/(?<=\S)\.(?=\S)/g, ' ').trim() || base;
}

/**
 * A link written out for a human to read, for the tooltip on a recents row.
 *
 * Two different problems, one per shape. A URL is percent-encoded, so a
 * Cyrillic or accented path arrives as a run of `%D0%B1` and the tooltip — whose
 * whole job is "which link is this" — became the least readable thing in the
 * dialog. And a magnet is not long because it says a lot: it is a hash plus a
 * name plus a *tracker list*, and trackers are how a client finds peers, not how
 * a person tells two magnets apart. So the trackers go, and what is left is the
 * name and the hash — the name because it is what the film is called, the hash
 * because it is the identity, and two rips of the same film differ by nothing
 * else. Deliberately not the same string as the row's own label, which is the
 * name alone: a tooltip that repeats its label is noise.
 */
export function readableLink(src: string): string {
  const s = src.trim();
  if (/^magnet:/i.test(s)) {
    const hash = /xt=urn:btih:([0-9a-z]+)/i.exec(s)?.[1];
    const dn = /[?&]dn=([^&]*)/i.exec(s)?.[1];
    const name = dn ? decodeMaybe(dn.replace(/\+/g, ' ')).trim() : '';
    return [name, hash && `btih:${hash.toLowerCase()}`].filter(Boolean).join(' · ') || s;
  }
  return decodeMaybe(s);
}

export type EpisodeRef = { title: string; season: number; episode: number };

/**
 * Season and episode from a file name, plus what is left of the show's title.
 *
 * Subtitle search needs this because a title alone reaches the wrong thing:
 * measured against OpenSubtitles, "the day of the jackal" returns the **1973
 * film**, while the same query with `season_number=1&episode_number=3` returns
 * 122 subtitles for the 2024 series. The numbers are the difference between
 * "nothing found" and a usable list for every series.
 *
 * Only unambiguous markers are recognized, and that restraint is the whole
 * design — the same lesson the chapter-skip patterns learned. A bare number in
 * a release name is not an episode: `1080p`, `2024`, `x264` and `5.1` are all
 * numbers sitting in exactly the places a naive parser would read one. So:
 * `S01E03` and its spacings, the spelled-out forms, and `1x03` — which is
 * bounded on both sides by non-digits precisely so `1920x1080` cannot produce
 * "season 20, episode 10".
 */
const EPISODE_PATTERNS: RegExp[] = [
  // S01E03, s1.e3, S01 E03. The leading boundary keeps it from matching inside
  // a word ending in "s".
  /(?:^|[^a-z0-9])s(\d{1,2})[ ._-]*e(\d{1,3})(?![0-9])/i,
  /(?:^|[^a-z0-9])season[ ._-]*(\d{1,2})[ ._-]*episode[ ._-]*(\d{1,3})(?![0-9])/i,
  /(?:^|[^a-z0-9])сезон[ ._-]*(\d{1,2})[ ._-]*серия[ ._-]*(\d{1,3})(?![0-9])/i,
  // Last, because it is the one with a plausible false positive.
  /(?:^|[^a-z0-9])(\d{1,2})x(\d{1,3})(?![0-9])/i,
];

export function parseEpisode(path: string): EpisodeRef | null {
  const base = baseName(path.split(/[?#]/)[0]).replace(/\.[^.]+$/, '');
  for (const pattern of EPISODE_PATTERNS) {
    const match = pattern.exec(base);
    if (!match) continue;
    const season = Number(match[1]);
    const episode = Number(match[2]);
    // Season 0 is where specials live and is legitimate; episode 0 is not.
    if (!Number.isFinite(season) || !Number.isFinite(episode) || episode < 1) continue;
    // What precedes the marker is the show. Empty when the name opens with it,
    // and then the caller keeps whatever title it already had.
    const title = base
      .slice(0, match.index)
      .replace(/[._]+/g, ' ')
      .replace(/\s+/g, ' ')
      .replace(/[\s\-–—]+$/, '')
      .trim();
    return { title, season, episode };
  }
  return null;
}

/**
 * Timestamp for an exported frame: the position inside the file, not the wall
 * clock, so two frames grabbed from one film sort in playback order.
 */
export function shotStamp(seconds: number): string {
  const s = Math.max(0, seconds);
  const pad = (n: number, width = 2) => String(Math.floor(n)).padStart(width, '0');
  return `${pad(s / 3600)}-${pad((s % 3600) / 60)}-${pad(s % 60)}.${pad((s % 1) * 1000, 3)}`;
}
