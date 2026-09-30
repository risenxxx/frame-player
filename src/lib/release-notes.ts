/**
 * The notes of a waiting update, as the tooltip of the title bar's update
 * button shows them.
 *
 * They arrive as `Update.body`, which is the `notes` field the release
 * workflow writes into `latest.json` from `changelog/<version>.md` (see
 * scripts/changelog.mjs): a one-paragraph summary, then `### New` and
 * `### Improvements and fixes`, each a list of `- **Headline.** Detail`. A
 * tooltip is plain text in a box 360px wide, so what it gets is the summary and
 * the headlines — the details are a paragraph each and would make the tip a
 * page. The full notes are on the site.
 *
 * Pure, because what it reads was written by another program at another time:
 * a manifest from before notes existed has none, and one written by hand may
 * not follow the format at all. Both must degrade to something shown, not to
 * an exception in a hover handler.
 */

/** More than this and the tip stops being a glance. */
export const MAX_ITEMS = 6;

export interface ReleaseNotes {
  summary: string;
  items: string[];
}

export function parseReleaseNotes(body: string | null | undefined): ReleaseNotes | null {
  const text = body?.replace(/\r\n/g, '\n').trim();
  if (!text) return null;
  const blocks = text.split(/\n\s*\n/);
  const summary = blocks[0].startsWith('#') || blocks[0].startsWith('- ') ? '' : blocks[0].replace(/\s*\n\s*/g, ' ');
  const items = [...text.matchAll(/^- \*\*(.+?)\*\*/gm)].map((m) => m[1].replace(/\.$/, ''));
  if (!summary && !items.length) {
    // Not our format: show its first paragraph as it is, Markdown and all.
    return { summary: blocks[0].replace(/\s*\n\s*/g, ' '), items: [] };
  }
  return { summary, items };
}

/**
 * The tooltip's text. Lines rather than markup: the tooltip renders text and
 * keeps its line breaks. `more` is the localized "and N more" for what did not
 * fit, since the notes themselves are English and this line is the player's.
 */
export function notesTip(heading: string, notes: ReleaseNotes, more: (n: number) => string): string {
  const shown = notes.items.slice(0, MAX_ITEMS);
  const rest = notes.items.length - shown.length;
  return [
    heading,
    ...(notes.summary ? ['', notes.summary] : []),
    ...(shown.length ? ['', ...shown.map((h) => `• ${h}`)] : []),
    ...(rest > 0 ? [more(rest)] : []),
  ].join('\n');
}
