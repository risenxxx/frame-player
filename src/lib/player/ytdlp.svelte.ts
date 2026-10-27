/** yt-dlp: where it is, whether it is ours, and telling mpv about it. */

import { invoke } from '@tauri-apps/api/core';
import { command } from 'tauri-plugin-libmpv-api';

/**
 * Where yt-dlp is, and whether it is ours.
 *
 * `managed` decides what the UI may offer: our own copy in the app data
 * directory can be updated in place (that is why it lives there), while one
 * from Homebrew or pip belongs to its package manager and `-U` there would
 * either refuse or fight it.
 */
class Ytdlp {
  path = $state<string | null>(null);
  managed = $state(false);
  version = $state<string | null>(null);

  get present(): boolean {
    return this.path !== null;
  }
}

export const ytdlp = new Ytdlp();

export async function refreshYtdlp() {
  const status = await invoke<{ path: string | null; managed: boolean; version: string | null }>(
    'ytdlp_status',
  ).catch(() => null);
  ytdlp.path = status?.path ?? null;
  ytdlp.managed = status?.managed ?? false;
  ytdlp.version = status?.version ?? null;
}

/// Tell mpv where it is. Set rather than passed at init when the binary arrives
/// later — installing it mid-session must not need a restart.
export function applyYtdlpPath() {
  if (!ytdlp.path) return;
  void command('set', ['script-opts', `ytdl_hook-ytdl_path=${ytdlp.path}`]).catch(() => {});
}

export function hasYtdlp(): boolean {
  return ytdlp.present;
}
