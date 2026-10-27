/**
 * The casting device check: the dialog that runs a television through the
 * checks in `diagnoseDevice` and lets the viewer copy the result. Opened from
 * the cast menu; `DiagnosisDialog.svelte` draws it from `diag`.
 *
 * Out of the page's script.
 */

import { diagnoseDevice, diagnosisText, type CheckLine, type TvDevice } from './cast.svelte';
import { t } from './i18n.svelte';
import { showOsd } from './osd.svelte';
import { overlays } from './overlays.svelte';

class CastDiagnosis {
  busy = $state<string | null>(null);
  open = $state(false);
  device = $state<TvDevice | null>(null);
  lines = $state<CheckLine[]>([]);
}

export const diag = new CastDiagnosis();

export async function runDiagnosis(device: TvDevice) {
  diag.busy = device.key;
  diag.device = device;
  diag.lines = [];
  // The dialog opens at once and fills in: the checks take a couple of
  // seconds against a device in standby, and a button that does nothing
  // visible for that long reads as a button that did nothing.
  diag.open = true;
  overlays.menu = null;
  try {
    diag.lines = await diagnoseDevice(device);
  } finally {
    diag.busy = null;
  }
}

export async function copyDiagnosis(device: TvDevice) {
  // The webview's own clipboard: this is text, and the Rust path next door
  // exists for an image the clipboard cannot take any other way.
  try {
    await navigator.clipboard.writeText(diagnosisText(device, diag.lines));
    showOsd(t('cast.diagnose_copied'));
  } catch (e) {
    console.warn('clipboard write failed:', e);
    showOsd(t('cast.diagnose_copy_failed'));
  }
}
