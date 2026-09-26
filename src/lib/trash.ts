/**
 * Send the file being watched to the system trash, and move on.
 *
 * How people clear out recordings and downloads while they watch them: seen
 * enough, one key, next episode. Three rules shape it.
 *
 * **The trash, never a delete.** Everything past the key is recoverable from
 * the system's own Trash or Recycle Bin, and the player offers no undo of its
 * own — a second way back that only works while the player is open would be a
 * promise it cannot keep across a restart. `trash.rs` fails rather than
 * falling back to deleting, and a failure is said out loud.
 *
 * **A file on disk, nothing else.** A link, a torrent stream or anything else
 * mpv reaches through a scheme is refused with a message rather than ignored:
 * a key that silently does nothing reads as a key that is not bound.
 *
 * **mpv lets go first.** The file leaves the queue through `playlist-remove
 * current`, which stops it and starts the next entry — or leaves mpv idle,
 * which is the start screen — and only once `path` has moved off it is the
 * file moved. On macOS the order would not matter; on Windows an open handle
 * is what makes the Recycle Bin refuse, and mpv holds one for as long as the
 * file plays.
 */

import { invoke } from '@tauri-apps/api/core';
import { command } from 'tauri-plugin-libmpv-api';

import { cancelAdvance } from './endscreen.svelte';
import { baseName } from './format';
import { forgetRecent } from './history.svelte';
import { t } from './i18n.svelte';
import { showOsd } from './osd.svelte';
import { IS_MAC } from './platform';
import { player } from './player.svelte';
import { isLocalFile } from './source';

/// How long mpv gets to close the file before it is moved anyway. A local file
/// closes in milliseconds; this is a ceiling for a machine that is busy, not a
/// wait anybody sees.
const RELEASE_CEILING_MS = 3000;

/// A few attempts, because on Windows mpv's handle and the storyboard
/// decoder's can each outlive the `path` change by a moment.
const TRASH_ATTEMPTS = 5;
const TRASH_RETRY_MS = 200;

let busy = false;

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

export async function trashCurrentFile() {
  const path = player.filePath;
  if (!path) return;
  if (!isLocalFile(path)) {
    showOsd(t(IS_MAC ? 'trash.not_local_mac' : 'trash.not_local_win'));
    return;
  }
  if (busy) return;
  busy = true;
  // Said before the wait rather than after it: the file has to be closed
  // first, and a key with nothing on screen for half a second reads as a key
  // that did not work — which invites the second press.
  showOsd(t(IS_MAC ? 'trash.busy_mac' : 'trash.busy_win'), { sticky: true });
  try {
    // An end-screen countdown still running would fire into the *next* file
    // the removal is about to open, and skip it.
    cancelAdvance();
    await command('playlist-remove', ['current']);
    const started = performance.now();
    while (player.filePath === path && performance.now() - started < RELEASE_CEILING_MS) {
      await sleep(50);
    }

    let error: unknown = null;
    for (let i = 0; i < TRASH_ATTEMPTS; i++) {
      try {
        await invoke('trash_file', { path });
        error = null;
        break;
      } catch (e) {
        error = e;
        await sleep(TRASH_RETRY_MS);
      }
    }
    if (error !== null) throw error;

    // After the file has left mpv, so the position the page commits on the way
    // out is already written — and is what this takes back out.
    forgetRecent(path);
    showOsd(t(IS_MAC ? 'trash.done_mac' : 'trash.done_win'), { sub: baseName(path) });
  } catch (e) {
    console.warn('trash failed:', e);
    showOsd(t(IS_MAC ? 'trash.failed_mac' : 'trash.failed_win'), { sub: baseName(path) });
  } finally {
    busy = false;
  }
}
