/**
 * Frame export.
 *
 * mpv writes the file itself, so the saved frame is exactly what the VO
 * decoded — including HDR tone mapping, which is what made the StepEngine
 * canvas path unusable for this. `video` is the clean frame (no OSD, no
 * subtitles); `subtitles` burns them in.
 */

import { invoke } from '@tauri-apps/api/core';
import { join } from '@tauri-apps/api/path';
import { command, getProperty } from 'tauri-plugin-libmpv-api';

import { baseName, fileStem, shotStamp } from './format';
import { t } from './i18n.svelte';
import { showOsd } from './osd.svelte';
import { player } from './player.svelte';

/**
 * Settings for the temp file behind clipboard copy. It exists for a fraction of
 * a second and reaches the clipboard as 8-bit RGBA regardless, so every byte of
 * compression work is thrown away — and 16-bit output (mpv's default for a
 * 10-bit source) doubles both the encode and the decode on the way back, for
 * precision the clipboard cannot carry.
 */
const CLIPBOARD_SHOT_OPTIONS: Record<string, string> = {
  'screenshot-png-compression': '0',
  'screenshot-png-filter': '0',
  'screenshot-high-bit-depth': 'no',
};

/**
 * Runs `write` with those settings and puts the previous ones back — including
 * after a failure, since leaving compression off would silently bloat every
 * frame the user saves afterwards.
 */
async function withFastScreenshot(write: () => Promise<unknown>) {
  const previous: [string, string][] = [];
  for (const [key, value] of Object.entries(CLIPBOARD_SHOT_OPTIONS)) {
    const was = await getProperty(key, 'string').catch(() => null);
    if (was !== null) previous.push([key, was]);
    await command('set', [key, value]).catch(() => {});
  }
  try {
    await write();
  } finally {
    for (const [key, value] of previous) {
      await command('set', [key, value]).catch(() => {});
    }
  }
}

export async function saveScreenshot(withSubtitles: boolean) {
  if (!player.hasFile) return;
  // Announced before the work, not after it. A 4K frame is a second of PNG
  // encoding on mpv's own thread, and until now that second was silent — the
  // menu closed and nothing happened, which reads as a click that missed.
  // Sticky, so it cannot fade while the encode is still running; every exit
  // from here replaces it.
  showOsd(t('osd.shot_saving'), { sticky: true });
  try {
    const dir = await invoke<string>('screenshot_dir');
    const name = `${fileStem(player.filename ?? '')}_${shotStamp(player.timePos)}.png`;
    await command('screenshot-to-file', [
      await join(dir, name),
      withSubtitles ? 'subtitles' : 'video',
    ]);
    // The folder is named in the message on purpose: a frame that silently
    // lands somewhere unnamed is a frame the user has to go hunting for. It
    // comes from the path Rust returned, so the two cannot drift apart.
    showOsd(t('osd.shot_saved'), { sub: `${baseName(dir)}/${name}` });
  } catch (e) {
    showOsd(t('osd.shot_failed'));
    console.warn('screenshot failed:', e);
  }
}

export async function copyScreenshot() {
  if (!player.hasFile) return;
  showOsd(t('osd.shot_copying'), { sticky: true });
  try {
    const path = await invoke<string>('screenshot_clip_path');
    await withFastScreenshot(() => command('screenshot-to-file', [path, 'video']));
    await invoke('screenshot_to_clipboard', { path });
    showOsd(t('osd.shot_copied'));
  } catch (e) {
    showOsd(t('osd.shot_failed'));
    console.warn('copy frame failed:', e);
  }
}
