#!/usr/bin/env node
// Finds scoped CSS rules whose markup is no longer in the same component.
//
// Svelte scopes `<style>` per component, so a rule keeps working only while the
// element it names is rendered by that same file. Move the markup into a child
// and the rule compiles to `.thing.svelte-<parent>` against an element carrying
// `.svelte-<child>` — it matches nothing.
//
// The compiler's own `unused CSS selector` warning does NOT catch this. It is
// computed from the template, and a `<ChildComponent />` tag is opaque to that
// analysis, so Svelte keeps the selector rather than proving it dead. Measured:
// after the start screen moved into `StartScreen.svelte`, `.overlay.start`
// stayed in the page, compiled to `.overlay.start.svelte-1uha8ag`, matched
// nothing, and the build was clean — the start screen had quietly lost its
// flex layout and its scrollbar gutter.
//
// So this checks the thing the compiler will not, in both directions.
//
// Outward: every class a `<style>` block names must appear somewhere in that
// same file's own markup — the rule left behind when its element moved out.
//
// Inward: every class a file's markup *uses* must have a base rule it can
// actually reach, meaning its own `<style>` or `app.css`. A class whose base
// rule sits in some *other* component is styled for that component and nothing
// else, which is the same bug arriving from the opposite side and is invisible
// to the outward check — the rule is still used where it is declared; what
// moved away is the other consumer. Measured: after the split, `.speedopt` kept
// its rule in `ContextMenu.svelte` while the delay stepper had become
// `TrackMenu.svelte`, so its −/+ rendered as native macOS push buttons; the
// same sweep then found `.link-actions`, `.cast-hint`, `.queue-row` and
// `.queue-remove` in the same state. `css-diff` cannot see any of it, because
// it normalises the scope hash away on purpose — to that comparison
// `.speedopt.svelte-a` and `.speedopt.svelte-b` are one rule.
//
//   node scripts/css-orphans.mjs
//
// False positives are possible and deliberate — a class assembled at runtime
// (`class={`row-${kind}`}`) cannot be seen here. Read each hit; the fix is
// usually to move the rule to where its markup went, or to `app.css` when two
// components genuinely share the vocabulary.

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';

const ROOT = resolve(import.meta.dirname, '..');
const SRC = join(ROOT, 'src');

function walk(dir) {
  return readdirSync(dir).flatMap((name) => {
    const full = join(dir, name);
    return statSync(full).isDirectory() ? walk(full) : full.endsWith('.svelte') ? [full] : [];
  });
}

/**
 * Tag names this file's own markup renders.
 *
 * Needed because a selector does not have to name a class to be orphaned:
 * `input[type='range']` styled the settings sliders and the volume bar, and
 * when the settings dialog moved out of the page it left the rule behind. The
 * check saw no class in the selector, said nothing, and the sliders quietly
 * reverted to the platform's own — reported as "they look almost stock now".
 *
 * Only the tag is matched, not the attribute filter: proving that some `<input>`
 * in the file is a range would mean evaluating the template. A file that renders
 * no `<input>` at all cannot own the rule, and that is the case worth catching.
 */
function markupTags(markup) {
  return new Set([...markup.matchAll(/<([a-z][a-z0-9]*)[\s/>]/g)].map((m) => m[1]));
}

/** Classes this file's own markup can put on an element. */
function markupClasses(markup) {
  const found = new Set();
  for (const [, list] of markup.matchAll(/\bclass\s*=\s*"([^"]*)"/g)) {
    // Static names only; `{...}` interpolations are handled by the sweep below.
    for (const c of list.replace(/\{[^}]*\}/g, ' ').split(/\s+/)) if (c) found.add(c);
  }
  for (const [, name] of markup.matchAll(/\bclass:([\w-]+)/g)) found.add(name);
  // `class={cond ? 'a' : 'b'}` and `class="x {y}"` — take every quoted word
  // inside a class expression, plus bare identifiers used as class strings.
  for (const [, expr] of markup.matchAll(/\bclass\s*=\s*\{([^}]*)\}/g)) {
    for (const [, s] of expr.matchAll(/'([^']*)'|"([^"]*)"/g)) {
      for (const c of (s ?? '').split(/\s+/)) if (c) found.add(c);
    }
  }
  return found;
}

/**
 * Classes that share an element with a runtime-built class name.
 *
 * `class="diag-line {check.state}"` can put anything next to `diag-line`, so
 * `.diag-line.ok` is not evidence of a missing element — the modifier simply
 * cannot be read statically. Anchoring on the static neighbour keeps the check
 * quiet about those without blinding it: `.overlay.start` is still reported,
 * because no element here writes `class="overlay {…}"`.
 */
function dynamicNeighbours(markup) {
  const found = new Set();
  for (const [, list] of markup.matchAll(/\bclass\s*=\s*"([^"]*)"/g)) {
    if (!list.includes('{')) continue;
    for (const c of list.replace(/\{[^}]*\}/g, ' ').split(/\s+/)) if (c) found.add(c);
  }
  return found;
}

/**
 * Keyframe names a stylesheet may reference.
 *
 * Svelte scopes `@keyframes` and rewrites the `animation` shorthand that uses
 * it — but only within the same component. Leave the keyframes behind when the
 * animated markup moves out and the rule is left pointing at a name that no
 * longer exists: the compiler says nothing, and the animation silently stops.
 * This has now happened four times in a row (`spin`, `torchip-pulse`, `osd-in`
 * and `skip-in`, `end-advance`), always found only in the compiled bundle, so
 * it gets a check of its own.
 *
 * Names defined in app.css are global and may be used from anywhere.
 */
const GLOBAL_KEYFRAMES = new Set(
  [...readFileSync(join(SRC, 'app.css'), 'utf8').matchAll(/@keyframes\s+([\w-]+)/g)].map(
    (m) => m[1],
  ),
);

/**
 * Classes a stylesheet gives a *base* rule — one whose final compound is the
 * class on its own.
 *
 * `.queue-row .menu-item` and `.menu-item.sel` both mention `menu-item`, and
 * neither is what makes a menu item look like one; only `.menu-item { … }` is.
 * That distinction is the whole reason the inward check is quiet: every
 * modifier a component adds to a shared class (`.active`, `.mini`, `.warn`)
 * would otherwise be reported the moment two components used it.
 *
 * It has to be the **whole** selector, not merely its last compound. Judging by
 * the last compound alone was the first attempt and it was silent on the very
 * bug that prompted this: `.delayrow .speedopt` ends in a bare `.speedopt`, so
 * the file that had *only* the override looked like the one that owned it.
 */
function baseClasses(css) {
  const out = new Set();
  for (const [, rawSel] of css.matchAll(/([^{}]+)\{/g)) {
    const sel = rawSel.trim();
    if (!sel || sel.startsWith('@') || /^\d|^from\b|^to\b/.test(sel)) continue;
    for (const part of sel.split(',')) {
      // Nothing is stripped but `:global()`. A pseudo is not a base rule
      // either: `.x:hover` says what `.x` does under the pointer and nothing
      // about what it looks like at rest, and treating it as ownership was the
      // second reason this check stayed silent — `.speedopt:hover` had stayed
      // behind in app.css and stood in for the rule that had not.
      const core = part.replace(/:global\(([^)]*)\)/g, '$1').trim();
      if (/^\.[a-zA-Z][\w-]*$/.test(core)) out.add(core.slice(1));
    }
  }
  return out;
}

const APP_CSS = readFileSync(join(SRC, 'app.css'), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');
const GLOBAL_BASE = baseClasses(APP_CSS);

/** Per-file record for the inward pass, which needs every file before it can judge one. */
const seen = [];

let problems = 0;
for (const file of walk(SRC)) {
  const src = readFileSync(file, 'utf8');
  const styleAt = src.lastIndexOf('<style');
  if (styleAt === -1) {
    seen.push({ file, used: markupClasses(src), base: new Set() });
    continue;
  }
  const style = src.slice(src.indexOf('>', styleAt) + 1, src.lastIndexOf('</style>'));
  const markup = src.slice(0, styleAt);
  seen.push({
    file,
    used: markupClasses(markup),
    base: baseClasses(style.replace(/\/\*[\s\S]*?\*\//g, '')),
  });
  const own = markupClasses(markup);
  const dynamic = dynamicNeighbours(markup);
  const tags = markupTags(markup);

  const clean = style.replace(/\/\*[\s\S]*?\*\//g, '');
  const defined = new Set([...clean.matchAll(/@keyframes\s+([\w-]+)/g)].map((m) => m[1]));
  for (const [, shorthand] of clean.matchAll(/\banimation(?:-name)?\s*:\s*([^;}]+)/g)) {
    for (const word of shorthand.split(/[\s,]+/)) {
      // The shorthand mixes durations, easings and the name; only a bare
      // identifier that is neither a keyword nor a time can be the name.
      if (!/^[a-zA-Z][\w-]*$/.test(word)) continue;
      if (/^(none|infinite|alternate|reverse|normal|forwards|backwards|both|linear|ease|ease-in|ease-out|ease-in-out|running|paused|step-start|step-end|var)$/.test(word)) continue;
      if (defined.has(word) || GLOBAL_KEYFRAMES.has(word)) continue;
      console.log(`${relative(ROOT, file)}: animation: ${word}   [no @keyframes here or in app.css]`);
      problems++;
    }
  }

  for (const [, rawSel] of clean.matchAll(/([^{}]+)\{/g)) {
    const sel = rawSel.trim();
    if (!sel || sel.startsWith('@') || sel.startsWith('from') || sel.startsWith('to')) continue;
    if (/^\d/.test(sel)) continue; // keyframe percentages
    for (const part of sel.split(',')) {
      // `:global(...)` deliberately escapes scoping; its contents are not ours.
      const scoped = part.replace(/:global\([^)]*\)/g, '');
      const classes = [...scoped.matchAll(/\.([a-zA-Z][\w-]*)/g)].map((m) => m[1]);
      const missing = classes.filter((c) => !own.has(c));
      // A classless selector is judged by the tags it names instead.
      if (!classes.length) {
        const named = [...scoped.matchAll(/(?:^|[\s>+~(])([a-z][a-z0-9]*)(?=[\s.:[#>+~)]|$)/g)]
          .map((m) => m[1])
          .filter((n) => !['from', 'to', 'and', 'not', 'where', 'is'].includes(n));
        const absent = named.filter((n) => !tags.has(n));
        if (named.length && absent.length) {
          console.log(`${relative(ROOT, file)}: ${part.trim()}   [no markup for: <${absent.join('>, <')}>]`);
          problems++;
        }
        continue;
      }
      // A selector anchored on an element whose class list is partly built at
      // runtime cannot be judged from here.
      if (classes.some((c) => dynamic.has(c))) continue;
      if (classes.length && missing.length) {
        console.log(`${relative(ROOT, file)}: ${part.trim()}   [no markup for: ${missing.join(', ')}]`);
        problems++;
      }
    }
  }
}

// ---- Inward: a class used here whose base rule is scoped to another component.
const owners = new Map();
for (const { file, base } of seen) {
  for (const c of base) (owners.get(c) ?? owners.set(c, []).get(c)).push(file);
}
for (const { file, used, base } of seen) {
  for (const c of used) {
    if (base.has(c) || GLOBAL_BASE.has(c)) continue;
    const elsewhere = (owners.get(c) ?? []).filter((f) => f !== file);
    if (!elsewhere.length) continue;
    console.log(
      `${relative(ROOT, file)}: .${c} used here, base rule scoped to ` +
        `${elsewhere.map((f) => relative(ROOT, f)).join(', ')}   [move it to app.css]`,
    );
    problems++;
  }
}

console.log(problems ? `\n${problems} orphaned selector(s).` : '\nno orphaned selectors.');
process.exit(problems ? 1 : 0);
