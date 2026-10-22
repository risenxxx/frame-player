#!/usr/bin/env node
// Holds the three READMEs to one another.
//
// The README is in English, Russian and Simplified Chinese, and the failure a
// translation invites is silent drift: a section added to one file and not the
// others, or an in-page link that still points at the English anchor. GitHub
// derives an anchor from the heading text, so `## Установка` is `#установка`
// and a `(#install)` copied across from the English leads nowhere — the page
// renders, the link just does not move.
//
// So, for each file: every `#anchor` link must name a heading in that same file
// (slugged the way GitHub slugs them), every relative link must name a file that
// exists, and the language switcher must reach the other two. Across files: the
// headings must have the same levels in the same order, which is what "the same
// document" means once the words differ.

import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const FILES = ['README.md', 'README.ru.md', 'README.zh-CN.md'];

let problems = 0;
const fail = (file, msg) => {
  problems++;
  console.log(`${file}: ${msg}`);
};

/** Markdown with fenced code blocks blanked out, line count preserved. */
function prose(text) {
  let fenced = false;
  return text
    .split('\n')
    .map((line) => {
      if (/^\s*(```|~~~)/.test(line)) {
        fenced = !fenced;
        return '';
      }
      return fenced ? '' : line;
    })
    .join('\n');
}

/** GitHub's heading anchor: lower-cased, punctuation dropped, spaces to hyphens. */
function slug(heading) {
  return heading
    .replace(/<[^>]+>/g, '')
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/[`*]/g, '')
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\p{M}\s_-]/gu, '')
    .replace(/ /g, '-');
}

function headings(text) {
  const out = [];
  for (const line of prose(text).split('\n')) {
    const md = line.match(/^(#{1,6})\s+(.+?)\s*#*\s*$/);
    if (md) out.push({ level: md[1].length, text: md[2] });
    const html = line.match(/<h([1-6])[^>]*>(.*?)<\/h\1>/);
    if (html) out.push({ level: Number(html[1]), text: html[2] });
  }
  return out;
}

const docs = FILES.map((file) => {
  const path = join(root, file);
  if (!existsSync(path)) {
    fail(file, 'missing');
    return null;
  }
  return { file, text: readFileSync(path, 'utf8') };
}).filter(Boolean);

for (const { file, text } of docs) {
  const hs = headings(text);
  const anchors = new Set();
  const seen = new Map();
  for (const h of hs) {
    const base = slug(h.text);
    const n = seen.get(base) ?? 0;
    seen.set(base, n + 1);
    anchors.add(n ? `${base}-${n}` : base);
  }

  const body = prose(text);
  const links = [
    ...[...body.matchAll(/\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g)].map((m) => m[1]),
    ...[...body.matchAll(/\b(?:href|src)="([^"]+)"/g)].map((m) => m[1]),
  ];
  for (const link of links) {
    if (/^[a-z]+:/i.test(link)) continue;
    const [path, hash] = link.split('#');
    if (path && !existsSync(join(root, decodeURI(path)))) fail(file, `links to a missing file: ${link}`);
    if (!path && hash !== undefined && !anchors.has(decodeURIComponent(hash).toLowerCase()))
      fail(file, `links to #${hash}, which no heading here produces`);
  }

  for (const other of FILES) {
    if (other !== file && !links.includes(other)) fail(file, `the language switcher does not reach ${other}`);
  }
}

if (docs.length === FILES.length) {
  const shape = (d) => headings(d.text).map((h) => h.level).join(' ');
  const [first, ...rest] = docs;
  for (const d of rest) {
    if (shape(d) === shape(first)) continue;
    const a = headings(first.text);
    const b = headings(d.text);
    const at = a.findIndex((h, i) => b[i]?.level !== h.level);
    const where = at < 0 ? a.length : at;
    fail(
      d.file,
      `headings differ from ${first.file} at #${where + 1}: ` +
        `${a[where] ? `h${a[where].level} “${a[where].text}”` : 'end'} vs ` +
        `${b[where] ? `h${b[where].level} “${b[where].text}”` : 'end'}`,
    );
  }
}

if (problems) {
  console.log(`\n${problems} problem(s).`);
  process.exit(1);
}
console.log(`readme: ${docs.length} languages, same headings, every link resolves.`);
