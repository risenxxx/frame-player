/**
 * The issue report's text, from parts already gathered — pure, so what may
 * appear in it can be tested without a player. Gathering is
 * `report.svelte.ts`'s; taking things *out* of the finished text is
 * `redact.ts`'s, and it runs over everything this produces.
 *
 * English on purpose, like the cast device check: it is read by whoever fixes
 * the bug, and an issue is searched in one language.
 */

export interface ReportParts {
  generatedAt: Date;
  summary: Array<[string, string]>;
  playback: Array<[string, string]>;
  casting: Array<[string, string]> | null;
  settings: string[];
  frontend: { lines: string[]; dropped: number };
  native: { lines: string[]; dropped: number; capture: string };
  mpv: { source: string; tail: string; total: number; shown: number } | null;
  mpvNote: string | null;
}

/**
 * Settings, with only the values that cannot identify anyone.
 *
 * A setting is printed when its value is a flag, a number or a short word —
 * `encryption=prefer`, `fitToVideo=true` — and as `set`/`empty` otherwise, or
 * when its key says it is an address, a path, a credential or a name. That is
 * the shape of a choice rather than of a person: a proxy URL, a torrent
 * folder, a relay address and an indexer key all fail it twice over.
 */
const SENSITIVE_KEY = /key|token|secret|pass|proxy|url|dir|path|root|host|name|relay|indexer|tmdb|cdn|addr|device|offset/i;
const PLAIN_WORD = /^[\w.+-]{1,24}$/;

export function describeValue(key: string, value: unknown, depth = 0): string {
  if (value === null || value === undefined) return 'null';
  if (typeof value === 'boolean' || typeof value === 'number') {
    return SENSITIVE_KEY.test(key) && typeof value !== 'boolean' ? 'set' : String(value);
  }
  if (typeof value === 'string') {
    if (value === '') return 'empty';
    return !SENSITIVE_KEY.test(key) && PLAIN_WORD.test(value) ? value : 'set';
  }
  if (Array.isArray(value)) return `[${value.length}]`;
  if (typeof value === 'object') {
    const entries = Object.entries(value as Record<string, unknown>);
    if (depth > 0 || SENSITIVE_KEY.test(key)) return `{${entries.length}}`;
    return `{${entries.map(([k, v]) => `${k}=${describeValue(k, v, depth + 1)}`).join(' ')}}`;
  }
  return 'set';
}

/// One line per store: `name: field=value field=value`. A store that is not
/// JSON is one value under its own name.
export function settingsLines(stores: Array<[string, string | null]>): string[] {
  const out: string[] = [];
  for (const [name, raw] of stores) {
    if (raw === null) continue;
    let parsed: unknown = raw;
    try {
      parsed = JSON.parse(raw);
    } catch {
      // A bare string — `frameplayer.locale` is one.
    }
    if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
      const entries = Object.entries(parsed as Record<string, unknown>);
      out.push(`${name}: ${entries.map(([k, v]) => `${k}=${describeValue(k, v)}`).join(' ') || '(empty)'}`);
    } else {
      out.push(`${name}: ${describeValue(name, parsed)}`);
    }
  }
  return out;
}

function table(rows: Array<[string, string]>): string[] {
  const width = Math.max(0, ...rows.map(([k]) => k.length));
  return rows.map(([k, v]) => `${`${k}:`.padEnd(width + 2)}${v}`);
}

export function kib(bytes: number): string {
  return bytes >= 1 << 20 ? `${(bytes / (1 << 20)).toFixed(1)} MiB` : `${Math.round(bytes / 1024)} KiB`;
}

export function utc(date: Date): string {
  return `${date.toISOString().slice(0, 19).replace('T', ' ')} UTC`;
}

export function assemble(p: ReportParts): string {
  const out: string[] = [
    'Frame Player issue report',
    `Generated ${utc(p.generatedAt)}. Paths, file names, links, addresses, device names and identifiers are replaced`,
    '(<path>.mkv, lan-1, uuid-1, <device>, …) before this text is shown; it is shared only as shown.',
    '',
    '## Summary',
    ...table(p.summary),
    '',
    '## Playback',
    ...table(p.playback),
  ];
  if (p.casting) out.push('', '## Casting', ...table(p.casting));
  out.push('', '## Settings', ...(p.settings.length ? p.settings : ['(defaults)']));

  const dropped = (n: number) => (n ? `, ${n} older dropped` : '');
  out.push(
    '',
    `## Interface log (${p.frontend.lines.length} lines${dropped(p.frontend.dropped)})`,
    ...p.frontend.lines,
  );
  out.push(
    '',
    `## Native log (${p.native.lines.length} lines${dropped(p.native.dropped)}; stderr ${p.native.capture})`,
    ...p.native.lines,
  );
  if (p.mpv) {
    const part = p.mpv.shown < p.mpv.total ? `last ${kib(p.mpv.shown)} of ${kib(p.mpv.total)}` : kib(p.mpv.total);
    out.push('', `## mpv log (${part}, ${p.mpv.source})`, p.mpv.tail.trimEnd());
  } else {
    out.push('', '## mpv log', p.mpvNote ?? '(none)');
  }
  return out.join('\n');
}

/// What was taken out, in one line at the very end — appended after the
/// redaction, which it describes.
export function redactedLine(counts: Record<string, number>): string {
  const parts = Object.entries(counts)
    .sort((a, b) => b[1] - a[1])
    .map(([kind, n]) => `${kind} ×${n}`);
  return `\n## Replaced\n${parts.length ? parts.join(', ') : 'nothing'}\n`;
}
