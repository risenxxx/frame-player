#!/usr/bin/env node
// Proves that a frontend refactor did not change a single rendered style.
//
// Moving markup into a component moves its CSS with it, and Svelte scopes
// `<style>` per component — so a selector whose ancestor stays behind while its
// descendant leaves silently stops matching. Svelte does warn (`unused CSS
// selector`) and `npm run check` is the cheap gate for that, but the warning is
// easy to lose in a long run and says nothing about a rule whose *value*
// drifted while it was being retyped by hand.
//
// So this compares the built stylesheets instead — the compiled order and the
// compiled text are what actually decide, which is the same reason the queue-row
// and settings-sheet measurements in CLAUDE.md were taken against the bundle
// rather than a hand-written copy of the rules.
//
//   node scripts/css-diff.mjs [ref]      # ref defaults to HEAD
//
// It builds the working tree, builds `ref` in a throwaway git worktree, and
// reports:
//   * rules present in one build and not the other, hashes normalised away
//   * selectors that span two style scopes, which is the broken-boundary shape
//     a `:global()` escape hatch can smuggle past the compiler's own warning
//
// An empty report means the refactor was style-preserving. A non-empty one is
// not automatically wrong — folding a descendant override into the rule it beat
// shows up here and is fine — but every line has to be explained.

import { execFileSync } from 'node:child_process';
import { mkdtempSync, readdirSync, readFileSync, rmSync, symlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const ROOT = resolve(import.meta.dirname, '..');
const ref = process.argv[2] ?? 'HEAD';

const run = (cmd, args, cwd) =>
  execFileSync(cmd, args, { cwd, stdio: ['ignore', 'pipe', 'inherit'] }).toString();

/** Concatenate every stylesheet a build emitted, in name order. */
function builtCss(dir) {
  const assets = join(dir, 'build/_app/immutable/assets');
  return readdirSync(assets)
    .filter((f) => f.endsWith('.css'))
    .sort()
    .map((f) => readFileSync(join(assets, f), 'utf8'))
    .join('\n');
}

/**
 * Split a stylesheet into top-level rules with scope hashes stripped. `@media`
 * blocks stay whole, which is enough: nothing here moves a rule in or out of
 * one, and treating the block as a unit keeps the comparison honest about
 * nesting.
 */
function rules(css) {
  const flat = css.replace(/\.svelte-[a-z0-9]+/g, '').replace(/:where\(\)/g, '');
  const out = [];
  let depth = 0;
  let buf = '';
  for (const ch of flat) {
    buf += ch;
    if (ch === '{') depth++;
    else if (ch === '}' && --depth === 0) {
      out.push(buf.split(/\s+/).join(' ').trim());
      buf = '';
    }
  }
  return out;
}

/** Selectors mentioning two different scope hashes never match anything. */
function crossScope(css) {
  const bad = [];
  for (const [, sel] of css.matchAll(/([^{}]+)\{/g)) {
    if (sel.trim().startsWith('@')) continue;
    for (const part of sel.split(',')) {
      const hashes = new Set([...part.matchAll(/svelte-([a-z0-9]+)/g)].map((m) => m[1]));
      if (hashes.size > 1) bad.push(part.trim());
    }
  }
  return bad;
}

/**
 * Rules that could fight over the same element, and whether their relative
 * order changed.
 *
 * Moving a rule from a component into `app.css` (or into another component)
 * changes where it lands in the concatenated stylesheet. That is harmless right
 * up until two rules of **equal specificity** set the same property on the same
 * element, where the cascade falls back to source order — the exact arithmetic
 * that put `display: block` over the queue rows' `display: flex` and sent the
 * torrent list's delete button to the corner of the window. The rule-set diff
 * cannot see it, because both rules are still present and unchanged.
 *
 * So: for every pair sharing a class name and carrying the same specificity,
 * check that whoever won before still wins. Only pairs that actually declare a
 * property in common are reported — the rest cannot conflict.
 */
function orderConflicts(before, after) {
  const parse = (list) =>
    list.map((r, i) => {
      const cut = r.indexOf('{');
      const sel = r.slice(0, cut);
      const props = new Set(
        r
          .slice(cut + 1, r.lastIndexOf('}'))
          .split(';')
          .map((d) => d.split(':')[0].trim())
          .filter(Boolean),
      );
      const classes = [...sel.matchAll(/\.([a-zA-Z][\w-]*)/g)].map((m) => m[1]);
      // Good enough for the comparison at hand: rules differing in specificity
      // are decided by it, not by order, so only equal counts matter here.
      const spec = `${classes.length}|${(sel.match(/:{1,2}[a-z-]+/g) ?? []).length}`;
      return { i, sel: sel.trim(), props, classes, spec };
    });

  const [a, b] = [parse(before), parse(after)];
  const rank = new Map(b.map((r) => [r.sel, r.i]));
  const out = [];
  for (let x = 0; x < a.length; x++) {
    for (let y = x + 1; y < a.length; y++) {
      const [p, q] = [a[x], a[y]];
      if (p.spec !== q.spec) continue;
      if (!p.classes.some((c) => q.classes.includes(c))) continue;
      const shared = [...p.props].filter((k) => q.props.has(k));
      if (!shared.length) continue;
      const [np, nq] = [rank.get(p.sel), rank.get(q.sel)];
      if (np === undefined || nq === undefined) continue;
      // `q` came later in `before`, so `q` used to win. If `p` is later now,
      // the winner changed.
      if (np > nq) out.push(`${p.sel}  now beats  ${q.sel}   [${shared.join(', ')}]`);
    }
  }
  return out;
}

function diff(before, after) {
  const count = (list) => {
    const m = new Map();
    for (const r of list) m.set(r, (m.get(r) ?? 0) + 1);
    return m;
  };
  const [a, b] = [count(before), count(after)];
  // `Math.max`, not `|| 0`: a negative count is falsy only at zero, so a rule
  // that lost a duplicate reached `Array(-1)` and threw.
  const missing = [...a].flatMap(([r, n]) => Array(Math.max(0, n - (b.get(r) ?? 0))).fill(r));
  const added = [...b].flatMap(([r, n]) => Array(Math.max(0, n - (a.get(r) ?? 0))).fill(r));
  return { missing, added };
}

console.log(`building working tree…`);
run('npm', ['run', 'build'], ROOT);
const after = builtCss(ROOT);

const tmp = mkdtempSync(join(tmpdir(), 'fp-cssdiff-'));
const base = join(tmp, 'base');
let before;
try {
  console.log(`building ${ref} in a throwaway worktree…`);
  run('git', ['worktree', 'add', '--detach', base, ref], ROOT);
  // node_modules is 100s of MB and identical; a symlink is the whole point of
  // doing this locally rather than in CI.
  symlinkSync(join(ROOT, 'node_modules'), join(base, 'node_modules'));
  run('npm', ['run', 'build'], base);
  before = builtCss(base);
} finally {
  run('git', ['worktree', 'remove', '--force', base], ROOT);
  rmSync(tmp, { recursive: true, force: true });
}

const [ra, rb] = [rules(before), rules(after)];
const { missing, added } = diff(ra, rb);
const cross = crossScope(after);
const flipped = orderConflicts(ra, rb);

console.log(`\nrules: ${ra.length} in ${ref}, ${rb.length} now`);
for (const r of missing) console.log(`  - ${r}`);
for (const r of added) console.log(`  + ${r}`);
for (const s of cross) console.log(`  ! cross-scope selector: ${s}`);
for (const s of flipped) console.log(`  ~ cascade order flipped: ${s}`);

const clean = !missing.length && !added.length && !cross.length && !flipped.length;
console.log(
  clean
    ? '\nno style changes.'
    : `\n${missing.length + added.length} rule change(s), ${cross.length} broken selector(s), ${flipped.length} order flip(s) — explain each.`,
);
process.exit(cross.length || flipped.length ? 1 : 0);
