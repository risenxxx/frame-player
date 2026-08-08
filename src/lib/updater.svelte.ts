/**
 * The signed auto-update, driven off the `latest.json` manifest on R2.
 *
 * **Nothing after `downloadAndInstall` runs on Windows** — the NSIS installer
 * kills the process inside that call, so `relaunch()` below is effectively dead
 * code there and anything that must survive the update has to be written first.
 * That is what the two `saveResumeSnapshot()` calls are: one before the
 * download starts, one when it finishes, because the download may have taken
 * minutes and the position has moved since.
 */

import { relaunch } from '@tauri-apps/plugin-process';
import { check, type Update } from '@tauri-apps/plugin-updater';

import { t } from './i18n.svelte';
import { dropResumeSnapshot, saveResumeSnapshot } from './history.svelte';
import { showOsd } from './osd.svelte';

class Updater {
  /// A release is waiting, or null. Shown as the button in the title bar.
  available = $state<Update | null>(null);
  /// Download progress while installing, or null when not installing.
  percent = $state<number | null>(null);
}

export const updater = new Updater();

/// Ask R2 whether there is a newer signed build.
export async function checkForUpdate() {
  updater.available = await check().catch(() => null);
}

export async function installUpdate() {
  if (!updater.available || updater.percent !== null) return;
  try {
    let total = 0;
    let done = 0;
    updater.percent = 0;
    saveResumeSnapshot();
    await updater.available.downloadAndInstall((e) => {
      if (e.event === 'Started') total = e.data.contentLength ?? 0;
      else if (e.event === 'Progress') {
        done += e.data.chunkLength;
        if (total > 0) updater.percent = Math.round((done / total) * 100);
      } else if (e.event === 'Finished') {
        updater.percent = 100;
        // the download may have taken minutes — refresh the position first
        saveResumeSnapshot();
      }
    });
    await relaunch();
  } catch (e) {
    // do not leave the snapshot behind, or a later ordinary launch would
    // suddenly open a video
    dropResumeSnapshot();
    updater.percent = null;
    showOsd(t('osd.update_failed'));
    console.warn('update failed:', e);
  }
}

/// Builds the list and pulls in posters. Entries whose file vanished drop out
/// silently: poster_frame returns an error, and keeping something unopenable
/// in the list is pointless.
///
/// The sequence number is bumped on every call: the list reloads on every
/// return to the start screen, and posters from the previous pass must not
/// append themselves to it.

// Custom seekbar (a native input[type=range] is unusable: the click is mapped
// accounting for the thumb width — diverging from the preview — and clicking
// the thumb itself is a dead zone). Click and preview use one formula: the
// fraction of the track width.
