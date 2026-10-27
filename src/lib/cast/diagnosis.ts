/** The device check: everything the player can learn about a television without playing anything. */

import { invoke } from '@tauri-apps/api/core';

import { t } from '../i18n.svelte';
import type { DlnaDevice, TvDevice } from './state.svelte';

export interface CheckLine {
  /// A stable name for the check; the label and the explanation are this
  /// side's, because Rust cannot reach the dictionary.
  id: string;
  state: 'ok' | 'warn' | 'fail' | 'info' | 'timeout';
  /// What only Rust knows: addresses, lists, the device's own error text.
  detail: string;
}

/// The label and, when there is one to give, the sentence that says what the
/// answer means. A check with nothing to explain shows only its data.
export function checkLabel(line: CheckLine): string {
  return t(`diag.${line.id}` as Parameters<typeof t>[0]);
}

export function checkNote(line: CheckLine): string {
  const key = `diag.${line.id}_${line.state}`;
  const note = t(key as Parameters<typeof t>[0]);
  // `t` echoes an unknown key, which is the signal that this state needs no
  // sentence — the data speaks for itself.
  return note === key ? '' : note;
}

/**
 * Everything the player can learn about a device without playing anything.
 *
 * The case it exists for is somebody else's television, where a cast fails and
 * the difference between "the network cannot reach it", "the receiver refused
 * the file" and "this device does not do that at all" is invisible from the
 * outside and decides everything. Each check is one the normal flow already
 * makes; here their answers are written down instead of consumed.
 */
export async function diagnoseDevice(device: TvDevice): Promise<CheckLine[]> {
  const lines = await invoke<CheckLine[]>('cast_diagnose', {
    ip: device.ip,
    castPort: device.cast?.port ?? null,
    // The description URL, not the control URL: the command re-reads the whole
    // description, which is where the model, the services and their own
    // descriptions come from.
    dlnaLocation: device.dlna ? dlnaDescriptionUrl(device.dlna) : null,
  }).catch((e) => [{ id: 'diagnosis', state: 'fail' as const, detail: String(e) }]);
  return lines;
}

function dlnaDescriptionUrl(d: DlnaDevice): string {
  const [origin] = d.control_url.split('/AVTransport/');
  return origin ? `${origin}/` : d.control_url;
}

/// The report as one block of text — what actually reaches a bug report.
export function diagnosisText(device: TvDevice, lines: CheckLine[]): string {
  const head = [
    `Frame Player — device check`,
    `${device.name}${device.model && device.model !== device.name ? ` (${device.model})` : ''}`,
    `${device.ip} · transports: ${[device.cast && 'Chromecast', device.dlna && 'DLNA']
      .filter(Boolean)
      .join(', ')}`,
    '',
  ];
  return [
    ...head,
    ...lines.map((l) => `[${l.state}] ${l.id}${l.detail ? `: ${l.detail}` : ''}`),
  ].join('\n');
}
