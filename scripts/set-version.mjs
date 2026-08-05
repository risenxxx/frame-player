/**
 * Set the player's version everywhere at once, or report where it disagrees.
 *
 *   node scripts/set-version.mjs            # report; exits 1 if the files disagree
 *   node scripts/set-version.mjs 0.21.0     # set exactly that
 *   node scripts/set-version.mjs patch      # 0.20.0 -> 0.20.1
 *   node scripts/set-version.mjs minor      # 0.20.0 -> 0.21.0
 *   node scripts/set-version.mjs major      # 0.20.0 -> 1.0.0
 *
 * **Five files carry the version, and there is no way to collapse them into
 * one.** `tauri.conf.json` can be pointed at a package.json instead of holding
 * a literal, but the release workflow reads that literal (`jq -r '.version'`)
 * to decide whether a push is a release at all — so making it indirect would
 * mean the gate no longer sees a version to compare. Cargo cannot read a
 * version from JSON in any form. So they are written, not derived, and this
 * script is what keeps that from being five manual edits.
 *
 * Drift here is silent, which is the reason to have this at all: `package-lock`
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

import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

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
    file: 'package-lock.json',
    // Anchored on the name above it, twice: npm writes the root version and
    // the one inside `packages[""]`, and everything after them belongs to a
    // dependency. Without the anchor this would be two of some 2000 matches.
    pattern: /("name":\s*"frameplayer",\s*\n\s*"version":\s*")([^"]+)(")/,
    all: true,
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
  const wanted = target.all ? 2 : 1;
  if (matches.length !== wanted) {
    throw new Error(
      `${target.file}: expected ${wanted} version line(s), found ${matches.length}. ` +
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

const arg = process.argv[2];
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
    console.error(`\nVersions disagree. Run: npm run set-version ${current}`);
    process.exit(1);
  }
  console.log(`\nAll five agree on ${current}.`);
  process.exit(0);
}

const next = ['major', 'minor', 'patch'].includes(arg) ? bump(current, arg) : arg;
if (!SEMVER.test(next)) {
  console.error(`"${arg}" is neither a version like 1.2.3 nor major/minor/patch.`);
  process.exit(1);
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

console.log(
  `\nDone. Pushing this to main is what releases it: the workflow compares ` +
    `tauri.conf.json's version against the previous commit's.`,
);
