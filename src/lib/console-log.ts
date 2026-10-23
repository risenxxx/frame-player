/**
 * The frontend's half of the issue report's log: `console.*` into a ring.
 *
 * The webview's console is invisible in an installed copy — there is no
 * inspector to open — and ~75 places write a warning or an error to it that
 * nobody ever reads. This keeps the last lines (and anything thrown that
 * nobody caught) so a report can carry them.
 *
 * A leaf on purpose: it is installed by the layout before anything else runs,
 * so it may import nothing that would start the rest of the app. The verbose
 * flag is pushed into it (`setVerboseConsole`) rather than read from the
 * report module, which sits far above.
 *
 * `error`, `warn` and `log` are always kept; `info` and `debug` only while the
 * verbose log is on — the same split as the native side, where the wire logs
 * are the verbose half.
 */

const NORMAL = { lines: 5_000, bytes: 1 << 20 };
const VERBOSE = { lines: 20_000, bytes: 4 << 20 };
/// One argument is cut here: an object logged whole (a track list, a status
/// record) is useful for its first screen, and a ring of them is not.
const ARG_MAX = 2_000;

let verbose = false;
let lines: string[] = [];
let bytes = 0;
let dropped = 0;
let installed = false;

export function setVerboseConsole(on: boolean) {
  verbose = on;
  trim();
}

/// The kept lines, oldest first, and how many fell off the front.
export function consoleLines(): { lines: string[]; dropped: number } {
  return { lines: [...lines], dropped };
}

function trim() {
  const caps = verbose ? VERBOSE : NORMAL;
  let drop = 0;
  while (lines.length - drop > caps.lines || (bytes > caps.bytes && lines.length - drop > 1)) {
    bytes -= lines[drop].length;
    drop++;
  }
  if (drop) {
    lines = lines.slice(drop);
    dropped += drop;
  }
}

/// `HH:MM:SS.mmm` in UTC, as the native ring stamps its lines.
function clock(): string {
  return new Date().toISOString().slice(11, 23);
}

function describe(arg: unknown): string {
  let text: string;
  if (typeof arg === 'string') text = arg;
  else if (arg instanceof Error) text = arg.stack ? `${arg.message}\n${arg.stack}` : `${arg.name}: ${arg.message}`;
  else {
    try {
      text = JSON.stringify(arg) ?? String(arg);
    } catch {
      text = String(arg);
    }
  }
  return text.length > ARG_MAX ? `${text.slice(0, ARG_MAX)} …[+${text.length - ARG_MAX}]` : text;
}

export function record(level: string, args: unknown[]) {
  const line = `${clock()} [${level}] ${args.map(describe).join(' ')}`;
  lines.push(line);
  bytes += line.length;
  trim();
}

/**
 * Wrap the console. Idempotent, because HMR re-runs the layout's script and a
 * second wrap would record every line twice.
 */
export function installConsoleLog() {
  if (installed || typeof console === 'undefined') return;
  installed = true;
  for (const level of ['error', 'warn', 'log', 'info', 'debug'] as const) {
    const original = console[level].bind(console);
    console[level] = (...args: unknown[]) => {
      if (verbose || (level !== 'info' && level !== 'debug')) record(level, args);
      original(...args);
    };
  }
  if (typeof window !== 'undefined') {
    // What nobody caught is exactly what a report is for, and it never reaches
    // `console.error` on its own in a production build.
    window.addEventListener('error', (e) => record('uncaught', [e.error ?? e.message]));
    window.addEventListener('unhandledrejection', (e) => record('unhandled', [e.reason]));
  }
}
