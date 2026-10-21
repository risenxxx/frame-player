/**
 * What the audio and subtitle buttons say about the file before they are
 * opened: one small tag in the button's lower-right corner.
 *
 * The two buttons are not alike, and that decides what each tag carries. The
 * audio button exists only when a file has two tracks or more, so its presence
 * already says "there is a choice"; what it cannot say is *which* one is
 * playing, and that is the question when a film starts — dubbed or original.
 * The subtitle button is always there, because it is also the way into the
 * search, so on its own it says nothing at all: neither whether the file has
 * subtitles nor whether they are on. Its tag therefore has three states —
 * nothing, how many there are while they are off, the language once one is on.
 *
 * `on` is the accent, and it means what the accent means everywhere else in
 * the player. The glyph's own accent is already taken (it marks the menu as
 * open), which is why the state lives in the tag's fill and not in the icon.
 */
import { LANG_ALIASES } from './languages';
import type { Track } from './player.svelte';

export interface TrackBadge {
  text: string;
  on: boolean;
}

/// Tags that name no language. `und` is what a muxer writes when nobody told it
/// anything, and a corner reading "UND" says less than an empty one.
const NO_LANGUAGE = new Set(['und', 'zxx', 'mis', 'mul', 'qaa']);

/// A track's language in capitals, as short as the tables know it: `rus` and
/// `ru` are both `RU`, `pt-BR` is `PT`, an unknown three-letter tag stays three
/// letters. Null for anything that is not a language code.
export function langTag(lang: string | null): string | null {
  if (!lang) return null;
  const code = lang.trim().toLowerCase().split(/[-_]/)[0];
  if (NO_LANGUAGE.has(code)) return null;
  const short = LANG_ALIASES[code] ?? code;
  return /^[a-z]{2,3}$/.test(short) ? short.toUpperCase() : null;
}

/// What a track is called in a tag. Closed captions carry no language, and
/// "CC" is the name everybody already reads them by.
function trackTag(track: Track): string | null {
  if (track.codec === 'eia_608' || track.codec === 'eia_708') return 'CC';
  return langTag(track.lang);
}

/// The track being heard, when there is exactly one. `heard` is the mix's own
/// list while several tracks are mixed: mpv then calls every track selected,
/// so its flag answers nothing (see "Several audio tracks at once" in
/// tracks.svelte.ts).
export function playingAudio(tracks: Track[], heard: number[] | null): Track | undefined {
  if (heard) return heard.length === 1 ? tracks.find((t) => t.id === heard[0]) : undefined;
  return tracks.find((t) => t.selected);
}

export function audioBadge(tracks: Track[], heard: number[] | null): TrackBadge | null {
  if (heard && heard.length > 1) return { text: `×${heard.length}`, on: true };
  const playing = playingAudio(tracks, heard);
  const tag = playing && trackTag(playing);
  return tag ? { text: tag, on: false } : null;
}

/// A subtitle that is on but names no language still has to read as on, so it
/// falls back to its place in the list — in the accent, which keeps it apart
/// from the count an "off" tag shows.
export function subBadge(tracks: Track[]): TrackBadge | null {
  if (!tracks.length) return null;
  const shown = tracks.find((t) => t.selected);
  if (!shown) return { text: String(tracks.length), on: false };
  return { text: trackTag(shown) ?? String(tracks.indexOf(shown) + 1), on: true };
}
