/** Chapters: the list, their names, the skippable ones, and moving between them. */

import { command, getProperty } from 'tauri-plugin-libmpv-api';

import { formatTime } from '../format';
import { t } from '../i18n.svelte';
import { latest } from '../latest';
import { showOsd } from '../osd.svelte';
import { player, type Chapter } from './state.svelte';
import { readList } from './tracks';

// ---- Chapters -------------------------------------------------------------

/// Same role as `trackReads`: `chapter-list` is not final at `file-loaded` for
/// every container, so there are several reasons to start a read, and a slow
/// earlier one must not finish last and overwrite a fresh list.
const chapterReads = latest();

/// mpv's placeholder for a chapter with no title in the container.
///
/// The two list properties disagree here, so the track pattern cannot simply be
/// copied. Measured on mpv 0.41: `track-list/N/title` of an untitled track is
/// *unavailable* (the read fails and the catch below produces null), while
/// `chapter-list/N/title` is available and reads back as this literal. Left
/// alone it would put an English placeholder in the chapter list of a Russian
/// interface, instead of the numbered fallback.
const MPV_UNNAMED = '(unnamed)';

export async function loadChapters() {
  const run = chapterReads.begin();
  const list = await readList('chapter-list', async (base, index) => {
    // A chapter without a time is not navigable, and mpv does not produce one —
    // but the read can also fail simply because the file went away underneath.
    const time = await getProperty(`${base}/time`, 'double').catch(() => null);
    if (time === null) return null;
    const raw = (await getProperty(`${base}/title`, 'string').catch(() => null))?.trim();
    const title = !raw || raw === MPV_UNNAMED ? null : raw;
    return { index, time, title } satisfies Chapter;
  }).catch(() => [] as Chapter[]);
  if (run.stale) return;
  player.chapters = list;
}

/// Formatted at the call site rather than stored, so nameless chapters are
/// renamed when the interface language changes.
export function chapterTitle(chapter: Chapter): string {
  return chapter.title ?? t('chapter.generic', { n: chapter.index + 1 });
}

/// The chapter containing `time`, or null when the file has none. Used by the
/// seekbar hover: the list is short (tens of entries), so a scan is cheaper
/// than keeping an index in sync with it.
export function chapterAt(time: number): Chapter | null {
  const list = player.chapters;
  for (let i = list.length - 1; i >= 0; i--) {
    if (time >= list[i].time) return list[i];
  }
  return list[0] ?? null;
}

/// Chapters a viewer normally skips. Split by kind, because the button has to
/// name what it skips: "Skip credits" over an opening is worse than no button.
export type SkipKind = 'intro' | 'recap' | 'preview' | 'trailer' | 'credits' | 'ad';

/**
 * Titles that announce a chapter as skippable.
 *
 * Matched against the **whole** title, not searched inside it. A substring
 * search reads "Ending the war for good" as the closing credits and "Opening
 * the vault" as a title sequence — measured against a corpus of real chapter
 * names, that is not a rare edge case but the common one, because these words
 * are ordinary English verbs. Announcement chapters, by contrast, are named
 * exactly what they are.
 *
 * Anchoring also removes the "субтитры" contains "титры" collision for free,
 * and it makes the failure direction the harmless one: an unusual name
 * ("Opening theme (1:45)") simply offers no button, rather than a button
 * appearing over a scene the viewer wanted to watch.
 *
 * The only decorations allowed are the ones rips actually use: a leading index
 * ("01. Intro") and trailing punctuation ("Previously on…").
 */
const SKIP_ALTERNATIVES: ReadonlyArray<readonly [SkipKind, string]> = [
  // Ordered intro-first, which **is no longer load-bearing** and is kept as
  // defense rather than as a rule. It mattered while the patterns were
  // substring searches: "Opening Credits" matched both. Anchoring them to the
  // whole title made the alternatives disjoint — verified by enumerating every
  // title they match from a corpus of their own literals and diffing the
  // verdicts under both orders, which are identical. Re-order freely; but if a
  // pattern ever loses its anchors, this is what stops the pair colliding.
  [
    'intro',
    String.raw`intro(?:duction)?|opening(?:\s+(?:credits|titles?|sequence|theme|song))?|op\s*\d*|main\s+titles?|title\s+sequence|заставка|опенинг|вступление|(?:вступительные|начальные)\s+титры`,
  ],
  // The "on <show name>" tails are the only open-ended alternatives here, and
  // they are bounded twice: by a character budget, and by the word cap in
  // `skipKind`. Note `\b` is useless after a Cyrillic letter (it is ASCII-only
  // in JS, so "ранее в\b" never matched at all) — those need an explicit \s.
  [
    'recap',
    // "re-cap" is the same word hyphenated, and rips spell it both ways; the
    // optional hyphen costs nothing because the pattern is anchored anyway.
    String.raw`re-?cap|previously(?:\s+on(?:\s+.{0,40})?)?|ранее\s+в(?:\s+.{0,40})?|в\s+предыдущих\s+сериях?|краткое\s+содержание|рекап`,
  ],
  [
    'preview',
    String.raw`(?:next\s+episode\s+)?preview|teaser|next\s+time(?:\s+on(?:\s+.{0,40})?)?|анонс|в\s+следующей\s+серии`,
  ],
  // A kind of its own rather than a spelling of "preview", because the kind is
  // only ever read to label the button: an anime episode's preview announces
  // the next episode, while a chapter called "Trailer" on a disc rip is a promo
  // for another film entirely, and "Пропустить анонс" over one of those is the
  // "Skip credits over an opening" mistake in miniature. The button names what
  // it skips, so the pattern that knows the difference keeps it.
  ['trailer', String.raw`trailers?|трейлеры?`],
  [
    'credits',
    String.raw`(?:end|ending|final|closing)?\s*credits|ending(?:\s+(?:song|theme))?|outro|ed\s*\d*|(?:финальные|заключительные|конечные)?\s*титры|концовка|эндинг`,
  ],
  [
    'ad',
    String.raw`ads?|advert(?:isement)?s?|sponsor(?:ed|ship)?|commercials?|реклама|спонсор`,
  ],
];

const SKIP_PATTERNS: ReadonlyArray<readonly [SkipKind, RegExp]> = SKIP_ALTERNATIVES.map(
  ([kind, alternatives]) =>
    [kind, new RegExp(String.raw`^\s*(?:\d{1,2}\s*[.)\-–—:]\s*)?(?:${alternatives})\s*[.!…]*\s*$`, 'i')] as const,
);

/// An announcement chapter names itself and stops; a title of half a sentence
/// is describing a scene. The outer bound on top of the anchoring, so that the
/// open-ended tails ("Previously on <show>") and any pattern added later cannot
/// quietly start matching prose.
const SKIP_MAX_WORDS = 5;

/// What kind of skippable chapter this is, or null. A chapter mpv had to name
/// itself (no title in the container) can never match — which is right: with no
/// title there is nothing to base the claim on.
export function skipKind(chapter: Chapter): SkipKind | null {
  const title = chapter.title;
  if (!title || title.split(/\s+/).length > SKIP_MAX_WORDS) return null;
  for (const [kind, pattern] of SKIP_PATTERNS) {
    if (pattern.test(title)) return kind;
  }
  return null;
}

/**
 * Jump to the start of a chapter, by index.
 *
 * Through the `chapter` property rather than a seek to `chapter.time`: with
 * ordered chapters (MKV) a chapter's content can live in a different file
 * entirely, which only mpv's own chapter seek handles. It is an exact seek —
 * the same contract as a seekbar click, since this is a deliberate "take me
 * there", not a preview.
 */
export function seekChapter(index: number) {
  const chapter = player.chapters[index];
  if (!chapter) return;
  // `set` with a string: mpv's own parser is guaranteed to understand the
  // property, same as with aid/sid.
  void command('set', ['chapter', String(index)]).catch(() => {});
  player.chapterIndex = index;
  showOsd(chapterTitle(chapter), { sub: formatTime(chapter.time) });
}

/**
 * Previous/next chapter.
 *
 * Clamped at both ends on purpose: mpv's own `add chapter ±1` walks off into
 * the neighbouring playlist entry, and "previous chapter" silently opening the
 * previous episode is not what the key says it does. At the first chapter,
 * "previous" restarts it — which is what every other player does too.
 */
export function jumpChapter(delta: number) {
  if (!player.hasFile || player.chapters.length === 0) return;
  // The mirror can be stale (a dropped property-change), and it is -1 until mpv
  // reports the first one — derive the current chapter from the position.
  const current = player.chapterIndex >= 0
    ? player.chapterIndex
    : (chapterAt(player.timePos)?.index ?? 0);
  const target = current + delta;
  if (target >= player.chapters.length) return;
  seekChapter(Math.max(0, target));
}
