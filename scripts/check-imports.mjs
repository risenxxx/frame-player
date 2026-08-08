#!/usr/bin/env node
// Import cycles in `src/`.
//
// "Keep the dependency direction one-way" has been a rule here since the state
// modules were introduced, and nothing enforced it — a cycle costs no error, no
// warning and no build failure, because a bundler resolves it. What it costs is
// a module-evaluation order nobody chose: whichever file is imported first wins,
// and the other one sees `undefined` for anything it reads at module scope. That
// is a crash on startup with a stack trace pointing at the innocent party.
//
//   node scripts/check-imports.mjs
//
// One cycle existed when this was written — `thumbs` ⇄ `torrent`, alive for
// months — and only the *notification* half was wrong: `thumbs` asking the
// torrent client where the data is (`positionBuffered`) is a fair question about
// its state, while the torrent client telling the thumbnail service to start is
// knowledge it has no business having. The fix that generalises is to turn the
// notification into a callback the composition root supplies, not to invent a
// third module for the pair to share.

import { readdirSync, readFileSync, existsSync } from 'node:fs';
import { dirname, join, normalize, relative, resolve } from 'node:path';

const ROOT = resolve(import.meta.dirname, '..');
const SRC = join(ROOT, 'src');

function walk(dir) {
  const out = [];
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) out.push(...walk(p));
    else if (/\.(ts|svelte)$/.test(e.name)) out.push(p);
  }
  return out;
}

/** Resolve an import specifier the way the bundler does, or null for a package. */
function resolveTo(fromFile, spec) {
  const base = spec.startsWith('$lib/')
    ? join(SRC, 'lib', spec.slice(5))
    : spec.startsWith('.')
      ? normalize(join(dirname(fromFile), spec))
      : null;
  if (!base) return null;
  // `.svelte.ts` before `.ts`: `./player.svelte` must not resolve to a
  // hypothetical `player.svelte` component when the module is what is meant.
  for (const ext of ['.ts', '.svelte', '']) {
    if (existsSync(base + ext)) return base + ext;
  }
  return null;
}

const graph = new Map();
for (const file of walk(SRC)) {
  const src = readFileSync(file, 'utf8');
  const deps = new Set();
  for (const [, spec] of src.matchAll(/from\s+'([^']+)'/g)) {
    const target = resolveTo(file, spec);
    if (target && target !== file) deps.add(target);
  }
  graph.set(file, deps);
}

// Depth-first, coloring nodes: gray means "on the current path", which is what
// a back edge points at.
const color = new Map();
const stack = [];
const cycles = new Map();
function visit(node) {
  color.set(node, 1);
  stack.push(node);
  for (const dep of [...(graph.get(node) ?? [])].sort()) {
    if (color.get(dep) === 1) {
      const loop = stack.slice(stack.indexOf(dep)).concat(dep);
      cycles.set([...loop].sort().join('|'), loop);
    } else if (!color.has(dep)) {
      visit(dep);
    }
  }
  stack.pop();
  color.set(node, 2);
}
for (const node of [...graph.keys()].sort()) if (!color.has(node)) visit(node);

for (const loop of cycles.values()) {
  console.log(`  ! cycle: ${loop.map((f) => relative(SRC, f)).join(' -> ')}`);
}
console.log(
  cycles.size
    ? `\n${cycles.size} import cycle(s) — turn the notification half into a callback the page supplies.`
    : 'imports: no cycles.',
);
process.exit(cycles.size ? 1 : 0);
