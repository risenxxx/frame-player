#!/usr/bin/env node
// Three mistakes a refactor leaves behind that no tool reports. The third is
// not about runes and did not deserve a script of its own — it is the same
// failure: something moved out and its declaration stayed.
//
// ## 1. A `$effect` at the top level of a `.svelte.ts` module
//
// It compiles, `svelte-check` reports nothing, `npm run build` succeeds — and
// the app throws `effect_orphan` the instant the module is imported, which for
// a state module imported by the page means a window that never paints. Nothing
// in the toolchain says a word about it, because the mistake is only a mistake
// at runtime: `$effect` needs an active effect, and module evaluation has none.
//
// Measured against the real compiler rather than assumed — `compileModule` on
//
//   let n = $state(0);
//   $effect(() => console.log(n));
//
// emits a bare `$.user_effect(…)` at module scope, and importing the result
// throws `https://svelte.dev/e/effect_orphan`.
//
// The fix is always the same shape: wrap it in an exported `initX()` and call
// that from a component's initialisation, as `trackTorrentPlayback` and
// `initChrome` do.
//
//   node scripts/check-runes.mjs
//
// What counts as orphaned is a `$effect` **statement** at brace depth 0. An
// effect inside a function is somebody's initialiser and is the whole point;
// `const f = () => $effect(…)` is one too, since an arrow body does not run
// until it is called. So the token is only reported when nothing but `;` or `}`
// precedes it on its line — which is what tells a statement from an expression
// without parsing the file.
//
// ## 2. A `$state` nothing ever writes
//
// The other half of a refactor going quiet. State declared in one file and
// assigned from another is normal here — that is what `chrome.overlayOpen` is —
// but a `let x = $state(…)` that its *own* file only ever reads is a write that
// went missing. Twice now: `chrome.overlayOpen` replaced an expression and
// nothing assigned the replacement, so the chrome faded out from under an open
// menu; and `updateInputEl` kept its declaration and its `?.focus()` while the
// `bind:this` stayed behind in the markup that moved into a component, so the
// torrent update dialog silently stopped focusing its field. Neither is an
// error to any tool — an optional chain on `undefined` is valid, and a boolean
// that stays false is a perfectly good boolean.
//
// Only same-file writes count as writes, so this is deliberately narrow: it
// says nothing about class fields, which are the cross-file case. A `$state`
// whose initialiser is the whole point and which is never reassigned should be
// a `const` and will be reported until it is one.

// ## 3. A declaration whose code left
//
// Moving a block into a module and forgetting the `let` it used costs nothing
// at runtime and nothing at build time — an unread variable is legal. What it
// costs is a reader, who has no way to tell a live piece of state from the
// corpse of one. Five of them survived the pointer gestures moving into
// `input.svelte.ts`, all plain `let`s, which is why this is separate from the
// `$state` check above.

import { readdirSync, readFileSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';

const ROOT = resolve(import.meta.dirname, '..');
const SRC = join(ROOT, 'src');

function walk(dir, ext) {
  const out = [];
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) out.push(...walk(p, ext));
    else if (ext.some((x) => e.name.endsWith(x))) out.push(p);
  }
  return out;
}

/**
 * Every `$effect` token that is real code — not inside a string, a template
 * literal or a comment — with the brace depth it sits at and the line it is on.
 *
 * A hand-rolled scanner rather than a parse: all it has to get right is `{`,
 * `}` and where code stops being code, and getting that wrong would drift the
 * depth for the rest of the file and produce a loud false report rather than a
 * quiet miss.
 */
function effectTokens(src) {
  const out = [];
  let depth = 0;
  let i = 0;
  let line = 0;
  let lineStart = 0;
  let quote = null; // ' " ` or null
  let comment = null; // '//' | '/*' | null
  while (i < src.length) {
    const ch = src[i];
    const next = src[i + 1];
    if (ch === '\n') {
      line++;
      lineStart = i + 1;
      if (comment === '//') comment = null;
      i++;
      continue;
    }
    if (comment === '//') { i++; continue; }
    if (comment === '/*') {
      if (ch === '*' && next === '/') { comment = null; i += 2; } else i++;
      continue;
    }
    if (quote) {
      if (ch === '\\') { i += 2; continue; }
      if (ch === quote) quote = null;
      i++;
      continue;
    }
    if (ch === '/' && next === '/') { comment = '//'; i += 2; continue; }
    if (ch === '/' && next === '*') { comment = '/*'; i += 2; continue; }
    if (ch === "'" || ch === '"' || ch === '`') { quote = ch; i++; continue; }
    if (ch === '{') { depth++; i++; continue; }
    if (ch === '}') { depth--; i++; continue; }
    if (ch === '$' && src.startsWith('$effect', i) && !/[.\w]/.test(src[i - 1] ?? '')) {
      out.push({ line, depth, before: src.slice(lineStart, i) });
      i += '$effect'.length;
      continue;
    }
    i++;
  }
  return out;
}

const bad = [];
for (const file of walk(SRC, ['.svelte.ts'])) {
  const src = readFileSync(file, 'utf8');
  if (!src.includes('$effect')) continue;
  const lines = src.split('\n');
  for (const tok of effectTokens(src)) {
    if (tok.depth !== 0) continue;
    // An expression — an arrow body, an argument — is not a statement, and only
    // a statement runs at import time.
    if (!/^[\s;}]*$/.test(tok.before)) continue;
    bad.push(`${relative(ROOT, file)}:${tok.line + 1}  ${lines[tok.line].trim()}`);
  }
}

const unwritten = [];
for (const file of walk(SRC, ['.svelte', '.svelte.ts'])) {
  const src = readFileSync(file, 'utf8');
  for (const m of src.matchAll(/\blet\s+([A-Za-z_]\w*)(?:\s*:[^=]+)?\s*=\s*\$state\b/g)) {
    const name = m.group?.(1) ?? m[1];
    const rest = src.slice(0, m.index) + src.slice(m.index + m[0].length);
    // An assignment to the bare name, or a binding that hands the element or a
    // field of it back. `bind:x={a.b}` writes `a`, so the field form counts too.
    const assigned =
      new RegExp(`(?<![=!<>])\\b${name}\\s*(=(?!=)|\\+\\+|--|\\+=|-=)`).test(rest) ||
      new RegExp(`bind:\\w+=\\{${name}(\\.\\w+|\\[)?`).test(src);
    if (assigned) continue;
    const line = src.slice(0, m.index).split('\n').length;
    unwritten.push(`${relative(ROOT, file)}:${line}  ${name}`);
  }
}

const dead = [];
for (const file of walk(SRC, ['.svelte', '.svelte.ts'])) {
  const src = readFileSync(file, 'utf8');
  // Indentation is the scope test, and it means different things in the two
  // file kinds: a module's own declarations sit at column 0, a component's sit
  // at two inside `<script>`. Getting that wrong reads a function body as
  // component scope — which is how this check first reported a perfectly live
  // local in `diagnosisText`.
  const scope = file.endsWith('.svelte') ? /^ {2}(?:let|const)\s+([A-Za-z_]\w*)/gm : /^(?:let|const)\s+([A-Za-z_]\w*)/gm;
  // A spread is not a property access. `...head` has a dot in front of it and
  // the usual `(?<![.\w])` guard threw the use away, which was the other half of
  // that false report.
  const counted = src.replace(/\.\.\./g, ' ');
  for (const m of src.matchAll(scope)) {
    const name = m[1];
    const uses = counted.match(new RegExp(`(?<![.\\w])${name}\\b`, 'g'))?.length ?? 0;
    if (uses > 1) continue;
    const line = src.slice(0, m.index).split('\n').length;
    dead.push(`${relative(ROOT, file)}:${line}  ${name}`);
  }
}

for (const b of bad) console.log(`  ! top-level $effect: ${b}`);
for (const u of unwritten) console.log(`  ! $state never written in its own file: ${u}`);
for (const d of dead) console.log(`  ! declared and never used: ${d}`);
const problems = bad.length + unwritten.length + dead.length;
console.log(
  problems
    ? `\n${bad.length} orphan effect(s), ${unwritten.length} unwritten state, ${dead.length} dead — ` +
        `wrap effects in an init function; make read-only state a const, or find the write that went missing.`
    : 'runes: no top-level $effect, no unwritten $state, nothing declared and unused.',
);
process.exit(problems ? 1 : 0);
