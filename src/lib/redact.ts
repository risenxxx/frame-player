/**
 * What a bug report may not carry, taken out of it.
 *
 * The issue report is assembled from logs nobody wrote with an audience in mind
 * — the Cast wire log is raw CASTV2 JSON, the DLNA one raw SOAP, mpv's names
 * every file it opens — and it ends up on a public issue tracker. So this runs
 * over the **finished text**, not over the fields that are known to be
 * sensitive: a new log line, a new source of lines, a library that starts
 * printing something tomorrow all pass through here on their way out, and none
 * of them can opt out by being added somewhere this file was never told about.
 *
 * Two kinds of knowledge, and both are needed:
 *
 * - **Shapes.** A path, a URL, an address, a MAC, a UUID, a long hex id, a
 *   token, `key=value` where the key names a secret, and the fields of the
 *   three protocols that carry a name (`friendlyName`, `fn=`, `<key>name`).
 * - **Literals.** A name is not a shape: in `[cast] unicast: Living Room at …`
 *   nothing around "Living Room" says where it begins. The caller passes what
 *   the player knows — the television names discovery has seen, the file and
 *   its title, the user, the machine — and every occurrence goes.
 *
 * What it keeps on purpose: an extension (the container is half of most
 * playback bugs), a port, loopback addresses, the hosts the player itself
 * talks to, and the shape of a LAN — every address becomes a stable alias
 * (`lan-1`, `lan-2`, …), so "the television at lan-2 fetched from lan-1" still
 * reads as a conversation between two machines.
 *
 * Pure and tested (`redact.test.ts`), because a mistake here is invisible from
 * inside the player and permanent once the issue is posted.
 */

export type LiteralKind = 'file' | 'title' | 'device' | 'user' | 'home' | 'host';

export interface Literal {
  kind: LiteralKind;
  value: string;
}

export interface RedactResult {
  text: string;
  /// How many replacements of each kind — printed at the end of the report, so
  /// a reader can see that something was taken out and roughly what.
  counts: Record<string, number>;
}

/// Hosts the player itself talks to. Their names say which service failed,
/// which is what a report is for, and say nothing about the viewer.
const KNOWN_HOSTS = [
  'frameplayer.app',
  'github.com',
  'githubusercontent.com',
  'opensubtitles.com',
  'opensubtitles.org',
  'themoviedb.org',
  'tmdb.org',
  'gstatic.com',
];
/// …and of those, the ones whose paths are ours too (a release, an endpoint).
const KNOWN_PATHS = ['frameplayer.app', 'github.com'];

/// Absolute paths that name the system rather than a person: keep them, a
/// failing framework or library is worth seeing.
const SYSTEM_PREFIXES = [
  '/System/',
  '/usr/',
  '/bin/',
  '/sbin/',
  '/dev/',
  '/Library/',
  '/opt/homebrew/',
  '/Applications/',
];
const WINDOWS_SYSTEM = /^[A-Za-z]:[\\/]+(?:Windows|Program Files(?: \(x86\))?)[\\/]/i;

/// Keys whose value is a secret wherever they appear: `key=value`,
/// `"key": "value"`, `key: value`.
const SECRET_KEYS =
  'api[_-]?key|apikey|x-api-key|access[_-]?token|refresh[_-]?token|token|auth|authorization|password|passwd|secret|session[_-]?id|transport[_-]?id|cookie|set-cookie|signature|sig|pk|psk|key|user(?:name)?|login|ssid|bssid|deviceid|device[_-]?id|serial(?:number|[_-]?number)?|mac(?:[_-]?address)?';
/// Keys whose value is a name a person gave something, or the name of what is
/// being watched.
const NAME_KEYS =
  'friendlyName|deviceName|displayName|name|fn|title|media-title|force-media-title|subtitle|seriesTitle|artist|album|album_artist|comment|description|show|synopsis|filename';

class Aliases {
  private maps = new Map<string, Map<string, string>>();

  get(kind: string, value: string): string {
    let map = this.maps.get(kind);
    if (!map) this.maps.set(kind, (map = new Map()));
    const key = value.toLowerCase();
    let alias = map.get(key);
    if (!alias) map.set(key, (alias = `${kind}-${map.size + 1}`));
    return alias;
  }
}

export function redact(text: string, literals: Literal[] = []): RedactResult {
  const counts: Record<string, number> = {};
  const count = (kind: string) => (counts[kind] = (counts[kind] ?? 0) + 1);
  const aliases = new Aliases();
  const ip = (addr: string) => ipAlias(addr, aliases, count);

  let out = text;
  // Order matters: a URL holds an address and a path, and a path holds a name.
  out = out.replace(/\bmagnet:\?[^\s"'<>]+/gi, () => (count('magnet'), '<magnet>'));
  // A URL stops at an escaped bracket too: DIDL-Lite travels inside SOAP as
  // `&lt;res&gt;http://…&lt;/res&gt;`.
  out = out.replace(/\b[a-z][a-z0-9+.-]{1,15}:\/\/(?:(?!&lt;|&gt;|&quot;|&#)[^\s"'<>`])+/gi, (m) => {
    const { body, tail } = splitTrailing(m);
    return url(body, ip, count) + tail;
  });
  out = replacePaths(out, count);
  out = replaceLiterals(out, literals, count);
  out = out.replace(/\beyJ[\w-]{8,}\.[\w-]{8,}\.[\w-]{8,}/g, () => (count('token'), '<token>'));
  out = out.replace(/\bBearer\s+[^\s"',;]+/gi, () => (count('secret'), 'Bearer <secret>'));
  out = out.replace(
    new RegExp(`(["']?\\b(?:${SECRET_KEYS})\\b["']?\\s*[:=]\\s*)(["']?)([^\\s"'&,;<>}\\]]+)`, 'gi'),
    (m, head: string, quote: string, value: string) => {
      // `key: <path>` already said what it was; a number is a count, not a secret.
      if (value.startsWith('<') || /^(?:\d{1,6}|true|false|null|none|-)$/i.test(value)) return m;
      count('secret');
      return `${head}${quote}<secret>`;
    },
  );
  // <serialNumber>X</serialNumber> — the element form of the same keys.
  out = out.replace(
    new RegExp(`(<|&lt;)((?:[\\w-]+:)?(?:${SECRET_KEYS}))(>|&gt;)([^<]*?)(<|&lt;)\\/\\2(>|&gt;)`, 'gi'),
    (m, o1, tag: string, c1, value: string, o2, c2) =>
      // A plist's `<key>` is the label of the next value, not a value.
      value.trim() && tag.toLowerCase() !== 'key'
        ? (count('secret'), `${o1}${tag}${c1}<secret>${o2}/${tag}${c2}`)
        : m,
  );
  out = replaceNames(out, count);
  // mpv names the sound output, and a Bluetooth one is named by its owner.
  out = out.replace(/(audio output device:\s*)([^\n\r]+?)(\s*\(\d+\))?$/gim, (m, head, _name, id = '') =>
    (count('device'), `${head}<device>${id}`),
  );
  out = out.replace(/[\w.+-]+@[\w-]+(?:\.[\w-]+)+/g, () => (count('email'), '<email>'));
  out = out.replace(
    /(?<![\w:-])[0-9A-Fa-f]{2}([:-])(?:[0-9A-Fa-f]{2}\1){4}[0-9A-Fa-f]{2}(?![\w:-])/g,
    () => (count('mac'), '<mac>'),
  );
  out = out.replace(
    /\b[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}\b/g,
    (m) => (count('uuid'), aliases.get('uuid', m)),
  );
  out = out.replace(/\b[0-9A-Fa-f]{16,}\b/g, (m) =>
    /[0-9]/.test(m) && /[A-Fa-f]/.test(m) ? (count('id'), '<id>') : m,
  );
  out = out.replace(
    /(?<![\w-])(?=[\w-]{24,}(?![\w-]))(?=[\w-]*\d)(?=[\w-]*[a-z])(?=[\w-]*[A-Z])[\w-]+/g,
    // A C constant is not a token: `VK_KHR_get_physical_device_properties2` is
    // half of what mpv says about the GPU, measured in a real report.
    (m) => (/^[A-Z][A-Z0-9]*(?:_[A-Za-z0-9]+)+$/.test(m) ? m : (count('token'), '<token>')),
  );
  out = out.replace(/(?<![\w.-])[A-Za-z0-9][\w-]*(?:\.[A-Za-z0-9][\w-]*)*\.local\b/g, () => (count('host'), '<host>.local'));
  // …but a four-part version is not an address (`Conformance version: 1.3.8.0`).
  out = out.replace(/(?<![\w.])(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})(?![\w]|\.\d)/g, (m, ...rest) => {
    const at = rest[rest.length - 2] as number;
    const src = rest[rest.length - 1] as string;
    return /version:?\s*$/i.test(src.slice(Math.max(0, at - 12), at)) ? m : ip(m);
  });
  out = out.replace(/(?<![\w:.%-])[0-9A-Fa-f]{0,4}(?::[0-9A-Fa-f]{0,4}){2,7}(?![\w:])/g, (m) =>
    isIpv6(m) ? ip(m) : m,
  );
  return { text: out, counts };
}

/// Trailing punctuation belongs to the sentence, not to the URL or path.
function splitTrailing(m: string): { body: string; tail: string } {
  const cut = /[.,;:!?)\]}]+$/.exec(m);
  return cut ? { body: m.slice(0, cut.index), tail: cut[0] } : { body: m, tail: '' };
}

function ipAlias(addr: string, aliases: Aliases, count: (k: string) => void): string {
  const kind = ipKind(addr);
  if (kind === 'keep') return addr;
  count(kind);
  return aliases.get(kind, addr.replace(/%.*$/, ''));
}

/// `keep` for addresses that say nothing about anyone: loopback, unspecified,
/// broadcast and multicast (the SSDP and mDNS groups are in every log line of
/// discovery). `lan` for private and link-local space, `ip` for the rest.
function ipKind(addr: string): 'keep' | 'lan' | 'ip' {
  if (addr.includes(':')) {
    const a = addr.toLowerCase().replace(/%.*$/, '');
    if (a === '::' || a === '::1' || a.startsWith('ff')) return 'keep';
    if (/^fe[89ab]/.test(a) || /^f[cd]/.test(a)) return 'lan';
    return 'ip';
  }
  const p = addr.split('.').map(Number);
  if (p.some((n) => n > 255)) return 'keep';
  const [a, b] = p;
  if (a === 127 || a === 0 || (a >= 224 && a <= 239) || addr === '255.255.255.255') return 'keep';
  if (a === 10 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168)) return 'lan';
  if ((a === 169 && b === 254) || (a === 100 && b >= 64 && b <= 127)) return 'lan';
  return 'ip';
}

function isIpv6(s: string): boolean {
  const groups = (part: string) => (part === '' ? [] : part.split(':'));
  const halves = s.split('::');
  if (halves.length > 2) return false;
  const all = halves.flatMap(groups);
  if (!all.every((g) => /^[0-9A-Fa-f]{1,4}$/.test(g))) return false;
  if (halves.length === 1) return all.length === 8;
  return all.length <= 7 && (s.length > 2 || s === '::');
}

/// A URL keeps its scheme, its port and — for the player's own services — its
/// host; an address becomes its alias; the path keeps an extension and, on a
/// server of ours (loopback or the LAN), its first short segment (`/t/`, `/c/`),
/// which says *which* server without the token or the name after it.
function url(m: string, ip: (a: string) => string, count: (k: string) => void): string {
  const at = /^([a-z][a-z0-9+.-]*):\/\/([^/?#]*)([^?#]*)(\?[^#]*)?(#.*)?$/i.exec(m);
  if (!at) return m;
  const [, scheme, authority, path, query] = at;
  count('url');
  const userinfo = authority.includes('@');
  const hostPort = authority.slice(authority.lastIndexOf('@') + 1);
  const hp = /^(\[[^\]]+\]|[^:]*)(:\d+)?$/.exec(hostPort);
  const rawHost = hp ? hp[1] : hostPort;
  const port = hp?.[2] ?? '';
  const bare = rawHost.replace(/^\[|\]$/g, '');
  const lower = bare.toLowerCase();

  let host: string;
  let ours = false;
  let known = false;
  if (lower === '' || lower === 'localhost') {
    host = rawHost;
    ours = true;
  } else if (/^[\d.]+$/.test(bare) || bare.includes(':')) {
    const alias = ip(bare);
    ours = alias === bare || alias.startsWith('lan-');
    host = rawHost.startsWith('[') && alias === bare ? rawHost : alias;
  } else if (KNOWN_HOSTS.some((h) => lower === h || lower.endsWith(`.${h}`))) {
    host = bare;
    known = KNOWN_PATHS.some((h) => lower === h || lower.endsWith(`.${h}`));
  } else if (lower.endsWith('.local')) {
    host = '<host>.local';
  } else {
    host = '<host>';
  }

  let outPath = '';
  if (known) {
    outPath = path;
  } else if (path && path !== '/') {
    const segments = path.split('/').filter(Boolean);
    const first = ours && segments.length > 1 && /^[a-z]{1,4}$/i.test(segments[0]) ? `/${segments[0]}` : '';
    outPath = `${first}/<path>${extensionOf(segments[segments.length - 1] ?? '')}`;
  } else {
    outPath = path;
  }
  const q = query && query.length > 1 ? '?<query>' : '';
  return `${scheme}://${userinfo ? '<user>@' : ''}${host}${port}${outPath}${q}`;
}

function extensionOf(name: string): string {
  let decoded = name;
  try {
    decoded = decodeURIComponent(name);
  } catch {
    // A stray `%` is not worth a failed report.
  }
  const ext = /\.([A-Za-z0-9]{1,5})$/.exec(decoded);
  return ext && ext.index > 0 ? `.${ext[1]}` : '';
}

/// Where a path may begin: the start of a line or after a delimiter — never in
/// the middle of a word, so `audio/aac` and `src/lib.rs:12` stay as they are.
/// Unix absolute, home-relative, a Windows drive (also JSON-escaped, `C:\\`)
/// and a UNC share.
const PATH_START = /(?<=^|[\s"'=(\[{,;>|])(?<!&lt;|&gt;|&quot;)(?:~?\/(?=[^\s/])|[A-Za-z]:(?:\\\\|\\|\/)|\\\\(?:\\\\)?(?=\w))/gm;
/// Characters a path cannot run through in a log line.
const PATH_STOP = /["'<>|\n\r\t]/;

/**
 * Paths, by scanning rather than by one expression, because the hard part is
 * where a path *ends*: a film lives in `My Movies/Film (2020)/Film.mkv`,
 * spaces and parentheses included. The end is the first extension followed by
 * the end of a token; failing that, the last token that still continues the
 * path (holds a separator). A path that ends in a folder whose last name has a
 * space keeps the word after the space — the price of not swallowing the rest
 * of every log line, and the reason the caller also passes the home directory
 * and the file as literals.
 */
function replacePaths(text: string, count: (k: string) => void): string {
  let out = '';
  let last = 0;
  PATH_START.lastIndex = 0;
  for (let m = PATH_START.exec(text); m; m = PATH_START.exec(text)) {
    const start = m.index;
    if (start < last) continue;
    const end = pathEnd(text, start);
    const path = text.slice(start, end);
    if (!/[\\/][^\\/\s]/.test(path.slice(1)) && !path.startsWith('~')) {
      // A lone `/word` is not a path anybody's name is in.
      continue;
    }
    out += text.slice(last, start);
    if (SYSTEM_PREFIXES.some((p) => path.startsWith(p)) || WINDOWS_SYSTEM.test(path)) {
      out += path;
    } else {
      count('path');
      const name = path.split(/[\\/]+/).pop() ?? '';
      out += `<path>${extensionOf(name)}`;
    }
    last = end;
    PATH_START.lastIndex = end;
  }
  return out + text.slice(last);
}

function pathEnd(text: string, start: number): number {
  let limit = start;
  while (limit < text.length && !PATH_STOP.test(text[limit])) limit++;
  const span = text.slice(start, limit);
  // Whole tokens, as far as the last one within reach that still holds a
  // separator: a folder called `Secret Film (2019)` is three tokens of one
  // path, and only the third says so (measured: mpv's `find_files` line).
  const parts = span.split(/(\s+)/);
  let end = parts[0].length;
  for (let i = 1, run = 0; i + 1 < parts.length && i < 24; i += 2) {
    run += parts[i].length + parts[i + 1].length;
    if (/[\\/]/.test(parts[i + 1])) {
      end += run;
      run = 0;
    }
  }
  // The last name may have spaces of its own (`Film Name (2020).mkv`): it runs
  // to the first extension that ends a token, a dozen tokens at most.
  const lastSep = Math.max(span.lastIndexOf('/', end - 1), span.lastIndexOf('\\', end - 1));
  const tail = span.slice(lastSep + 1).split(/(\s+)/).slice(0, 24).join('');
  const ext = /\.[A-Za-z0-9]{1,5}(?=$|[\s,;:)\]}]|\.(?:\s|$))/.exec(tail);
  if (ext) return start + lastSep + 1 + ext.index + ext[0].length;
  return start + end - splitTrailing(span.slice(0, end)).tail.length;
}

function escapeRe(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/// Every literal, longest first so a file's path goes before its name. Names
/// that are also ordinary words (a user called "media") are matched as whole
/// words; a path is matched with either separator and either case, as Windows
/// and a JSON-escaped log line would spell it.
function replaceLiterals(text: string, literals: Literal[], count: (k: string) => void): string {
  const expanded: Literal[] = [];
  for (const l of literals) {
    const value = l.value.trim();
    // A shorter name is part of every other word; a title is held to four,
    // since three-letter titles are ordinary English words in a log.
    if (value.length < (l.kind === 'title' ? 4 : 3)) continue;
    expanded.push({ kind: l.kind, value });
    if (l.kind === 'file' || l.kind === 'home') {
      const parts = value.split(/[\\/]+/).filter(Boolean);
      const name = parts.pop() ?? '';
      const stem = name.replace(/\.[A-Za-z0-9]{1,5}$/, '');
      const parent = parts.pop() ?? '';
      if (l.kind === 'home') {
        if (name.length >= 3) expanded.push({ kind: 'user', value: name });
        continue;
      }
      if (name.length >= 4) expanded.push({ kind: 'file', value: name });
      if (stem.length >= 4) expanded.push({ kind: 'file', value: stem });
      if (parent.length >= 4) expanded.push({ kind: 'file', value: parent });
    }
  }
  // One expression for all of them, longest first so a file's path wins over
  // its name: a queue of two hundred episodes is eight hundred literals, and
  // eight hundred passes over a report of several megabytes is a frozen window.
  const key = (v: string) => v.toLowerCase().replace(/(?:\\{1,2}|\/)+/g, '/');
  const kinds = new Map<string, LiteralKind>();
  for (const l of expanded) {
    if (!kinds.has(key(l.value))) kinds.set(key(l.value), l.kind);
  }
  if (!kinds.size) return text;
  const alternatives = [...kinds.keys()]
    .sort((a, b) => b.length - a.length)
    .map((v) => v.split('/').map(escapeRe).join('(?:\\\\{1,2}|/)'));
  const re = new RegExp(`(?<![\\p{L}\\p{N}_<])(?:${alternatives.join('|')})(?![\\p{L}\\p{N}_>])`, 'giu');
  return text.replace(re, (m) => {
    const kind = kinds.get(key(m)) ?? 'file';
    count(kind);
    return kind === 'file' ? `<file>${extensionOf(m)}` : `<${kind}>`;
  });
}

/// The protocol fields that carry a name somebody gave something, or the name
/// of what is playing: XML elements (DLNA's description, DIDL-Lite's
/// `dc:title`), JSON members (Cast), plist pairs (AirPlay), TXT records and
/// mpv's own `key: value` dump of a file's tags.
function replaceNames(text: string, count: (k: string) => void): string {
  let out = text;
  const keys = NAME_KEYS;
  // <friendlyName>X</friendlyName>, <dc:title>X</dc:title>, entity-escaped too.
  out = out.replace(
    new RegExp(`(<|&lt;)((?:[\\w-]+:)?(?:${keys}))(>|&gt;)([^<]*?)(<|&lt;)\\/\\2(>|&gt;)`, 'gi'),
    (m, o1, tag, c1, value: string, o2, c2) => {
      if (!value.trim() || value.startsWith('<')) return m;
      count('name');
      return `${o1}${tag}${c1}<name>${o2}/${tag}${c2}`;
    },
  );
  // <key>name</key><string>X</string>
  out = out.replace(
    new RegExp(`(<key>(?:${keys})</key>\\s*<string>)([^<]*)(</string>)`, 'gi'),
    (m, a, value: string, b) => (value.trim() ? (count('name'), `${a}<name>${b}`) : m),
  );
  // "name": "X"  (JSON, also with escaped quotes inside the value)
  out = out.replace(
    new RegExp(`("(?:${keys})"\\s*:\\s*")((?:[^"\\\\]|\\\\.)*)(")`, 'gi'),
    (m, a, value: string, b) => (value.trim() ? (count('name'), `${a}<name>${b}`) : m),
  );
  // fn=X  name=X  title: X — to the end of the field.
  out = out.replace(
    // An apostrophe is part of a name ("Anna's Bedroom"), so it does not end one.
    // Not after another word: `Device Name: Apple M3 Max` is the GPU, and
    // `Driver name:` the driver — measured in mpv's log, and worth keeping.
    new RegExp(`(^|[\\s,;{(\\[])(?<![A-Za-z] )((?:${keys})\\s*[:=]\\s*)([^,;\\n\\r"}\\]]+)`, 'gim'),
    (m, lead: string, key: string, value: string) => {
      const v = value.trim();
      // Already handled, or a count / a flag rather than a name.
      if (!v || v.startsWith('<') || /^(?:\d+|true|false|null|none|yes|no|-)$/i.test(v)) return m;
      count('name');
      return `${lead}${key}<name>`;
    },
  );
  return out;
}
