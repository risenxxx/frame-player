/**
 * Volume leveling as a one-gesture toggle: the hotkey, the context menu's row
 * and the control bar's indicator all come through here (issue #58 — the
 * switch in Settings → Audio was the only way to reach it, which is too far
 * for something wanted at night, in the middle of a film).
 *
 * The filter itself and its stored state are `applyNormalize`'s
 * (player/filters.ts). What this module adds is the one fact those gestures
 * need and the settings sheet reads from its own mpv.conf copy: whether audio
 * is being bitstreamed, in which case there is no decoded sound for the filter
 * to work on and saying so beats a toggle that silently does nothing.
 */
import { getProperty } from 'tauri-plugin-libmpv-api';

import { t } from './i18n.svelte';
import { showOsd } from './osd.svelte';
import { applyNormalize, player } from './player.svelte';
import { playback } from './playback.svelte';

class Leveling {
  /// `audio-spdif` names at least one codec. Read at start-up and whenever the
  /// settings sheet changes that option; nothing else can change it while the
  /// player runs (an mpv.conf edited by hand is read at the next start).
  bitstream = $state(false);
}

export const leveling = new Leveling();

export async function refreshLeveling() {
  const spdif = await getProperty('audio-spdif', 'string').catch(() => '');
  leveling.bitstream = Boolean(spdif);
}

/// Whether the filter is on *and* doing something the viewer hears — what the
/// indicator and the menu's check mark show. Off while casting (the television
/// decodes its own sound) and while bitstreaming.
export function levelingHeard(): boolean {
  return player.normalize && !leveling.bitstream && playback.can.leveling;
}

export function toggleLeveling() {
  if (leveling.bitstream) {
    showOsd(t('set.normalize'), { sub: t('osd.leveling_spdif') });
    return;
  }
  const on = !player.normalize;
  applyNormalize(on);
  showOsd(t(on ? 'osd.leveling_on' : 'osd.leveling_off'));
}
