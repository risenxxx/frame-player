/**
 * Release notes: from the changesets written while working to the page a
 * release is published with.
 *
 *   node scripts/changelog.mjs check          # the gate: every changeset and every release file is well-formed
 *   node scripts/changelog.mjs preview        # what the next release's notes would say, without writing anything
 *   node scripts/changelog.mjs notes 1.18.0   # the notes of one release as Markdown, for the GitHub release
 *
 * **Two kinds of file, and the second is the record.**
 *
 * `.changeset/*.md` is one change a viewer will notice, written when the change
 * is made, in the format of the `changesets` tool so its CLI (`npx changeset`)
 * can write one too:
 *
 *     ---
 *     "frameplayer": minor          ← minor/major is "New", patch is "Improvements and fixes"
 *     ---
 *
 *     Lists fade at their edges     ← the headline, one line
 *
 *     The queue, chapters and …     ← the detail, one paragraph, optional
 *
 * A changeset with **empty** frontmatter is not a change but the release's
 * one-line summary — the sentence the home page prints on the release's card.
 *
 * `changelog/<version>.md` is a published release, written by
 * `set-version` out of the changesets it then deletes. It is plain Markdown
 * with its version, date and summary in the frontmatter, and it is what the
 * GitHub release, the site's `/updates` page and its home-page cards are all
 * rendered from — so what was published is in the repository, reviewable and
 * correctable after the fact, rather than in a GitHub release body nobody
 * diffs.
 *
 * `changeset version` itself is deliberately not used: it would bump one of the
 * five files that carry the version (see set-version.mjs) and write a
 * CHANGELOG.md with neither dates nor summaries, which is what the site needs.
 */

import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { createRequire } from 'node:module';

/*
  Loaded on first use rather than imported: the release workflow's publish job
  runs `notes` with the runner's own Node and no `npm ci`, and reading a
  published release needs no changeset parser.
*/
const parse = (text) => createRequire(import.meta.url)('@changesets/parse').default(text);

export const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const CHANGESETS = join(root, '.changeset');
export const RELEASES = join(root, 'changelog');

const PACKAGE = 'frameplayer';
export const SECTIONS = { new: 'New', fixes: 'Improvements and fixes' };
const RANK = { patch: 0, minor: 1, major: 2 };
/** Past this a card on the home page wraps into a paragraph. */
const SUMMARY_MAX = 200;
const HEADLINE_MAX = 80;

// ── changesets ───────────────────────────────────────────────────────────────

/**
 * When a changeset was first committed, so the notes list changes in the order
 * they were made. A file name is a random slug and says nothing about that;
 * one not committed yet is the newest.
 */
function addedAt(path) {
  try {
    const out = execFileSync('git', ['log', '--diff-filter=A', '--format=%ct', '--', path], { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] });
    const times = out.trim().split('\n').filter(Boolean).map(Number);
    return times.length ? Math.min(...times) : Infinity;
  } catch {
    return Infinity;
  }
}

/** One paragraph, however the author wrapped it. */
const unwrap = (text) => text.replace(/\s*\n\s*/g, ' ').trim();

/**
 * Every pending changeset, parsed and checked. Returns the changes, in the
 * order they were made, and the summary if one was written; throws with every
 * problem at once rather than the first.
 */
export function readChangesets() {
  if (!existsSync(CHANGESETS)) return { changes: [], summary: null, files: [] };
  const files = readdirSync(CHANGESETS).filter((f) => f.endsWith('.md') && f !== 'README.md');
  const problems = [];
  const changes = [];
  const summaries = [];

  for (const file of files) {
    const path = join(CHANGESETS, file);
    let parsed;
    try {
      parsed = parse(readFileSync(path, 'utf8'));
    } catch (error) {
      problems.push(`${file}: ${error.message.split('\n')[0]}`);
      continue;
    }
    const { releases, summary } = parsed;
    if (!summary) {
      problems.push(`${file}: no text under the frontmatter`);
      continue;
    }
    if (releases.length === 0) {
      summaries.push({ file, text: unwrap(summary) });
      continue;
    }
    const foreign = releases.filter((r) => r.name !== PACKAGE);
    if (foreign.length || releases.length !== 1) {
      problems.push(`${file}: the frontmatter must name "${PACKAGE}" and nothing else`);
      continue;
    }
    const { type } = releases[0];
    if (!(type in RANK)) {
      problems.push(`${file}: "${type}" — a change that is not worth a line in the notes needs no changeset`);
      continue;
    }
    const [headline, ...rest] = summary.split(/\n\s*\n/);
    const head = unwrap(headline).replace(/^#+\s*/, '');
    if (head.length > HEADLINE_MAX) problems.push(`${file}: the headline is ${head.length} characters, keep it under ${HEADLINE_MAX}`);
    if (rest.length > 1) problems.push(`${file}: the detail must be one paragraph`);
    changes.push({ file, type, headline: head, detail: unwrap(rest.join(' ')), at: addedAt(path) });
  }

  if (summaries.length > 1) problems.push(`${summaries.map((s) => s.file).join(', ')}: only one release summary (empty frontmatter) per release`);
  for (const s of summaries) {
    if (s.text.length > SUMMARY_MAX) problems.push(`${s.file}: the summary is ${s.text.length} characters, keep it under ${SUMMARY_MAX}`);
  }
  if (summaries.length && !changes.length) problems.push(`${summaries[0].file}: a summary with no changes to summarize`);
  if (problems.length) throw new Error(problems.join('\n'));

  changes.sort((a, b) => a.at - b.at || a.file.localeCompare(b.file));
  return { changes, summary: summaries[0]?.text ?? null, files: [...changes, ...summaries].map((c) => join(CHANGESETS, c.file)) };
}

/** The bump the pending changes call for, or null when there are none. */
export function bumpFor(changes) {
  if (!changes.length) return null;
  return changes.reduce((best, c) => (RANK[c.type] > RANK[best] ? c.type : best), 'patch');
}

// ── rendering ────────────────────────────────────────────────────────────────

const period = (s) => (/[.!?…:]$/.test(s) ? s : `${s}.`);
const firstSentence = (s) => s.match(/^.*?[.!?](?=\s|$)/)?.[0] ?? s;

/**
 * What the card says when nobody wrote a summary: the first new thing's first
 * sentence, or the first fix's. Serviceable rather than good — which is what
 * `set-version` says out loud when it falls back on it.
 */
function fallbackSummary(changes) {
  const lead = changes.find((c) => c.type !== 'patch') ?? changes[0];
  if (!lead) return 'Maintenance release with no changes to the player itself.';
  return lead.detail ? firstSentence(lead.detail) : period(lead.headline);
}

export function renderRelease({ version, date, changes, summary }) {
  const item = (c) => `- **${period(c.headline)}**${c.detail ? ` ${c.detail}` : ''}`;
  const fresh = changes.filter((c) => c.type !== 'patch');
  const fixes = changes.filter((c) => c.type === 'patch');
  const sections = [
    fresh.length ? `### ${SECTIONS.new}\n\n${fresh.map(item).join('\n')}` : '',
    fixes.length ? `### ${SECTIONS.fixes}\n\n${fixes.map(item).join('\n')}` : '',
  ].filter(Boolean);

  return [
    '---',
    `version: ${version}`,
    `date: ${date}`,
    // A JSON string is a valid YAML double-quoted scalar, and quoting always
    // spares having to know which characters YAML would read as syntax.
    `summary: ${JSON.stringify(summary ?? fallbackSummary(changes))}`,
    '---',
    '',
    ...(sections.length ? [sections.join('\n\n'), ''] : []),
  ].join('\n');
}

// ── published releases ───────────────────────────────────────────────────────

const FRONTMATTER = /^---\n([\s\S]*?)\n---\n?([\s\S]*)$/;

/** A release file's frontmatter and body. Only the three flat keys this format has. */
export function readRelease(version) {
  const path = join(RELEASES, `${version}.md`);
  const text = readFileSync(path, 'utf8').replace(/\r\n/g, '\n');
  const m = FRONTMATTER.exec(text);
  if (!m) throw new Error(`changelog/${version}.md: no frontmatter`);
  const data = {};
  for (const line of m[1].split('\n')) {
    const kv = /^(\w+):\s*(.*)$/.exec(line);
    if (!kv) throw new Error(`changelog/${version}.md: cannot read the frontmatter line "${line}"`);
    let value = kv[2].trim();
    if (value.startsWith('"')) value = JSON.parse(value);
    else if (value.startsWith("'")) value = value.slice(1, -1).replace(/''/g, "'");
    data[kv[1]] = value;
  }
  return { ...data, body: m[2].trim() };
}

function checkRelease(file) {
  const problems = [];
  const version = file.replace(/\.md$/, '');
  let r;
  try {
    r = readRelease(version);
  } catch (error) {
    return [error.message];
  }
  if (r.version !== version) problems.push(`changelog/${file}: says version ${r.version}`);
  if (!/^\d{4}-\d{2}-\d{2}$/.test(r.date ?? '')) problems.push(`changelog/${file}: date must be YYYY-MM-DD`);
  if (!r.summary) problems.push(`changelog/${file}: no summary`);
  else if (r.summary.length > SUMMARY_MAX) problems.push(`changelog/${file}: the summary is ${r.summary.length} characters, keep it under ${SUMMARY_MAX}`);
  const headings = [...r.body.matchAll(/^(#+)\s*(.*)$/gm)];
  const allowed = Object.values(SECTIONS);
  for (const [, hashes, title] of headings) {
    if (hashes !== '###' || !allowed.includes(title)) problems.push(`changelog/${file}: unexpected heading "${hashes} ${title}" — only "### ${allowed.join('" and "### ')}"`);
  }
  return problems;
}

export function releaseVersions() {
  if (!existsSync(RELEASES)) return [];
  return readdirSync(RELEASES)
    .filter((f) => /^\d+\.\d+\.\d+\.md$/.test(f))
    .map((f) => f.slice(0, -3))
    .sort((a, b) => {
      const [x, y] = [a, b].map((v) => v.split('.').map(Number));
      return x[0] - y[0] || x[1] - y[1] || x[2] - y[2];
    });
}

/** The version `tauri.conf.json` carries — the one the release workflow will publish. */
export function currentVersion() {
  return JSON.parse(readFileSync(join(root, 'src-tauri/tauri.conf.json'), 'utf8')).version;
}

// ── the command line ─────────────────────────────────────────────────────────

function check() {
  const problems = [];
  let pending = { changes: [] };
  try {
    pending = readChangesets();
  } catch (error) {
    problems.push(...error.message.split('\n'));
  }
  for (const v of releaseVersions()) problems.push(...checkRelease(`${v}.md`));
  // The release workflow publishes whatever tauri.conf.json says, and its notes
  // come from this file: a version without one would go out with no notes.
  const current = currentVersion();
  if (!existsSync(join(RELEASES, `${current}.md`))) {
    problems.push(`changelog/${current}.md is missing — the version was set without "npm run set-version", which writes it`);
  }
  if (problems.length) {
    console.error(`Release notes:\n  ${problems.join('\n  ')}`);
    process.exit(1);
  }
  const n = pending.changes.length;
  console.log(`Release notes: ${releaseVersions().length} releases, ${n} pending change${n === 1 ? '' : 's'}.`);
}

function preview() {
  const { changes, summary } = readChangesets();
  if (!changes.length) {
    console.log('No pending changesets.');
    return;
  }
  console.log(`Next release: a ${bumpFor(changes)} bump.\n`);
  console.log(renderRelease({ version: '<next>', date: new Date().toISOString().slice(0, 10), changes, summary }));
}

/** The notes as the GitHub release shows them: the summary as a lead, then the sections. */
export function releaseNotes(version) {
  const r = readRelease(version);
  return [r.summary, r.body].filter(Boolean).join('\n\n') + '\n';
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [cmd, arg] = process.argv.slice(2);
  if (cmd === 'check') check();
  else if (cmd === 'preview') preview();
  else if (cmd === 'notes' && arg) process.stdout.write(releaseNotes(arg));
  else {
    console.error('usage: changelog.mjs check | preview | notes <version>');
    process.exit(2);
  }
}
