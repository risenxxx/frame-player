/**
 * The signed auto-update, driven off the `latest.json` manifest on R2.
 *
 * There are two ways it lands, and the difference is one platform's.
 *
 * On **macOS** the plugin does all of it: it downloads the `.app.tar.gz`,
 * verifies it and unpacks it over the application directory, and `relaunch()`
 * afterwards runs.
 *
 * On **Windows** there are now two paths of its own. The player asks its own
 * backend first (`update_prepare`), which downloads the installation as a zip
 * and stages what changed; `update_commit` then swaps the files and relaunches,
 * with no installer window and no uninstall pass. When that cannot be used —
 * a directory it may not write to, a release with no such payload, a file set
 * that changed — `update_prepare` says so without downloading anything in the
 * common cases, and the installer runs exactly as it always did. That path
 * still ends inside `downloadAndInstall`: the NSIS installer kills the process
 * from in there, and no code after it runs.
 *
 * Which is why the resume snapshot is written twice on every path: once before
 * the download starts, once when it finishes, because the download may have
 * taken minutes and the position has moved since. See update.rs for what the
 * swap actually does and why the file set may not change.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { openUrl } from '@tauri-apps/plugin-opener';
import { relaunch } from '@tauri-apps/plugin-process';
import { check, type Update } from '@tauri-apps/plugin-updater';

import { t } from './i18n.svelte';
import { dropResumeSnapshot, saveResumeSnapshot } from './history.svelte';
import { showOsd } from './osd.svelte';
import { IS_MAC } from './platform';

/** Whole percent of the in-place payload's download, from update.rs. */
const PROGRESS_EVENT = 'frameplayer://update-progress';

/** What `update_prepare` decided. `reason` is for the console, not the window. */
interface Plan {
  /// `inplace` — staged, swap it. `installer` — run the installer, as every
  /// release did before. `download` — a portable copy, for which running the
  /// installer would mean a second, ordinary copy in the profile and the
  /// portable one left behind; so the release page opens instead.
  mode: 'inplace' | 'installer' | 'download';
  version: string | null;
  files: number;
  reason: string | null;
}

/// Where a portable copy is sent when it cannot swap its own files. The tag
/// rather than the latest: it lands on the page that actually carries that
/// version's archive.
function releasePage(version: string | null): string {
  const base = 'https://github.com/risenxxx/frame-player/releases';
  return version ? `${base}/tag/v${version}` : `${base}/latest`;
}

/** What `update_check` announces. The plugin's own `Update` satisfies it too. */
export interface Waiting {
  version: string;
  body?: string | null;
}

class Updater {
  /// A release is waiting, or null. Shown as the button in the title bar.
  available = $state<Waiting | null>(null);
  /// Download progress while installing, or null when not installing.
  percent = $state<number | null>(null);
}

export const updater = new Updater();

/**
 * The plugin's own handle on the waiting release, kept for the paths that
 * install through it — macOS always, Windows when the swap is refused. Separate
 * from `updater.available`, which is only what the button shows: on Windows
 * that comes from our own backend, so that one place decides what is announced
 * and what gets installed.
 */
let pluginUpdate: Update | null = null;

/// Ask R2 whether there is a newer signed build.
export async function checkForUpdate() {
  if (IS_MAC) {
    pluginUpdate = await check().catch(() => null);
    updater.available = pluginUpdate;
    return;
  }
  updater.available = await invoke<Waiting | null>('update_check').catch((e) => {
    console.warn('update check failed:', e);
    return null;
  });
}

export async function installUpdate() {
  if (!updater.available || updater.percent !== null) return;
  try {
    updater.percent = 0;
    saveResumeSnapshot();
    if (!IS_MAC && (await swapInPlace())) return;
    // The installer runs through the plugin, which needs its own handle on the
    // release. On Windows there is none yet: the announcement came from our
    // backend, so the manifest is read once more here — on the rare path, and
    // it is a kilobyte and a half.
    const update = pluginUpdate ?? (await check());
    if (!update) throw new Error('the release is no longer announced');
    await runInstaller(update);
  } catch (e) {
    // do not leave the snapshot behind, or a later ordinary launch would
    // suddenly open a video
    dropResumeSnapshot();
    updater.percent = null;
    showOsd(t('osd.update_failed'));
    console.warn('update failed:', e);
  }
}

/**
 * The Windows path. Returns false when the backend asked for the installer
 * instead, and otherwise does not return at all: `update_commit` replaces this
 * process with the new one.
 *
 * A commit that fails has put every file back and says so. It is not retried
 * here and does not fall through to the installer: the payload it staged is
 * gone with it, so a fallback would mean a second download of its own, and a
 * viewer who presses the button again gets exactly that — deliberately, rather
 * than as something the player decided for them after a failure it could not
 * explain.
 */
async function swapInPlace(): Promise<boolean> {
  const unlisten = await listen<number>(PROGRESS_EVENT, (e) => {
    updater.percent = e.payload;
  });
  try {
    const plan = await invoke<Plan>('update_prepare');
    if (plan.mode === 'download') {
      console.info('update: sending a portable copy to the release page -', plan.reason);
      updater.percent = null;
      dropResumeSnapshot();
      showOsd(t('osd.update_download'));
      await openUrl(releasePage(updater.available?.version ?? null));
      return true;
    }
    if (plan.mode === 'installer') {
      console.info('update: using the installer -', plan.reason);
      return false;
    }
    updater.percent = 100;
    // the download may have taken minutes — refresh the position first
    saveResumeSnapshot();
    await invoke('update_commit');
    // Reached only when the swap had nothing to move, which means this process
    // is already running those files: there is no relaunch and nothing to
    // restore afterwards, so put the button back the way a finished update
    // leaves it.
    dropResumeSnapshot();
    updater.percent = null;
    await checkForUpdate();
    return true;
  } finally {
    unlisten();
  }
}

/** What every release did before, and what macOS still does. */
async function runInstaller(update: Update) {
  let total = 0;
  let done = 0;
  updater.percent = 0;
  await update.downloadAndInstall((e) => {
    if (e.event === 'Started') total = e.data.contentLength ?? 0;
    else if (e.event === 'Progress') {
      done += e.data.chunkLength;
      if (total > 0) updater.percent = Math.round((done / total) * 100);
    } else if (e.event === 'Finished') {
      updater.percent = 100;
      saveResumeSnapshot();
    }
  });
  // Windows never gets here; macOS does.
  await relaunch();
}
