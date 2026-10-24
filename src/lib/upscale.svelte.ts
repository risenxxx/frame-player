/**
 * The upscaler: a neural-network shader that mpv runs over the picture when it
 * is drawn larger than it was decoded, chosen from the context menu's picture
 * panel or stepped through by an unbound hotkey.
 *
 * **Two scopes, and the narrower wins.** A mode is kept for this file or for
 * its playlist (the folder, or the torrent — the key the picture adjustments
 * and the folder track memory use), and on `file-loaded` the file's own record
 * beats the playlist's. There is deliberately no third, global scope: whether a
 * picture is worth enlarging is a property of the source, a season shares one,
 * and an always-on network would be paid for by every 4K film, on battery,
 * for nothing. With nothing stored it is off, and the panel speaks about this
 * file. **Moving to the playlist overwrites what the playlist held** rather
 * than asking, as the picture adjustments do: there the two sets of values are
 * both somebody's tuning, here the viewer has just said which mode the season
 * gets. Moving back to "this file" pins the mode on screen to this file and
 * leaves the playlist's record for the other episodes.
 *
 * **`glsl-shaders` is an option, so it leaks across `loadfile`** like the
 * equalizer: every file gets the list it should have, an empty one included —
 * but only sent when it changes, because changing it makes libplacebo rebuild
 * the pass list. A list from the viewer's own mpv.conf is theirs, and the
 * switch steps aside for it (`userShaders`) rather than replacing it.
 *
 * **Whether anything runs is the shader's decision**, made per frame from the
 * size on screen — see upscale-plan.ts. `engaged` repeats it here so the panel
 * can say "not needed at this size" instead of looking broken.
 */

import { join } from '@tauri-apps/api/path';
import { command } from 'tauri-plugin-libmpv-api';

import { upscaleScopes, writeUpscale, type UpscaleScope } from './history.svelte';
import { locale, t } from './i18n.svelte';
import { showOsd } from './osd.svelte';
import { player, userConfOptionNames } from './player.svelte';
import {
  fileRecord,
  nextUpscale,
  resolveUpscale,
  UPSCALE_SHADERS,
  upscaleEngaged,
  upscaleRatio,
  type UpscaleMode,
} from './upscale-plan';

export type { UpscaleMode, UpscaleScope };

class Upscale {
  /// The mode in force on the file playing now.
  mode = $state<UpscaleMode>('off');
  /// Where `mode` came from, and where a change in the panel goes.
  scope = $state<UpscaleScope>('file');
  /// A link has no playlist scope to offer.
  hasFolder = $state(true);
  /// The viewer's mpv.conf sets shaders of its own; the switch stays out of
  /// their way.
  userShaders = $state(false);

  /// How far the picture is enlarged on screen, as the shaders measure it.
  get ratio(): { x: number; y: number } | null {
    return upscaleRatio(player.osd, player.decoded, player.voRotate);
  }

  /// The mode's shader is actually running at this size.
  get engaged(): boolean {
    return upscaleEngaged(this.mode, this.ratio);
  }
}

export const upscale = new Upscale();

/// Where the shaders are, once `initUpscale` has been told.
let shaderDir: string | null = null;
/// What mpv has been given, so a file change with the same mode sends nothing.
/// Null when unknown: before the first send, and after one that failed.
let sent: UpscaleMode | null = null;

/// Called once mpv is up: the shader directory (`resource_dir/shaders`), and
/// whether the viewer's own configuration already owns the option.
export function initUpscale(dir: string) {
  shaderDir = dir;
  sent = null;
  upscale.userShaders = userConfOptionNames().some((n) => n.startsWith('glsl-shader'));
  // A file that finished loading before the directory was known.
  if (player.hasFile) void send(upscale.mode);
}

/**
 * Give mpv the shader list for `mode`. `clr` then `append` rather than one
 * `set`, because `set` splits its value on the platform's path separator and
 * `append` takes one path whole, whatever it contains.
 */
async function send(mode: UpscaleMode) {
  if (upscale.userShaders || !shaderDir || sent === mode) return;
  sent = mode;
  try {
    await command('change-list', ['glsl-shaders', 'clr', '']);
    if (mode !== 'off') {
      const path = await join(shaderDir, UPSCALE_SHADERS[mode].file);
      await command('change-list', ['glsl-shaders', 'append', path]);
    }
  } catch (e) {
    sent = null;
    console.warn('upscaler: mpv refused the shader list:', e);
  }
}

/// Write the mode on screen to the scope on screen.
function save(path: string) {
  if (upscale.scope === 'folder') {
    writeUpscale(path, 'file', null);
    writeUpscale(path, 'folder', upscale.mode === 'off' ? null : upscale.mode);
  } else {
    writeUpscale(path, 'file', fileRecord(upscale.mode, upscaleScopes(path).folder));
  }
}

/// Put this file's mode in force: its own, its playlist's, or off. Runs on
/// `file-loaded`.
export function restoreUpscale() {
  const path = player.filePath;
  if (!path) return;
  const s = upscaleScopes(path);
  const r = resolveUpscale(s.file, s.folder);
  upscale.hasFolder = s.hasFolder;
  upscale.mode = r.mode;
  upscale.scope = r.scope;
  void send(r.mode);
}

export function setUpscale(mode: UpscaleMode) {
  const path = player.filePath;
  if (!player.hasFile || !path || upscale.userShaders) return;
  upscale.mode = mode;
  void send(mode);
  save(path);
}

/// Move the mode on screen to another scope.
export function setUpscaleScope(scope: UpscaleScope) {
  const path = player.filePath;
  if (!path || scope === upscale.scope) return;
  if (scope === 'folder' && !upscale.hasFolder) return;
  upscale.scope = scope;
  save(path);
}

export function upscaleModeLabel(mode: UpscaleMode): string {
  return t(`upscale.${mode}`);
}

/// "×1,6" in the interface's own way of writing a decimal.
function factor(ratio: { x: number; y: number }): string {
  return Math.min(ratio.x, ratio.y).toLocaleString(locale(), { maximumFractionDigits: 1 });
}

/**
 * What the panel says under a mode that is on: running and by how much, or
 * why not — the window is too small for it to matter (a larger one would
 * engage it), or the video is already as large as the surface it is drawn on.
 */
export function upscaleStatus(fullscreen: boolean): string {
  const ratio = upscale.ratio;
  if (upscale.engaged && ratio) return t('upscale.engaged', { ratio: factor(ratio) });
  return t(fullscreen ? 'upscale.idle_screen' : 'upscale.idle_window');
}

/// The hotkey: the next mode, and a popup saying which and whether it runs.
export function cycleUpscale() {
  if (!player.hasFile) return;
  if (upscale.userShaders) {
    showOsd(t('upscale.title'), { sub: t('upscale.user_conf') });
    return;
  }
  setUpscale(nextUpscale(upscale.mode));
  const label = upscaleModeLabel(upscale.mode);
  const idle = upscale.mode !== 'off' && !upscale.engaged;
  showOsd(t('upscale.title'), { sub: idle ? t('upscale.osd_idle', { mode: label }) : label });
}
