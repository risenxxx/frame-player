/**
 * The pure end of playing an audio file: the order an album plays in, and the
 * lyric line as the screen shows it. The state is `audio-screen.svelte.ts`.
 *
 * **The order an album plays in.**
 *
 * The file names usually say it — `01 - …`, `02 - …` — and natural order is
 * what the queue has always used. Where they do not, the tags do: a release
 * named by title alone, or a folder whose names were tidied by hand, plays in
 * alphabetical order of its songs otherwise. So the track numbers win, but only
 * when they settle the whole order: every file numbered, no two the same.
 * Anything less — a folder of singles all tagged track 1, one file without a
 * tag — is a guess, and the names are a better one.
 */

/// `[disc, track]`, as `audio_track_numbers` answers; null where there is none.
export type TrackNumber = [number, number] | null;

/// Natural order, the way a file manager sorts: "ep2" before "ep10".
export const naturalOrder = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

export function albumOrder(
  paths: readonly string[],
  numbers: readonly TrackNumber[],
  name: (path: string) => string,
): string[] {
  const byName = [...paths].sort((a, b) => naturalOrder.compare(name(a), name(b)));
  if (numbers.length !== paths.length) return byName;
  const keyed: { path: string; disc: number; track: number }[] = [];
  const seen = new Set<string>();
  for (const [i, path] of paths.entries()) {
    const n = numbers[i];
    if (!n) return byName;
    const key = `${n[0]}/${n[1]}`;
    if (seen.has(key)) return byName;
    seen.add(key);
    keyed.push({ path, disc: n[0], track: n[1] });
  }
  return keyed.sort((a, b) => a.disc - b.disc || a.track - b.track).map((k) => k.path);
}

/// The lyric line as the audio screen shows it. The same line twice in a row
/// is collapsed: measured once on mpv 0.41, an `.lrc` seeked into straight
/// after it opened reported its line twice over (`"Second line here\nSecond
/// line here"`), and a lyric that sings one line twice at the same moment does
/// not exist.
export function lyricLine(text: string): string {
  const lines = text.split('\n').map((l) => l.trim());
  return lines.filter((l, i) => l !== '' && l !== lines[i - 1]).join('\n');
}
