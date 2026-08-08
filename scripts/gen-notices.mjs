#!/usr/bin/env node
// Renders THIRD-PARTY-NOTICES.md — the attribution and license texts that ship
// inside the application.
//
//   node scripts/gen-notices.mjs            # write the file
//   node scripts/gen-notices.mjs --check    # fail if the committed file is stale
//   node scripts/gen-notices.mjs --refresh  # re-read cargo/npm first, then write
//
// **This is not paperwork.** The player links libmpv, FFmpeg, libplacebo and
// friends dynamically, all of them LGPL-2.1, and every one of those wants a
// prominent notice naming it, a copy of its license and a route to its source.
// Until this file existed the bundle carried none of the three. Frame Player's
// own GPL-3.0 asks for the same of itself, which is why its LICENSE is embedded
// here as well rather than only sitting beside the executable: the dialog that
// shows this file is then the one place that answers every license question.
//
// Rendering reads only committed inputs — the manifest, the texts, and the two
// dependency snapshots — so `--check` runs anywhere, including the Node-only CI
// job. Talking to cargo and npm is `--refresh`, which a human runs.
//
// The one thing rendering *does* look at, when it happens to be present, is the
// macOS library set on disk: every dylib there must be covered by the manifest.
// That is the check that makes this file follow the build instead of drifting
// away from it, and it is why `libs` exists in the manifest at all.

import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const dir = join(root, 'licenses');
const OUT = join(root, 'THIRD-PARTY-NOTICES.md');

const read = (p) => readFileSync(p, 'utf8');
const readJson = (p) => JSON.parse(read(p));

/**
 * Whether a shipped file is the library the manifest calls `name`.
 *
 * Matching is by declared name plus a version suffix, never by stripping digits
 * off the file: the digits in `libX11`, `liblcms2`, `libpng16` and `libpcre2-8`
 * belong to the name, and no regular expression can tell them from the `.6` in
 * `libX11.6.dylib`. Guessing the stem reported three covered libraries as
 * missing on the first run — a gate crying wolf is worse than none, because the
 * fix is to weaken it.
 */
function matches(file, name) {
  const rest = file.slice(name.length);
  return file.startsWith(name) && /^(\.\d+(\.\d+)*)?\.(dylib|dll)$/i.test(rest);
}

/**
 * Every dylib we ship must be named by the manifest. A dependency that appears
 * in the build and not here is the failure this whole file exists to prevent,
 * and it is silent by nature: nothing about an extra dylib announces that it
 * arrived with a license attached.
 */
function verifyClosure(projects) {
  const libDir = join(root, 'src-tauri', 'lib');
  if (!existsSync(libDir)) return null;

  const names = projects.flatMap((p) => p.libs);

  const files = readdirSync(libDir).filter((f) => /\.(dylib|dll)$/i.test(f));
  const missing = files.filter((f) => !names.some((n) => matches(f, n)));
  if (missing.length) {
    console.error(`\nlicenses/manifest.json does not cover ${missing.length} shipped librar${
      missing.length === 1 ? 'y' : 'ies'
    }:\n`);
    for (const f of missing) console.error(`  ${f}`);
    console.error(
      '\nAdd an entry for each, with the license of the artifact as built — not\n' +
        "necessarily the project's headline license. See the manifest's own comment.\n",
    );
    process.exit(1);
  }
  return files.length;
}

function refresh() {
  console.log('==> cargo metadata');
  const meta = JSON.parse(
    execFileSync(
      'cargo',
      ['metadata', '--format-version', '1', '--filter-platform', process.env.FP_TARGET ?? hostTarget()],
      { cwd: join(root, 'src-tauri'), encoding: 'utf8', maxBuffer: 1 << 28 },
    ),
  );
  const pkgs = new Map(meta.packages.map((p) => [p.id, p]));
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  // Normal and build dependencies only: a dev-dependency is a test's, and never
  // reaches a user's machine.
  const seen = new Set();
  const stack = [meta.resolve.root];
  while (stack.length) {
    const id = stack.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    for (const d of nodes.get(id)?.deps ?? []) {
      if (d.dep_kinds.some((k) => k.kind === null || k.kind === 'build')) stack.push(d.pkg);
    }
  }
  seen.delete(meta.resolve.root);

  const crates = [...seen]
    .map((id) => pkgs.get(id))
    .map((p) => ({ name: p.name, version: p.version, license: p.license ?? null }))
    .sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));

  const unlicensed = crates.filter((c) => !c.license);
  if (unlicensed.length) {
    console.error(`crates with no license field: ${unlicensed.map((c) => c.name).join(', ')}`);
    process.exit(1);
  }
  writeFileSync(join(dir, 'rust-crates.json'), `${JSON.stringify(crates, null, 2)}\n`);
  console.log(`    ${crates.length} crates`);

  console.log('==> npm');
  // Runtime dependencies only, plus Svelte: everything else in package.json is a
  // build tool, but Svelte's runtime is compiled into the shipped bundle.
  const pkg = readJson(join(root, 'package.json'));
  const wanted = new Set([...Object.keys(pkg.dependencies ?? {}), 'svelte']);
  const js = [...wanted]
    .map((name) => {
      const p = join(root, 'node_modules', name, 'package.json');
      if (!existsSync(p)) {
        console.error(`node_modules/${name} is not installed — run npm install first`);
        process.exit(1);
      }
      const m = readJson(p);
      return { name, version: m.version, license: m.license ?? null };
    })
    .sort((a, b) => a.name.localeCompare(b.name));
  writeFileSync(join(dir, 'js-deps.json'), `${JSON.stringify(js, null, 2)}\n`);
  console.log(`    ${js.length} packages`);
}

function hostTarget() {
  const arch = process.arch === 'arm64' ? 'aarch64' : 'x86_64';
  return process.platform === 'darwin' ? `${arch}-apple-darwin` : `${arch}-pc-windows-msvc`;
}

/** SPDX ids appearing in an expression like "MIT OR Apache-2.0 WITH LLVM-exception". */
function spdxIds(expr) {
  return expr
    .split(/\s+(?:OR|AND|WITH)\s+|\//)
    .map((t) => t.trim().replace(/^\(|\)$/g, ''))
    .filter(Boolean);
}

function render() {
  const manifest = readJson(join(dir, 'manifest.json'));
  const projects = manifest.projects;
  const crates = readJson(join(dir, 'rust-crates.json'));
  const js = readJson(join(dir, 'js-deps.json'));

  const count = verifyClosure(projects);
  if (count !== null) console.log(`closure: ${count} shipped libraries, all covered`);

  const text = (name) => {
    const p = join(dir, 'text', `${name}.txt`);
    if (!existsSync(p)) {
      console.error(`licenses/text/${name}.txt is missing`);
      process.exit(1);
    }
    return read(p).replace(/\s+$/, '');
  };

  const out = [];
  const w = (s = '') => out.push(s);

  w('# Third-party notices');
  w();
  w('Frame Player is free software under the GNU General Public License, version');
  w('3 or later — its own license text is at the end of this file. What follows');
  w('first is every third-party component the application ships, the license each');
  w('is used under, and those licenses in full.');
  w();
  w('Several of these libraries — mpv, FFmpeg, libplacebo, GLib, FriBidi, Graphite2,');
  w('libbluray, VapourSynth and the gettext runtime — are covered by the GNU Lesser');
  w('General Public License, which is compatible with the GPL this application uses.');
  w('They are **separate shared libraries**, loaded at run time rather than linked');
  w('into the executable, so you may replace any of them with your own build: they');
  w('live in `lib/` beside the application (inside `Contents/Resources/lib` on');
  w('macOS), and the application loads whatever is there.');
  w();
  w('**Source code.** Every component below is unmodified upstream code, and the');
  w('"Source" link for each goes to the project that publishes it. The scripts that');
  w('fetch and build them are part of Frame Player\'s own repository —');
  w('`scripts/build-macos-libs.sh` and `scripts/fetch-libs.ps1` — and record the exact');
  w('versions and configure flags used.');
  w();
  w('No component is licensed under the GNU General Public License. FFmpeg and mpv');
  w('are built with `--disable-gpl` and `-Dgpl=false` respectively, and');
  w('`scripts/check-macos-licenses.sh` refuses to publish a library set in which a');
  w('GPL-only component appears.');
  w();
  w('---');
  w();
  w('## Contents');
  w();
  w(`- [Native libraries](#native-libraries) (${projects.length})`);
  w(`- [Rust crates](#rust-crates) (${crates.length})`);
  w(`- [JavaScript packages](#javascript-packages) (${js.length})`);
  w('- [License texts](#license-texts)');
  w("- [Frame Player's own license](#frame-players-own-license)");
  w();
  w('---');
  w();
  w('## Native libraries');
  w();
  w('| Component | License | Source |');
  w('|---|---|---|');
  for (const p of projects) {
    const plat = p.platforms?.length === 1 ? ` (${p.platforms[0]} only)` : '';
    w(`| ${p.name}${plat} | ${p.license} | [${p.source.replace(/^https?:\/\//, '')}](${p.source}) |`);
  }
  w();
  const noted = projects.filter((p) => p.note);
  if (noted.length) {
    w('Notes on individual components:');
    w();
    for (const p of noted) w(`- **${p.name}** — ${p.note}`);
    w();
  }
  w('---');
  w();
  w('## Rust crates');
  w();
  w('Compiled into the application executable. Development and test dependencies are');
  w('not listed: they do not reach a user\'s machine.');
  w();
  w('| Crate | Version | License |');
  w('|---|---|---|');
  for (const c of crates) w(`| ${c.name} | ${c.version} | ${c.license} |`);
  w();
  w('---');
  w();
  w('## JavaScript packages');
  w();
  w('| Package | Version | License |');
  w('|---|---|---|');
  for (const p of js) w(`| ${p.name} | ${p.version} | ${p.license} |`);
  w();
  w('---');
  w();
  w('## License texts');
  w();
  w('Each native component below is followed by the license text as that project');
  w('publishes it, including its own copyright notices.');
  w();
  for (const p of projects) {
    w(`### ${p.name}`);
    w();
    w(`${p.license} — <${p.url}>`);
    w();
    w('```');
    w(text(p.text));
    w('```');
    w();
  }

  // One canonical text per SPDX id used by the Rust and JavaScript dependencies.
  // Those ecosystems declare a license rather than carrying a distinct text per
  // package, and reproducing four hundred near-identical MIT files would bury the
  // parts of this document a reader might actually need.
  const ids = new Set();
  for (const c of [...crates, ...js]) for (const id of spdxIds(c.license)) ids.add(id);
  w('### Rust and JavaScript dependencies');
  w();
  w('The packages listed above declare the licenses below. Texts are the canonical');
  w('SPDX ones; copyright is held by each package\'s own authors, as recorded in that');
  w('package\'s repository.');
  w();
  for (const id of [...ids].sort()) {
    const p = join(dir, 'spdx', `${id}.txt`);
    if (!existsSync(p)) {
      console.error(`licenses/spdx/${id}.txt is missing — a dependency declares it`);
      process.exit(1);
    }
    w(`#### ${id}`);
    w();
    w('```');
    w(read(p).replace(/\s+$/, ''));
    w('```');
    w();
  }

  w('---');
  w();
  w("## Frame Player's own license");
  w();
  w('Copyright (C) 2026 Evgenii Zakharov');
  w();
  w('This program is free software: you can redistribute it and/or modify it under');
  w('the terms of the GNU General Public License as published by the Free Software');
  w('Foundation, either version 3 of the License, or (at your option) any later');
  w('version. It is distributed in the hope that it will be useful, but WITHOUT ANY');
  w('WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A');
  w('PARTICULAR PURPOSE. See the license below for details.');
  w();
  w('Source code: <https://github.com/risenxxx/frame-player>');
  w();
  w('```');
  // Read from LICENSE rather than restated, so the two cannot disagree: editing
  // one without the other leaves the committed notices stale and the gate fires.
  w(read(join(root, 'LICENSE')).replace(/\s+$/, ''));
  w('```');
  w();

  return `${out.join('\n').replace(/\n{3,}/g, '\n\n').trimEnd()}\n`;
}

const argv = process.argv.slice(2);
if (argv.includes('--refresh')) refresh();

const rendered = render();
const digest = (s) => createHash('sha256').update(s).digest('hex').slice(0, 12);

if (argv.includes('--check')) {
  const current = existsSync(OUT) ? read(OUT) : '';
  if (current !== rendered) {
    console.error(
      `\nTHIRD-PARTY-NOTICES.md is stale (committed ${digest(current)}, ` +
        `rendered ${digest(rendered)}).\nRun: npm run notices\n`,
    );
    process.exit(1);
  }
  console.log(`notices: up to date (${digest(rendered)})`);
} else {
  writeFileSync(OUT, rendered);
  console.log(`wrote THIRD-PARTY-NOTICES.md (${rendered.length.toLocaleString('en-US')} bytes)`);
}
