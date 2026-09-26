/**
 * Brightness, contrast, saturation and gamma: mpv's equalizer, set from the
 * context menu's picture panel, from the settings sheet (the global values
 * only) and from four pairs of unbound hotkeys.
 *
 * **Three scopes, and the narrowest one that has something wins.** A value is
 * kept for this file, for its playlist (the folder, or the torrent; the same
 * key the folder track memory uses) or for every file, and on `file-loaded`
 * the file's own beats the playlist's, which beats the global one. The panel's
 * scope says where the values on screen came from, and a change made there is
 * written back to the same place. With nothing stored anywhere it is "this
 * file": a film graded too dark is the common case, and a correction for it
 * must not follow the viewer into the next one.
 *
 * **Moving to a scope that already holds other values is a question, not an
 * overwrite.** The case that forced it: a file tuned on its own, then a
 * different global correction set in the settings, then "all files" picked in
 * the panel. Either the file's values become everybody's or the file gives its
 * own up for the global ones, and both are reasonable, so the panel asks
 * (`conflict`) instead of silently destroying one of them.
 *
 * **These are options, so they leak.** `gamma` set on one file is still 40 on
 * the next (measured on 0.41, as with `video-crop`), so every file gets an
 * explicit value for each of the four, zero included. A key the viewer set in
 * their own mpv.conf is left alone in both directions, like the SDR mode's.
 *
 * Measured on 0.41 with gpu-next: all four reach a saved frame
 * (`screenshot-to-file … video`), and `gamma` composes with the SDR mode's
 * `gamma-factor` rather than replacing it, so the two need no arbitration.
 */

import { setProperty } from 'tauri-plugin-libmpv-api';

import { adjustScopes, loadAdjustAll, writeAdjust, type Adjust, type AdjustScope } from './history.svelte';
import { t } from './i18n.svelte';
import { showOsd } from './osd.svelte';
import { isUserConfKey, player } from './player.svelte';

export type { Adjust, AdjustScope };

export const ADJUST_PARAMS = ['brightness', 'contrast', 'saturation', 'gamma'] as const;
export type AdjustParam = (typeof ADJUST_PARAMS)[number];

export const ADJUST_SCOPES: readonly AdjustScope[] = ['file', 'folder', 'all'];

/// One press of a hotkey. mpv's own bindings move by 1, which on a -100..100
/// scale takes a held key several seconds to show anything.
export const ADJUST_KEY_STEP = 2;

/// A slider fires on every pixel of a drag; only where it stops is worth
/// writing down.
const WRITE_MS = 400;

export const ZERO: Adjust = { brightness: 0, contrast: 0, saturation: 0, gamma: 0 };

export function isZero(a: Adjust | null): boolean {
  return !a || ADJUST_PARAMS.every((k) => !a[k]);
}

function same(a: Adjust | null, b: Adjust | null): boolean {
  return ADJUST_PARAMS.every((k) => (a?.[k] ?? 0) === (b?.[k] ?? 0));
}

class PictureAdjust {
  /// What is in force on the file playing now.
  values = $state<Adjust>({ ...ZERO });
  /// Where `values` came from, and where a change in the panel goes.
  scope = $state<AdjustScope>('file');
  /// Whether this file has a record of its own in `scope`. False means it is
  /// showing what came from nowhere (zeros) — the one case where a change of
  /// the global values reaches this file although the panel says "file".
  stored = $state(false);
  /// A link has no playlist scope to offer.
  hasFolder = $state(true);
  /// The values for every file, which the settings sheet edits whether or not
  /// anything is playing.
  global = $state<Adjust | null>(loadAdjustAll());
  /// A scope the panel was asked to move to that already holds different
  /// values, waiting for the viewer to say which set survives.
  conflict = $state<{ scope: AdjustScope; theirs: Adjust } | null>(null);

  get changed(): boolean {
    return !isZero(this.values);
  }

  /// The global values are what this file shows, so editing them in the
  /// settings has to repaint it.
  get followsGlobal(): boolean {
    return this.scope === 'all' || !this.stored;
  }
}

export const adjust = new PictureAdjust();

/// Writes still waiting for a drag to settle, by what they are for: a file
/// change flushes them, so a correction dialled in just before the next episode
/// starts is neither lost nor written against the wrong source.
let pending: { path: string; scope: AdjustScope; values: Adjust } | null = null;
let pendingGlobal: Adjust | null | undefined;
let writeTimer: ReturnType<typeof setTimeout> | undefined;

function send(values: Adjust) {
  for (const k of ADJUST_PARAMS) {
    if (isUserConfKey(k)) continue;
    void setProperty(k, values[k]).catch(() => {});
  }
}

/**
 * Store `values` in `scope`, or clear it when that changes nothing.
 *
 * All zeros is stored only when a broader scope would otherwise show through:
 * "this file at zero" in a playlist brightened by ten is a real choice, while
 * the same zeros with nothing beneath them are simply no record.
 */
function write(path: string, scope: AdjustScope, values: Adjust) {
  if (scope === 'all') {
    writeGlobal(values);
    return;
  }
  const s = adjustScopes(path);
  const beneath = scope === 'file' ? (s.folder ?? s.all) : s.all;
  writeAdjust(path, scope, isZero(values) && isZero(beneath) ? null : { ...values });
}

function writeGlobal(values: Adjust) {
  const next = isZero(values) ? null : { ...values };
  adjust.global = next;
  writeAdjust('', 'all', next);
}

function flush() {
  clearTimeout(writeTimer);
  if (pending) write(pending.path, pending.scope, pending.values);
  if (pendingGlobal !== undefined) writeGlobal(pendingGlobal ?? ZERO);
  pending = null;
  pendingGlobal = undefined;
}

function writeSoon() {
  const path = player.filePath;
  if (!path) return;
  if (adjust.scope === 'all') {
    pendingGlobal = { ...adjust.values };
    adjust.global = isZero(adjust.values) ? null : { ...adjust.values };
  } else {
    pending = { path, scope: adjust.scope, values: { ...adjust.values } };
    adjust.stored = true;
  }
  clearTimeout(writeTimer);
  writeTimer = setTimeout(flush, WRITE_MS);
}

/// Put this file's values in force: its own, its playlist's, everybody's, or
/// none. Runs on `file-loaded`.
export function restoreAdjust() {
  flush();
  adjust.conflict = null;
  const path = player.filePath;
  if (!path) return;
  const s = adjustScopes(path);
  adjust.hasFolder = s.hasFolder;
  adjust.global = s.all;
  const [scope, values, stored]: [AdjustScope, Adjust, boolean] = s.file
    ? ['file', s.file, true]
    : s.folder
      ? ['folder', s.folder, true]
      : s.all
        ? ['all', s.all, true]
        : ['file', ZERO, false];
  adjust.scope = scope;
  adjust.stored = stored;
  adjust.values = { ...ZERO, ...values };
  send(adjust.values);
}

function clamp(value: number): number {
  return Math.max(-100, Math.min(100, Math.round(value)));
}

export function setAdjust(k: AdjustParam, value: number) {
  if (!player.hasFile) return;
  const v = clamp(value);
  if (adjust.values[k] === v) return;
  adjust.conflict = null;
  adjust.values[k] = v;
  if (!isUserConfKey(k)) void setProperty(k, v).catch(() => {});
  writeSoon();
}

/**
 * Edit the values for every file, from the settings sheet.
 *
 * Works with nothing playing. When something is, it is repainted only if it
 * is showing the global values: a file or a playlist with values of its own
 * keeps them, which is exactly what "the narrowest scope wins" promises, and
 * the sheet says so under the sliders. A file showing nothing at all starts
 * following the global values from the first change, the same as the next file
 * opened would.
 */
export function setGlobalAdjust(k: AdjustParam, value: number) {
  const v = clamp(value);
  const next = { ...(adjust.global ?? ZERO), [k]: v };
  if (same(next, adjust.global)) return;
  adjust.global = isZero(next) ? null : next;
  pendingGlobal = adjust.global;
  if (player.hasFile && adjust.followsGlobal) {
    adjust.conflict = null;
    adjust.scope = 'all';
    adjust.stored = !isZero(next);
    adjust.values = { ...next };
    if (!isUserConfKey(k)) void setProperty(k, v).catch(() => {});
  }
  clearTimeout(writeTimer);
  writeTimer = setTimeout(flush, WRITE_MS);
}

export function resetGlobalAdjust() {
  for (const k of ADJUST_PARAMS) setGlobalAdjust(k, 0);
  flush();
}

export function adjustLabel(k: AdjustParam): string {
  return t(`adj.${k}`);
}

/// The inline style `input.bipolar` fills itself from.
export function bipolarFill(v: number): string {
  const pos = (v + 100) / 2;
  return `--lo: ${Math.min(50, pos)}%; --hi: ${Math.max(50, pos)}%`;
}

export function signed(v: number): string {
  return v > 0 ? `+${v}` : String(v);
}

/// "Яркость +10, Гамма −5": what a set of values changes, for the conflict
/// question and the settings hint.
export function describeAdjust(a: Adjust): string {
  return ADJUST_PARAMS.filter((k) => a[k])
    .map((k) => `${adjustLabel(k)} ${signed(a[k])}`)
    .join(', ');
}

/// A hotkey's step, with the popup that says where it landed.
export function nudgeAdjust(k: AdjustParam, delta: number) {
  if (!player.hasFile) return;
  setAdjust(k, adjust.values[k] + delta);
  const v = adjust.values[k];
  showOsd(adjustLabel(k), { sub: signed(v), progress: (v + 100) / 200 });
}

export function resetAdjust() {
  if (!player.hasFile) return;
  adjust.conflict = null;
  adjust.values = { ...ZERO };
  send(adjust.values);
  writeSoon();
  flush();
  showOsd(t('adj.reset_done'));
}

/**
 * Move the values in force to another scope.
 *
 * When the target already holds different values it does not move yet: it
 * raises `conflict`, and `resolveAdjustConflict` finishes the move either way.
 * Widening clears the narrower records for this source, or they would go on
 * overriding what was just made general: "every file" with this file's own
 * record still in place would change every file but this one.
 */
export function setAdjustScope(scope: AdjustScope) {
  const path = player.filePath;
  if (!path) return;
  if (scope === adjust.scope) {
    adjust.conflict = null;
    return;
  }
  if (scope === 'folder' && !adjust.hasFolder) return;
  flush();
  const s = adjustScopes(path);
  const theirs = s[scope];
  if (theirs && !same(theirs, adjust.values)) {
    adjust.conflict = { scope, theirs: { ...ZERO, ...theirs } };
    return;
  }
  moveTo(path, scope, adjust.values);
}

/// `mine`: the values on screen go to the target scope. `theirs`: this source
/// gives up its own and takes what the target holds.
export function resolveAdjustConflict(keep: 'mine' | 'theirs') {
  const path = player.filePath;
  const c = adjust.conflict;
  adjust.conflict = null;
  if (!path || !c) return;
  if (keep === 'theirs') {
    adjust.values = { ...c.theirs };
    send(adjust.values);
  }
  moveTo(path, c.scope, adjust.values);
}

function moveTo(path: string, scope: AdjustScope, values: Adjust) {
  adjust.conflict = null;
  adjust.scope = scope;
  if (scope !== 'file') writeAdjust(path, 'file', null);
  if (scope === 'all') writeAdjust(path, 'folder', null);
  write(path, scope, values);
  adjust.stored = !!adjustScopes(path)[scope];
}

/// Whatever was asked and not answered is dropped with the surface that asked
/// it: a question left standing would come back on the next right-click about
/// a choice the viewer has moved on from.
export function dismissAdjustConflict() {
  adjust.conflict = null;
}
