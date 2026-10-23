/**
 * The issue report: everything a bug report needs, gathered by one button,
 * shown whole, and only then copied, saved or attached.
 *
 * **The ninth privacy enforcement point** (docs/rules/sources-and-privacy.md).
 * Every other one guards a path by which something about a file leaves the
 * machine; this one guards a text that is *made* to leave it, onto a public
 * tracker. Two rules make it safe, and both are structural rather than a
 * matter of care: the redaction (`redact.ts`) runs over the **assembled**
 * text, so no source of lines can reach the report around it; and nothing
 * leaves on its own — the dialog shows the very string that copy, save and
 * the issue link act on, and the issue link carries the version and the OS,
 * never the report.
 *
 * Also the verbose log's switch. It lives with the report because that is the
 * sequence it serves — turn it on, reproduce, collect — and it is not
 * remembered: the native flag is a process global, so a restart turns it off,
 * and the frontend asks for it again on start (`FP_CAST_DEBUG` may have turned
 * it on before the window existed).
 */

import { getVersion } from '@tauri-apps/api/app';
import { invoke } from '@tauri-apps/api/core';
import { getProperty } from 'tauri-plugin-libmpv-api';

import { output } from './audio-output.svelte';
import { cast, lastCastSession } from './cast.svelte';
import { consoleLines, setVerboseConsole } from './console-log';
import { extensionOf, formatTime } from './format';
import { locale, t } from './i18n.svelte';
import { REPO_URL } from './links';
import { isNetworkSource, player, userConfOptionNames } from './player.svelte';
import { playlist } from './playlist.svelte';
import { redact, type Literal, type LiteralKind } from './redact';
import { assemble, redactedLine, settingsLines, utc, type ReportParts } from './report-text';
import { isLocalFile, parseTorrentUrl } from './source';
import { displayName as roomName } from './sync/wire.svelte';

interface LogState {
  verbose: boolean;
  since: number | null;
  from_env: boolean;
}

interface Collected {
  native: string[];
  native_dropped: number;
  capture: string;
  mpv_log: { source: 'player' | 'mpv.conf'; tail: string; total: number; shown: number } | null;
  os: string;
  arch: string;
  translated: boolean;
  portable: boolean;
  installed: boolean;
  ffmpeg: string;
  secrets: Array<{ kind: LiteralKind; value: string }>;
  state: LogState;
}

class DebugLog {
  on = $state(false);
  /// When it was turned on, ms since the epoch.
  since = $state<number | null>(null);
  /// Turned on by `FP_CAST_DEBUG` rather than by the switch.
  fromEnv = $state(false);
}

export const debugLog = new DebugLog();

function applyState(s: LogState) {
  debugLog.on = s.verbose;
  debugLog.since = s.since;
  debugLog.fromEnv = s.from_env;
  setVerboseConsole(s.verbose);
}

/// Read the native flag once at start: the environment may have set it.
export async function syncDebugLog() {
  try {
    applyState(await invoke<LogState>('debug_log_state'));
  } catch (e) {
    console.warn('debug_log_state failed:', e);
  }
}

export async function setDebugLog(on: boolean) {
  try {
    applyState(await invoke<LogState>('debug_log_set', { on }));
    console.info(`[logs] verbose log ${on ? 'on' : 'off'}`);
  } catch (e) {
    console.warn('debug_log_set failed:', e);
  }
}

/// The report preferences worth seeing. Not the history stores — those are a
/// record of what was watched, and their size says nothing about a bug — nor
/// the per-device or per-file ones, which are keyed by names and paths.
const SETTINGS_STORES = [
  'locale',
  'window',
  'playlist',
  'seek',
  'thumbs',
  'frameStep',
  'loop',
  'normalise',
  'sdrcolor',
  'volume',
  'torrent',
  'castMode',
  'castCacheCap',
  'catalog',
  'tmdb',
  'indexer',
  'indexerApiKey',
  'relay',
  'handler',
  'history',
  'keys',
];

function storedSettings(): string[] {
  return settingsLines(
    SETTINGS_STORES.map((name) => {
      try {
        return [name, localStorage.getItem(`frameplayer.${name}`)] as [string, string | null];
      } catch {
        return [name, null] as [string, string | null];
      }
    }),
  );
}

async function prop(name: string): Promise<string | null> {
  try {
    const v = await getProperty(name, 'string');
    return typeof v === 'string' && v !== '' ? v : null;
  } catch {
    return null;
  }
}

/// What the source *is*, never which one: the redaction would take the path
/// out anyway, but the kind is the half a bug needs.
function sourceKind(path: string | null): string {
  if (!path) return 'nothing open';
  const ext = extensionOf(path.split(/[?#]/)[0]);
  const dotted = ext ? ` (.${ext})` : '';
  if (parseTorrentUrl(path)) return `torrent${dotted}`;
  if (isLocalFile(path)) return `local file${dotted}`;
  if (isNetworkSource(path)) return `network stream (${path.split(':')[0]})`;
  return 'other';
}

async function playbackRows(): Promise<Array<[string, string]>> {
  const rows: Array<[string, string]> = [['Source', sourceKind(player.filePath)]];
  if (!player.filePath) return rows;
  const [format, codec, pix, gamma, prim, fps, w, h, audio, hwdec] = await Promise.all([
    prop('file-format'),
    prop('video-format'),
    prop('video-params/pixelformat'),
    prop('video-params/gamma'),
    prop('video-params/primaries'),
    prop('container-fps'),
    prop('width'),
    prop('height'),
    prop('audio-codec-name'),
    prop('hwdec-current'),
  ]);
  if (format) rows.push(['Container', format]);
  if (codec) {
    const size = w && h ? ` ${w}x${h}` : '';
    const color = [pix, gamma, prim].filter(Boolean).join(', ');
    const rate = fps ? `, ${Number(fps).toFixed(3)} fps` : '';
    rows.push(['Video', `${codec}${size}${color ? ` (${color})` : ''}${rate}`]);
  }
  if (audio) rows.push(['Audio', audio]);
  rows.push(['Decoding', hwdec && hwdec !== 'no' ? `hardware (${hwdec})` : 'software']);
  rows.push([
    'Position',
    `${player.paused ? 'paused' : 'playing'} at ${formatTime(player.timePos)} of ${formatTime(player.duration)}` +
      `, speed ${player.speed}, queue ${player.playlistCount}`,
  ]);
  return rows;
}

function castingRows(): Array<[string, string]> | null {
  const r = lastCastSession();
  if (!r) return null;
  const rows: Array<[string, string]> = [
    ['Transport', r.transport === 'cast' ? 'Google Cast' : r.transport === 'dlna' ? 'DLNA' : 'AirPlay'],
    ['Model', r.model || 'not announced'],
  ];
  if (r.capabilities) rows.push(['Capabilities', r.capabilities]);
  if (r.container) rows.push(['Container', `${r.container}${r.verdict ? ` → ${r.verdict}` : ''}`]);
  rows.push(['Last state', `${r.state}${r.error ? ` (${r.error})` : ''}, ${r.fetches} fetches by the device`]);
  rows.push(['Started', utc(new Date(r.startedAt))]);
  rows.push(['Ended', r.endedAt ? `${utc(new Date(r.endedAt))}${r.ending ? ` — "${r.ending}"` : ''}` : 'still running']);
  return rows;
}

/// Everything the player knows that a report must not name. Names are not
/// shapes (see redact.ts), so they are collected here from wherever the player
/// holds them; a name that is also a model is left alone — the model is the
/// part the rules say to keep, and a Chromecast left under its factory name is
/// named after it.
function literals(collected: Collected): Literal[] {
  const out: Literal[] = [...collected.secrets];
  const add = (kind: LiteralKind, value: string | null | undefined) => {
    if (value) out.push({ kind, value });
  };
  add('file', player.filePath);
  add('title', player.mediaTitle);
  add('file', player.filename);
  for (const e of playlist.entries) {
    add('file', e.path);
    add('title', e.title);
  }
  add('device', cast.deviceName);
  for (const d of cast.devices) add('device', d.name);
  for (const d of cast.dlnaDevices) add('device', d.name);
  try {
    const profiles = JSON.parse(localStorage.getItem('frameplayer.tv-devices') ?? '{}') as Record<
      string,
      { name?: string }
    >;
    for (const p of Object.values(profiles)) add('device', p.name);
  } catch {
    // A broken store is a missing literal, and the shapes still apply.
  }
  add('user', roomName());
  // The sound output's name, as the system gives it — a Bluetooth device is
  // named by its owner — and every one with an offset remembered.
  add('device', output.name);
  try {
    const offsets = JSON.parse(localStorage.getItem('frameplayer.audioOffsets') ?? '{}') as Record<
      string,
      { name?: string }
    >;
    for (const o of Object.values(offsets)) add('device', o.name);
  } catch {
    // As above.
  }

  const models = [
    ...cast.devices.map((d) => d.model),
    ...cast.dlnaDevices.map((d) => d.model),
    lastCastSession()?.model ?? '',
  ]
    .filter(Boolean)
    .map((m) => m.toLowerCase());
  return out.filter(
    (l) => l.kind !== 'device' || !models.some((m) => m.includes(l.value.trim().toLowerCase())),
  );
}

export interface Report {
  text: string;
  version: string;
  os: string;
}

export async function buildReport(): Promise<Report> {
  const [collected, version, mpvVersion, ffmpegVersion, vo, ao, hwdecSetting] = await Promise.all([
    invoke<Collected>('diag_collect', { confRefused: player.confError !== null }),
    getVersion().catch(() => '?'),
    prop('mpv-version'),
    prop('ffmpeg-version'),
    prop('current-vo'),
    prop('current-ao'),
    prop('hwdec'),
  ]);
  applyState(collected.state);

  const os = `${collected.os}, ${collected.arch}${collected.translated ? ' (Rosetta)' : ''}`;
  const conf = userConfOptionNames();
  const since = debugLog.since ? ` since ${utc(new Date(debugLog.since))}` : '';
  const summary: Array<[string, string]> = [
    ['Version', version],
    ['OS', os],
    ['Copy', `${collected.installed ? 'installed' : 'not installed'}${collected.portable ? ', portable' : ''}`],
    ['mpv', mpvVersion ?? (player.ready ? '?' : `not started${player.initError ? ` (${player.initError})` : ''}`)],
    ['FFmpeg', `${ffmpegVersion ?? '?'} (mpv), ${collected.ffmpeg} (thumbnails)`],
    ['Video output', `${vo ?? '-'}, hwdec setting ${hwdecSetting ?? '-'}`],
    ['Audio output', ao ?? '-'],
    ['Interface', locale()],
    [
      'Verbose log',
      debugLog.on ? `on${since}${debugLog.fromEnv ? ' (FP_CAST_DEBUG)' : ''}` : 'off',
    ],
    ['mpv.conf', conf.length ? `sets ${conf.join(', ')}` : 'nothing'],
  ];
  if (player.confError) summary.push(['mpv.conf refused', player.confError]);

  const mpvLog = collected.mpv_log;
  const parts: ReportParts = {
    generatedAt: new Date(),
    summary,
    playback: await playbackRows(),
    casting: castingRows(),
    settings: storedSettings(),
    frontend: consoleLines(),
    native: { lines: collected.native, dropped: collected.native_dropped, capture: collected.capture },
    mpv: mpvLog && {
      ...mpvLog,
      source: mpvLog.source === 'mpv.conf' ? 'the file mpv.conf names' : "the player's",
    },
    mpvNote: '(no log file found)',
  };
  const { text, counts } = redact(assemble(parts), literals(collected));
  return { text: text + redactedLine(counts), version, os: collected.os };
}

/// A new issue on GitHub with a title and a short body. The report itself does
/// not fit in a URL — a few megabytes against a limit of a few kilobytes — and
/// a URL is the one thing here that is sent before anyone looks at it, so it
/// carries the version and the OS and asks for the saved file to be attached.
export function issueUrl(report: Report): string {
  const title = t('report.issue_title', { version: report.version });
  const body = t('report.issue_body', { version: report.version, os: report.os });
  return `${REPO_URL}/issues/new?title=${encodeURIComponent(title)}&body=${encodeURIComponent(body)}`;
}

export function reportFileName(now = new Date()): string {
  const stamp = now.toISOString().slice(0, 19).replace(/[-:]/g, '').replace('T', '-');
  return `frame-player-report-${stamp}.txt`;
}
