/**
 * The upscaler: a neural-network shader that mpv runs over the picture when it
 * is drawn larger than it was decoded, chosen from the context menu's picture
 * panel or stepped through by an unbound hotkey. Three of them: one that
 * reconstructs filmed video, one for animation, and a generative one that
 * draws detail in and is only offered for sources up to 720p (upscale-plan.ts
 * says what each costs and why).
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
import { untrack } from 'svelte';
import { SvelteSet } from 'svelte/reactivity';
import { command, getProperty } from 'tauri-plugin-libmpv-api';

import { upscaleScopes, writeUpscale, type UpscaleScope } from './history.svelte';
import { locale, t } from './i18n.svelte';
import { showOsd } from './osd.svelte';
import { player, userConfOptionNames } from './player.svelte';
import {
  fileRecord,
  nextUpscale,
  resolveUpscale,
  shaderFor,
  UPSCALE_SHADERS,
  upscaleEngaged,
  upscaleRatio,
  type ShaderMode,
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

  /// The shader the mode puts in for this source: `strong` is `video` above
  /// 720p (`STRONG_MAX`).
  get shader(): ShaderMode | null {
    return shaderFor(this.mode, player.decoded);
  }

  /// That shader is actually running at this size.
  get engaged(): boolean {
    return upscaleEngaged(this.shader, this.ratio);
  }

  /// mpv is compiling the shader and the picture stands still: since when, in
  /// `Date.now()` terms, or 0. See `initUpscaleWatch`.
  preparingSince = $state(0);

  /// The shader whose first run this session may still be ahead, and so may
  /// still cost a compile; null once it has run, or with nothing to run. A
  /// derived value rather than a getter so that the watch restarts when this
  /// changes, not at every step of a resize that moves `ratio`.
  pending = $derived.by((): ShaderMode | null => {
    const shader = this.given;
    if (!shader || !player.hasFile || this.userShaders) return null;
    return ran.has(shader) ? null : shader;
  });

  /// The shader in mpv's list, as of the last change to it that mpv took —
  /// which is not always `shader`: the list outlives `loadfile`, and the next
  /// file's own is sent only once it has loaded, after its first frame may
  /// already have compiled the previous file's shader.
  given = $state<ShaderMode | null>(null);
}

/// The shaders that have run in this session. libplacebo keeps a compiled pass
/// for as long as the video output lives, so a shader that has run once does
/// not compile again until the player is restarted. Written by the watch.
const ran = new SvelteSet<ShaderMode>();

export const upscale = new Upscale();

/// Where the shaders are, once `initUpscale` has been told.
let shaderDir: string | null = null;
/// The shader mpv has been given (null: none), so a file change that keeps it
/// sends nothing. Undefined when unknown: before the first send, and after one
/// that failed.
let sent: ShaderMode | null | undefined;

/// Called once mpv is up: the shader directory (`resource_dir/shaders`), and
/// whether the viewer's own configuration already owns the option.
export function initUpscale(dir: string) {
  shaderDir = dir;
  sent = undefined;
  upscale.userShaders = userConfOptionNames().some((n) => n.startsWith('glsl-shader'));
  // A file that finished loading before the directory was known.
  if (player.hasFile) void apply();
}

/**
 * Give mpv the shader list the mode calls for on this source. `clr` then
 * `append` rather than one `set`, because `set` splits its value on the
 * platform's path separator and `append` takes one path whole, whatever it
 * contains.
 */
async function apply() {
  const shader = upscale.shader;
  if (upscale.userShaders || !shaderDir || sent === shader) return;
  sent = shader;
  try {
    await command('change-list', ['glsl-shaders', 'clr', '']);
    if (shader) {
      const path = await join(shaderDir, UPSCALE_SHADERS[shader].file);
      await command('change-list', ['glsl-shaders', 'append', path]);
    }
    upscale.given = shader;
  } catch (e) {
    sent = undefined;
    console.warn('upscaler: mpv refused the shader list:', e);
  }
}

/// How often the watch looks. Every look asks mpv a question and reads the
/// position.
const WATCH_EVERY_MS = 250;
/// How long a question may go unanswered before the picture is said to be
/// waiting. Above the Mac's whole compile (~0.27 s) and above anything a warm
/// cache costs, so the plate appears only where there is a wait to explain.
const STALL_MS = 500;
/// How long the position may stand still in playback before the same is said.
const FROZEN_MS = 700;
/// How long the shader has to have been engaged before the picture moving
/// again means it has run. `engaged` is read off the mirrors, and they get
/// there first: the size that starts a compile is reported before the frame
/// that compiles, and a file opening into a large window is engaged before its
/// first frame.
const SETTLED_MS = 1000;
/// Position updates in the last second that make a picture moving: at 24 fps
/// mpv reports a couple of dozen, and one arrives now and then from inside a
/// compile.
const FLOWING_MOVES = 3;

/**
 * Say so while mpv compiles the shader. Under D3D11 the first run of ArtCNN is
 * 12 s of FXC on the render thread — the picture stands still, stretched to
 * whatever the window has become — and a compiled shader is kept on disk
 * (`gpu-shader-cache-dir`), so this is once on a machine, and it is the one
 * moment in the feature that looks like a hang.
 *
 * Nothing announces a compile, and it shows in two ways, both measured. Started
 * by a resize or by a redraw while paused, it holds **mpv's core**: a
 * `getProperty` sent as it began came back 12.4 s later, so a question left
 * unanswered for `STALL_MS` is the sign. Started by the first frame of a file
 * opening into a large window, the core goes on answering and only **the
 * position stands still**, so in playback a position frozen for `FROZEN_MS`
 * while the shader is engaged is the other. A paused file opening into a
 * compile shows neither, and goes unexplained.
 *
 * It runs only while mpv holds a shader that has not run yet this session
 * (`pending`): the switch off — the default — costs nothing, and neither does
 * a shader that has already run.
 */
function watchRenderer(shader: ShaderMode): () => void {
  let stopped = false;
  let next: ReturnType<typeof setTimeout> | undefined;
  /// When the mirrors first said the shader is engaged, 0 while they do not.
  let engagedAt = 0;
  let pos = player.timePos;
  /// When the position changed, the latest last.
  let moves: number[] = [Date.now()];
  const running = () => upscaleEngaged(shader, upscale.ratio);
  const playing = () => !player.paused && !player.stalled && !player.eofReached;

  const look = async () => {
    const now = Date.now();
    if (player.timePos !== pos) {
      pos = player.timePos;
      moves.push(now);
    }
    moves = moves.filter((t) => now - t < 1000 || t === moves[moves.length - 1]);
    const flowing = moves.filter((t) => now - t < 1000).length >= FLOWING_MOVES;
    if (!running()) engagedAt = 0;
    else if (!engagedAt) engagedAt = now;

    // In playback, up for a frozen position and down only once it flows again:
    // a single report from inside the compile must not drop the plate.
    if (engagedAt && playing()) {
      const still = now - moves[moves.length - 1] >= FROZEN_MS;
      if (!upscale.preparingSince && still) upscale.preparingSince = moves[moves.length - 1];
      else if (upscale.preparingSince && flowing) upscale.preparingSince = 0;
    } else {
      upscale.preparingSince = 0;
    }

    const stall = setTimeout(() => {
      if (!stopped && !upscale.preparingSince) upscale.preparingSince = now;
    }, STALL_MS);
    try {
      await getProperty('pause', 'flag');
    } catch {
      // An answer all the same: mpv is there to refuse.
    }
    clearTimeout(stall);
    if (stopped) return;
    // Answered, so a plate up for an unanswered question comes down; one up for
    // a frozen position waits for it to flow.
    if (!(engagedAt && playing())) upscale.preparingSince = 0;

    // Engaged for a while, answering, and in playback moving: it has run, and
    // will not compile again this session. This ends the watch.
    const settled = engagedAt && now - engagedAt >= SETTLED_MS && running();
    if (settled && !upscale.preparingSince && (!playing() || flowing)) {
      ran.add(shader);
      return;
    }
    next = setTimeout(() => void look(), WATCH_EVERY_MS);
  };
  void look();
  return () => {
    stopped = true;
    clearTimeout(next);
    upscale.preparingSince = 0;
  };
}

/// The standing effect behind `preparingSince`, started by the page.
export function initUpscaleWatch() {
  $effect(() => {
    const shader = upscale.pending;
    // Untracked: the first question is asked synchronously and reads
    // `engaged`, which would otherwise restart the watch at every change of it.
    if (shader) return untrack(() => watchRenderer(shader));
  });
}

/// The decoder reported the frame's size, which decides what `strong` puts in.
/// From the page's `property` hook, on `video-params/w` and `/h`.
export function upscaleSourceChanged() {
  if (upscale.mode === 'strong' && player.hasFile) void apply();
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
  void apply();
}

export function setUpscale(mode: UpscaleMode) {
  const path = player.filePath;
  if (!player.hasFile || !path || upscale.userShaders) return;
  upscale.mode = mode;
  void apply();
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

/// Said beside the status when `strong` is standing in as `video`, null
/// otherwise.
export function upscaleNote(): string | null {
  return upscale.mode === 'strong' && upscale.shader === 'video' ? t('upscale.strong_fallback') : null;
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
