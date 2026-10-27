/**
 * The two labeled filters the player owns — volume leveling (`@fpnorm`) and
 * the macOS SDR colour mode (`@fpsdr`) — and the record of which options the
 * viewer's own mpv.conf set, which anything the player would set steps aside
 * for.
 */

import { command } from 'tauri-plugin-libmpv-api';

import { IS_MAC } from '../platform';
import { player, type SdrColor } from './state.svelte';

// ---- Volume normalization -------------------------------------------------

// The stored key keeps its British spelling on purpose, alone in this file.
// It is not a word here, it is an address: renaming it would leave every
// existing installation's volume-leveling setting behind under a name nothing
// reads any more — a silent reset, on upgrade, for a preference the viewer set
// once and would not think to check.
const NORMALIZE_KEY = 'frameplayer.normalise';

/**
 * Night mode: even out quiet dialogue against loud effects.
 *
 * A LABELED filter, so it can be removed again without touching an `af` line
 * the user put in their own mpv.conf — which is also why the setting is kept in
 * localStorage rather than written to mpv.conf like the rest of the playback
 * settings: `mpv_conf_set` replaces the last uncommented line of a key, and for
 * `af` that would silently eat someone's own filter chain.
 *
 * The state cannot be read back from mpv either. `af` answers with its own
 * escaping (measured: `@fpnorm:lavfi=graph=%27%dynaudnorm=f=250:g=15:p=0.9`),
 * so ours is the record — the same arrangement as the repeat mode.
 *
 * Parameters are dynaudnorm's frame length, gaussian window and target peak:
 * gentle enough that music does not visibly pump, strong enough to matter.
 */
const NORMALIZE_FILTER = '@fpnorm:lavfi=[dynaudnorm=f=250:g=15:p=0.9]';

export function loadNormalize(): boolean {
  return localStorage.getItem(NORMALIZE_KEY) === 'on';
}

export function applyNormalize(on: boolean) {
  player.normalize = on;
  // `af remove` on a filter that is not there is harmless, and it is what makes
  // this idempotent enough to call at startup.
  void command('af', [on ? 'add' : 'remove', on ? NORMALIZE_FILTER : '@fpnorm']).catch(() => {});
  try {
    localStorage.setItem(NORMALIZE_KEY, on ? 'on' : 'off');
  } catch {
    // not critical: the choice simply will not survive a restart
  }
}

// ---- SDR color (macOS) ----------------------------------------------------

const SDR_COLOR_KEY = 'frameplayer.sdrcolor';

/**
 * "As in the system": draw BT.709 the way QuickTime does. Measured, and the
 * measurement is in docs/sdr-color.md — the short form is that AVFoundation
 * decodes a BT.709 picture with a pure power of ~1.961 and encodes it for the
 * display with the sRGB curve, where mpv hands the display the BT.1886 values
 * untouched. Same file, same window: QuickTime's mid-grey is 140 of 255 and
 * ours 129, and the difference is the whole of the "mpv is darker" complaint.
 *
 * The four values are the one combination that matched, to about one level
 * across the grey scale and to the unit on the six primaries:
 *
 * - `format=gamma=gamma2.2` re-labels the source as a pure power, so that
 * - `gamma-factor` 2.2/1.961 turns it into the 1.961 AVFoundation uses;
 * - `target-trc=srgb` encodes the result with the curve the display expects,
 * - which mpv 0.41 quietly treats as a pure 2.2 unless
 *   `treat-srgb-as-power22=no` — without it the shadows came out lifted by up
 *   to eight levels.
 *
 * **No ICC profile**, although the recipe this started from (IINA's) has one.
 * Our layer already goes through ColorSync as sRGB, so mpv converting to the
 * display profile as well colour-manages the picture twice: the tone curve
 * came out right and every primary desaturated (BT.709 red is 232,51,35 in
 * QuickTime and 213,68,49 with `icc-profile-auto`).
 *
 * The filter carries a label for the same reason `@fpnorm` does — it can be
 * removed again without touching a `vf` line from the user's mpv.conf — and a
 * key the user set there themselves is left alone in both directions.
 */
const SDR_COLOR_FILTER = '@fpsdr:format=gamma=gamma2.2';
const SDR_COLOR_OPTIONS: Record<string, [on: string, off: string]> = {
  'gamma-factor': ['1.1218765935747068', '1'],
  'target-trc': ['srgb', 'auto'],
  'treat-srgb-as-power22': ['no', 'auto'],
};

/// Keys that came from the user's mpv.conf and were accepted. Empty when mpv
/// refused the file and started without it — then nothing of theirs is in force.
let userConfKeys = new Set<string>();

/// Whether the viewer's own mpv.conf sets this option. Anything the player
/// would set on its own initiative steps aside for it.
export function isUserConfKey(name: string): boolean {
  return userConfKeys.has(name);
}

/// The option names the viewer's mpv.conf set, for the issue report — names
/// only: a value may be a path.
export function userConfOptionNames(): string[] {
  return [...userConfKeys].sort();
}

/// Set by `initPlayer` once mpv has accepted the viewer's mpv.conf — the one
/// write from outside this module, and an imported binding cannot be assigned.
export function setUserConfKeys(keys: Set<string>) {
  userConfKeys = keys;
}

/// What is in force in mpv right now. Ours, for the reason `normalize` is:
/// `vf` answers in its own form, and the three options may be the user's.
let sdrColorActive = false;
/// The last (mode, transfer) pair decided on. The 1 s sweep re-reports the
/// transfer, and without this every tick would re-send four commands.
let sdrColorDecided = '';

export function loadSdrColor(): SdrColor {
  try {
    return localStorage.getItem(SDR_COLOR_KEY) === 'system' ? 'system' : 'mpv';
  } catch {
    return 'mpv';
  }
}

/**
 * Switch the SDR color mode. Takes effect on the picture at once — every part
 * of it is a runtime property, paused frames included (measured) — and on
 * every file after it.
 */
export function applySdrColor(mode: SdrColor) {
  player.sdrColor = mode;
  try {
    localStorage.setItem(SDR_COLOR_KEY, mode);
  } catch {
    // not critical: the choice simply will not survive a restart
  }
  syncSdrColor();
}

/**
 * Put mpv in the state the mode and the current file call for.
 *
 * Only a picture mpv reads as `bt.1886` is touched: every SDR video transfer
 * (BT.709, BT.601, SDR BT.2020) is that, and so is mpv's guess for a video
 * that declares nothing — the case that started this. An RGB image is guessed
 * `srgb` and left alone, as QuickTime leaves it. PQ and HLG are never touched:
 * the filter would re-label an HDR signal as SDR, and the tone mapping has its
 * own settings. Measured on the way out of an SDR file into an HDR10 one: the
 * options are back 214 ms after `loadfile`, the first HDR frame is presented
 * at 233 ms.
 *
 * With no video (between files, audio only) whatever is in force stays, so
 * going from one episode to the next does not flash the other curve.
 */
export function syncSdrColor() {
  if (!IS_MAC || !player.ready) return;
  const mode = player.sdrColor;
  const trc = player.sourceTransfer;
  if (mode === 'system' && trc === null) return;
  const key = `${mode}|${trc}`;
  if (key === sdrColorDecided) return;
  sdrColorDecided = key;
  const want = mode === 'system' && trc === 'bt.1886';
  if (want === sdrColorActive) return;
  sdrColorActive = want;
  for (const [name, [on, off]] of Object.entries(SDR_COLOR_OPTIONS)) {
    if (userConfKeys.has(name)) continue;
    void command('set', [name, want ? on : off]).catch(() => {});
  }
  // `vf remove` on a filter that is not there is harmless.
  void command('vf', [want ? 'add' : 'remove', want ? SDR_COLOR_FILTER : '@fpsdr']).catch(() => {});
}
