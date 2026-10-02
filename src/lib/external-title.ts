/**
 * What to call an external track that lives in a subfolder of the release.
 *
 * mpv names an external file by what is left of its file name once the video's
 * name is taken off the front, so a dub at `RUS Sound/StudioA/Show - 01.mka`
 * is called "mka" — as is the one from StudioB beside it, and the track menu
 * offers two identical rows. In a release the folder *is* the name: one folder
 * per studio or per subtitle group. So the folders become the title, minus
 * the ones that only say what kind of file is inside ("RUS Sound", "Subs"),
 * and those give up their language instead when the file name has none.
 *
 * This is also what makes the remembered choice work across a season: a
 * description `{lang, title}` reading "StudioA" matches the next episode's
 * StudioA dub, where "mka" would have matched every dub equally.
 *
 * Pure, and tested, because a wrong answer here is a plausible label on the
 * wrong dub.
 */

import { LANG_ALIASES, LANGUAGES } from './languages';

/// Spellings a release folder uses that are not ISO codes.
const FOLDER_LANG: Record<string, string> = { jap: 'ja', ua: 'uk' };

const KINDS =
  /^(?:sounds?|audio|audios|dub|dubs|dubbing|voices?|voice[\s._-]?over|subs?|subtitles?|tracks?)$/i;

/// A language token as a release spells it, as a 639-1 code — or null.
function langOf(token: string): string | null {
  const t = token.toLowerCase().replace(/^[[(]|[\])]$/g, '');
  if (FOLDER_LANG[t]) return FOLDER_LANG[t];
  if (LANG_ALIASES[t]) return LANG_ALIASES[t];
  return LANGUAGES.some((l) => l.code === t) ? t : null;
}

/**
 * A folder that only says what kind of file is in it, perhaps with a language:
 * "Subs", "RUS Sound", "[ENG] Subtitles", "RUS". Returns its language (or ''
 * when it names none), or null for a folder that is a real name.
 */
export function genericFolder(name: string): string | null {
  const words = name.trim().split(/[\s._-]+/).filter(Boolean);
  if (!words.length) return null;
  const lang = langOf(words[0]);
  const rest = (lang ? words.slice(1) : words).join(' ');
  // A folder that is *only* a language has to look like a tag — "RUS", "[EN]"
  // — or a studio called "It" or "No" would be read as Italian or Norwegian.
  if (!rest) return lang && words[0] === words[0].toUpperCase() ? lang : null;
  return KINDS.test(rest) ? (lang ?? '') : null;
}

function parts(path: string): string[] {
  return path.split(/[\\/]+/).filter(Boolean);
}

function stripExt(name: string): string {
  return name.replace(/\.[^.]+$/, '');
}

/// mpv's own title for an external file without one in its container: the
/// file name with the video's name and the dot after it taken off the front.
export function mpvFallbackTitle(trackPath: string, videoPath: string): string {
  const name = parts(trackPath).at(-1) ?? '';
  const stem = stripExt(parts(videoPath).at(-1) ?? '');
  let title = name.startsWith(stem) ? name.slice(stem.length) : name;
  if (title.startsWith('.')) title = title.slice(1);
  return title;
}

/**
 * The title and language to show for an external track, or null when there is
 * nothing to improve: a file beside the video (mpv's naming is right there),
 * or one whose container carries a title of its own.
 */
export function externalTrackName(
  trackPath: string,
  videoPath: string,
  mpvTitle: string | null,
  mpvLang: string | null,
): { title: string; lang: string | null } | null {
  const track = parts(trackPath);
  const dir = parts(videoPath).slice(0, -1);
  const sameRoot =
    track.length > dir.length + 1 &&
    dir.every((p, i) => p.toLowerCase() === track[i].toLowerCase());
  if (!sameRoot) return null;
  if (mpvTitle && mpvTitle !== mpvFallbackTitle(trackPath, videoPath)) return null;

  const folders = track.slice(dir.length, -1);
  let lang = mpvLang;
  const named: string[] = [];
  for (const f of folders) {
    const generic = genericFolder(f);
    if (generic === null) named.push(f);
    else if (!lang && generic) lang = generic;
  }
  // What is left of the file name, unless it is only a language tag — that is
  // already the language column.
  // The extension goes first: for `<video>.ass` mpv's remainder is "ass",
  // which is the extension and nothing more.
  const name = stripExt(track.at(-1)!);
  const stem = stripExt(parts(videoPath).at(-1) ?? '');
  const rest = (name.startsWith(stem) ? name.slice(stem.length) : name).replace(
    /^[\s._-]+|[\s._-]+$/g,
    '',
  );
  const tail = rest && !langOf(rest) ? rest : '';
  const title = named.join(' / ') || tail || folders.at(-1)!;
  return { title, lang };
}
