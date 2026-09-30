/**
 * Set the player's version everywhere at once, or report where it disagrees.
 *
 *   node scripts/set-version.mjs            # report; exits 1 if the files disagree
 *   node scripts/set-version.mjs 0.21.0     # set exactly that
 *   node scripts/set-version.mjs patch      # 0.20.0 -> 0.20.1
 *   node scripts/set-version.mjs minor      # 0.20.0 -> 0.21.0
 *   node scripts/set-version.mjs major      # 0.20.0 -> 1.0.0
 *   node scripts/set-version.mjs release    # whatever the pending changesets call for
 *
 * **Setting a version publishes the pending changesets.** Every write turns
 * `.changeset/*.md` into `changelog/<version>.md` and deletes them (see
 * changelog.mjs), because the release that push produces takes its notes from
 * that file. With nothing pending it refuses: a release with no notes is
 * almost always a forgotten changeset, and `--allow-empty` is for the rare one
 * that really changes nothing a viewer would notice. A version that already
 * has notes (the files were set by hand, or this is being re-run) keeps them.
 *
 * **Four files carry the version, and there is no way to collapse them into
 * one.** `tauri.conf.json` can be pointed at a package.json instead of holding
 * a literal, but the release workflow reads that literal (`jq -r '.version'`)
 * to decide whether a push is a release at all — so making it indirect would
 * mean the gate no longer sees a version to compare. Cargo cannot read a
 * version from JSON in any form. So they are written, not derived, and this
 * script is what keeps that from being four manual edits. (The JavaScript
 * lockfile used to be a fifth; `bun.lock` does not record the root package's
 * version, so there is nothing in it to drift.)
 *
 * Drift here is silent, which is the reason to have this at all: npm's lockfile
 * had been sitting at **0.1.0** since the first commit, because nothing ever
 * reads it out loud. Hence the no-argument mode, which is a check rather than a
 * write and is worth running before a release.
 *
 * Edits are **surgical**, one anchored line per file, the same principle as
 * `mpv_conf_set` in lib.rs: a JSON round-trip would reformat
 * `tauri.conf.json`'s hand-written inline arrays (measured — it rewrites the
 * `endpoints` array across four lines), and reformatting a file to change six
 * characters buries the change in the diff. Every anchor must match **exactly
 * once**; anything else is a hard error naming the file, because a version bump
 * that quietly skipped a file is precisely the bug this exists to end.
 */

import { readFileSync, writeFileSync, existsSync, mkdirSync, unlinkSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { readChangesets, bumpFor, renderRelease, RELEASES } from './changelog.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

/**
 * Where the version lives. Each pattern captures the value in group 2, so the
 * same code both reads and writes — a reader written separately from the writer
 * is one that can disagree with it.
 */
const TARGETS = [
  {
    file: 'package.json',
    // The first `"version"` in the file is the package's own: dependency
    // versions are values of their package names, not of a `version` key.
    pattern: /^(\s*"version":\s*")([^"]+)(")/m,
  },
  {
    file: 'src-tauri/tauri.conf.json',
    pattern: /^(\s*"version":\s*")([^"]+)(")/m,
  },
  {
    file: 'src-tauri/Cargo.toml',
    // `[package]` is the first table in the manifest, so the first bare
    // `version = ` at the start of a line is ours; a dependency's version is
    // indented inside its own table or written inline.
    pattern: /^(version = ")([^"]+)(")/m,
  },
  {
    file: 'src-tauri/Cargo.lock',
    // Our own entry among ~700. `\r?` so a lockfile checked out with CRLF on
    // Windows still matches.
    pattern: /(\[\[package\]\]\r?\nname = "frameplayer"\r?\nversion = ")([^"]+)(")/,
  },
];

const SEMVER = /^\d+\.\d+\.\d+$/;

function read(target) {
  const path = join(root, target.file);
  const text = readFileSync(path, 'utf8');
  const matches = [...text.matchAll(new RegExp(target.pattern, target.pattern.flags + 'g'))];
  if (matches.length !== 1) {
    throw new Error(
      `${target.file}: expected 1 version line, found ${matches.length}. ` +
        `The file's shape changed — fix the pattern in scripts/set-version.mjs rather ` +
        `than letting a release go out with this file left behind.`,
    );
  }
  const found = new Set(matches.map((m) => m[2]));
  return { path, text, matches, versions: [...found] };
}

function bump(current, kind) {
  const [major, minor, patch] = current.split('.').map(Number);
  if (kind === 'major') return `${major + 1}.0.0`;
  if (kind === 'minor') return `${major}.${minor + 1}.0`;
  return `${major}.${minor}.${patch + 1}`;
}

const args = process.argv.slice(2);
const allowEmpty = args.includes('--allow-empty');
const arg = args.find((a) => !a.startsWith('--'));
const states = TARGETS.map((target) => ({ target, ...read(target) }));

// `tauri.conf.json` is the authority, because it is what the release workflow
// reads to decide whether a push is a release — so a bump is measured from
// whatever *it* says, even when another file has drifted away from it.
const authority = states.find((s) => s.target.file === 'src-tauri/tauri.conf.json');
const current = authority.versions[0];

if (!arg) {
  const disagree = states.some((s) => s.versions.length > 1 || s.versions[0] !== current);
  for (const s of states) {
    const mark = s.versions.length === 1 && s.versions[0] === current ? ' ' : '!';
    console.log(`${mark} ${s.versions.join(', ').padEnd(10)} ${s.target.file}`);
  }
  if (disagree) {
    console.error(`\nVersions disagree. Run: bun run set-version ${current}`);
    process.exit(1);
  }
  console.log(`\nAll ${states.length} agree on ${current}.`);
  const { changes } = readChangesets();
  if (changes.length) console.log(`${changes.length} pending changeset(s) call for a ${bumpFor(changes)} bump.`);
  process.exit(0);
}

let pending;
try {
  pending = readChangesets();
} catch (error) {
  console.error(`The changesets need fixing first:\n  ${error.message.split('\n').join('\n  ')}`);
  process.exit(1);
}
const needed = bumpFor(pending.changes);

if (arg === 'release' && !needed) {
  console.error('"release" bumps by the pending changesets, and there are none.');
  process.exit(1);
}
const kind = arg === 'release' ? needed : arg;
const next = ['major', 'minor', 'patch'].includes(kind) ? bump(current, kind) : kind;
if (!SEMVER.test(next)) {
  console.error(`"${arg}" is neither a version like 1.2.3 nor major/minor/patch/release.`);
  process.exit(1);
}

const notesPath = join(RELEASES, `${next}.md`);
const hasNotes = existsSync(notesPath);
if (!hasNotes && !needed && !allowEmpty) {
  console.error(
    `No changesets in .changeset/, so ${next} would be released with no notes.\n` +
      `Write one for each change a viewer will notice, or pass --allow-empty ` +
      `if this release really changes nothing they would.`,
  );
  process.exit(1);
}
const RANK = { patch: 0, minor: 1, major: 2 };
if (needed && ['major', 'minor', 'patch'].includes(kind) && RANK[kind] < RANK[needed]) {
  console.warn(`Note: the changesets call for a ${needed} bump, and this is a ${kind} one.`);
}

for (const { target, path, text, matches } of states) {
  let out = text;
  // Applied back to front, so replacing the first match cannot shift the
  // offsets of the second.
  for (const m of [...matches].reverse()) {
    out = out.slice(0, m.index) + m[1] + next + m[3] + out.slice(m.index + m[0].length);
  }
  if (out !== text) writeFileSync(path, out);
  console.log(`${matches[0][2]} -> ${next}  ${target.file}`);
}

if (hasNotes && pending.changes.length) {
  console.warn(`\nchangelog/${next}.md already exists, so the pending changesets were left where they are.`);
} else if (!hasNotes) {
  mkdirSync(RELEASES, { recursive: true });
  const date = new Date().toISOString().slice(0, 10);
  writeFileSync(notesPath, renderRelease({ version: next, date, ...pending }));
  for (const file of pending.files) unlinkSync(file);
  console.log(`\nWrote changelog/${next}.md from ${pending.changes.length} changeset(s), and removed them.`);
  if (pending.changes.length && !pending.summary) {
    console.log(`No summary changeset was written, so the summary is the first change's first sentence — worth a look.`);
  }
}

console.log(
  `\nDone. Pushing this to main is what releases it: the workflow compares ` +
    `tauri.conf.json's version against the previous commit's.`,
);
