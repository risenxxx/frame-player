/**
 * Audio and subtitle tracks: reading the list (and any list property) through
 * sub-properties, matching a remembered track in another file, selecting,
 * attaching and removing.
 */

import { open } from '@tauri-apps/plugin-dialog';
import { command, getProperty } from 'tauri-plugin-libmpv-api';

import { externalTrackName } from '../external-title';
import { baseName } from '../format';
import { t } from '../i18n.svelte';
import { LANG_ALIASES } from '../languages';
import { latest } from '../latest';
import { showOsd } from '../osd.svelte';
import { AUDIO_EXTENSIONS, SUBTITLE_EXTENSIONS } from './files';
import { player, type Track } from './state.svelte';

/**
 * What a chosen track was, in terms that survive into the next episode.
 *
 * Deliberately not the id: track ids are positions inside one file, and the
 * Russian dub that is #2 in episode 1 is routinely #3 in episode 2. The index
 * is kept anyway, but only to break ties between otherwise equal candidates.
 */
export interface TrackDesc {
  lang: string | null;
  title: string | null;
  codec: string | null;
  forced: boolean;
  index: number;
}

/// A remembered choice: a track to look for, or the explicit "none".
export type TrackWish = TrackDesc | 'no';

// ---- List properties ------------------------------------------------------

/**
 * Reads a list property through its sub-properties.
 *
 * IMPORTANT: the 'node' format in libmpv-wrapper 0.1.1 is broken (it writes an
 * mpv_node into a pointer variable -> stack corruption -> ACCESS_VIOLATION), so
 * a list is never read as a whole. Every list property in mpv is implemented
 * through the same helper on its side, which exposes `<prop>/count` and
 * `<prop>/N/<field>` in simple formats — so `chapter-list` and `playlist` are
 * readable exactly like `track-list`, and none of them needs a node.
 *
 * Entries the reader rejects (a missing mandatory field, a type we do not care
 * about) drop out; the caller keeps the sequence guard, because a read is dozens
 * of sequential round-trips and there are several reasons to start one.
 */
export async function readList<T>(
  prop: string,
  readItem: (base: string, index: number) => Promise<T | null>,
): Promise<T[]> {
  const count = (await getProperty(`${prop}/count`, 'int64').catch(() => null)) ?? 0;
  const items: T[] = [];
  for (let i = 0; i < count; i++) {
    const item = await readItem(`${prop}/${i}`, i);
    if (item) items.push(item);
  }
  return items;
}

// ---- Tracks ---------------------------------------------------------------

/// Bumped on every read of the track list. A read is dozens of sequential
/// getProperty calls, there are several reasons to start one (file-loaded, a
/// track-list/count change, opening the menu, switching a track), and the
/// counter changes several times in a row as tracks keep appearing. Without the
/// marker an earlier read can finish last and overwrite a fresh list.
const trackReads = latest();

export async function loadTracks() {
  const run = trackReads.begin();
  try {
    const count = (await getProperty('track-list/count', 'int64')) ?? 0;
    const audio: Track[] = [];
    const subs: Track[] = [];
    for (let i = 0; i < count; i++) {
      const base = `track-list/${i}`;
      const type = await getProperty(`${base}/type`, 'string').catch(() => null);
      if (type !== 'audio' && type !== 'sub') continue;
      const id = (await getProperty(`${base}/id`, 'int64').catch(() => null)) ?? 0;
      const selected = (await getProperty(`${base}/selected`, 'flag').catch(() => false)) ?? false;
      const rawTitle = await getProperty(`${base}/title`, 'string').catch(() => null);
      const rawLang = await getProperty(`${base}/lang`, 'string').catch(() => null);
      // Closed captions carry no title and no language, so they used to show up
      // as a bare "Track 1" — with no hint that picking it means captions
      // positioned on a 32-column grid (hence the off-center, indented text)
      // that also smear across seeks. Naming them is the honest minimum.
      const codec = await getProperty(`${base}/codec`, 'string').catch(() => null);
      const forced = (await getProperty(`${base}/forced`, 'flag').catch(() => false)) ?? false;
      const external = (await getProperty(`${base}/external`, 'flag').catch(() => false)) ?? false;
      const path = external
        ? await getProperty(`${base}/external-filename`, 'string').catch(() => null)
        : null;
      // A dub or subtitle from a release's subfolder is named after the folder
      // rather than "mka" — see external-title.ts. Written over the raw fields,
      // not only the label, because the remembered choice is matched on them.
      const better = path && player.filePath ? externalTrackName(path, player.filePath, rawTitle, rawLang) : null;
      const title = better ? better.title : rawTitle;
      const lang = better ? better.lang : rawLang;
      const named = [title, lang].filter(Boolean);
      const label =
        codec === 'eia_608' || codec === 'eia_708'
          ? [t('track.cc'), ...named].join(' · ')
          : named.join(' · ') || t('track.generic', { id });
      const channels =
        type === 'audio'
          ? await getProperty(`${base}/demux-channel-count`, 'int64').catch(() => null)
          : null;
      (type === 'audio' ? audio : subs).push({
        id,
        selected,
        label,
        lang,
        title,
        codec,
        forced,
        external,
        path,
        channels,
      });
    }
    if (run.stale) return;
    player.audioTracks = audio;
    player.subTracks = subs;
  } catch {
    if (run.stale) return;
    player.audioTracks = [];
    player.subTracks = [];
  }
}

/**
 * ISO-639-2 to -1 for the languages that actually turn up in releases.
 *
 * MKV stores three-letter codes, plenty of files use two — and comparing by
 * prefix is not enough, because `jpn` and `ja` share only one letter. Anything
 * unlisted falls through lower-cased, which still matches like against like.
 *
 * The table now comes from `languages.ts`, so the codes the track scoring
 * compares and the codes the settings picker writes into `alang`/`slang` are
 * one list rather than two that drift. That is a data import with no state and
 * no imports of its own, so it does not bend the "player depends on nothing"
 * rule the *stateful* modules follow.
 */
function normLang(lang: string | null): string | null {
  if (!lang) return null;
  const code = lang.trim().toLowerCase().split(/[-_]/)[0];
  return LANG_ALIASES[code] ?? code;
}

export function describeTrack(track: Track, list: Track[]): TrackDesc {
  return {
    lang: track.lang,
    title: track.title,
    codec: track.codec,
    forced: track.forced,
    index: Math.max(0, list.indexOf(track)),
  };
}

/// Below this, the candidate shares nothing that identifies a track — neither
/// the language nor the title — and guessing from a codec alone is worse than
/// leaving mpv's own `alang`/`slang` choice alone.
const MATCH_FLOOR = 60;

/**
 * Best match for a remembered track in another file's list, with its score.
 *
 * Scored rather than compared for equality, because not one of these fields is
 * guaranteed to be present: releases exist with languages and no titles, with
 * titles and no languages ("Rus"/"Jap" as names), and with two tracks of the
 * same language distinguished only by codec. Equality on any single field
 * would therefore fail on a large share of real files.
 */
export function matchTrack(list: Track[], want: TrackDesc): { track: Track; score: number } | null {
  let best: { track: Track; score: number } | null = null;
  for (const [index, track] of list.entries()) {
    let score = 0;
    const a = normLang(want.lang);
    const b = normLang(track.lang);
    if (a && b && a === b) score += 100;
    if (want.title && track.title && want.title === track.title) score += 60;
    if (want.codec && track.codec === want.codec) score += 12;
    // A forced track is a different thing from full subtitles — one carries the
    // alien dialogue, the other the whole script. Never silently swap them.
    if (want.forced !== track.forced) score -= 50;
    // Position only breaks ties; on its own it means nothing across files.
    if (index === want.index) score += 5;
    if (score >= MATCH_FLOOR && (!best || score > best.score)) best = { track, score };
  }
  return best;
}

export async function selectTrack(kind: 'audio' | 'sub', track: Track | null) {
  try {
    // Through command('set', ...): mpv's string parser is guaranteed to
    // understand choice properties (aid/sid = number | "no" | "auto").
    await command('set', [kind === 'audio' ? 'aid' : 'sid', track ? String(track.id) : 'no']);
    showOsd(track ? track.label : t('osd.subs_off'));
    setTimeout(() => void loadTracks(), 200);
  } catch (e) {
    showOsd(t('osd.track_failed'));
    console.warn('selectTrack failed:', e);
  }
}

export async function attachTrack(kind: 'sub' | 'audio', path: string) {
  try {
    // 'select' makes the new track the active one — dragging a subtitle file
    // onto the window means "show these", not "add them to a list".
    await command(kind === 'sub' ? 'sub-add' : 'audio-add', [path, 'select']);
    showOsd(t(kind === 'sub' ? 'osd.sub_added' : 'osd.audio_added'), { sub: baseName(path) });
    setTimeout(() => void loadTracks(), 200);
  } catch (e) {
    showOsd(t('osd.track_failed'));
    console.warn('attachTrack failed:', e);
  }
}

/**
 * Detach an external subtitle track from the session.
 *
 * mpv's `sub-remove` works on external files only — an embedded track lives in
 * the container and taking it out would mean rewriting the video, which is not
 * a player's job. The caller is expected to have hidden the control for
 * anything else; this refuses as well, because the two would otherwise have to
 * be kept in step by hand.
 */
export async function removeSubTrack(track: Track): Promise<boolean> {
  if (!track.external) return false;
  try {
    await command('sub-remove', [String(track.id)]);
    await loadTracks();
    return true;
  } catch (e) {
    console.warn('sub-remove failed:', e);
    return false;
  }
}

/** File picker for an external subtitle or audio track. */
export async function pickAndAttachTrack(kind: 'sub' | 'audio') {
  if (!player.hasFile) return;
  const sel = await open({
    multiple: true,
    filters: [
      {
        name: t(kind === 'sub' ? 'dialog.subtitles' : 'dialog.audio'),
        extensions: kind === 'sub' ? SUBTITLE_EXTENSIONS : AUDIO_EXTENSIONS,
      },
      { name: t('dialog.all_files'), extensions: ['*'] },
    ],
  });
  const paths = Array.isArray(sel) ? sel : sel ? [sel] : [];
  for (const path of paths) await attachTrack(kind, path);
}
