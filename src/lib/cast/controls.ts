/** The remote control: the live transport's verbs, answered optimistically. */

import { invoke } from '@tauri-apps/api/core';

import { t } from '../i18n.svelte';
import { showOsd } from '../osd.svelte';
import { cast } from './state.svelte';

// ---- Remote controls --------------------------------------------------------

/// Both transports answer the same four actions under different command names,
/// which is what lets one set of controls, one seekbar and one casting screen
/// serve either.
function controlCommand(): string {
  return transportCommand('control');
}

/// The live transport's own name for a verb — the three answer the same
/// questions under their own prefixes.
export function transportCommand(verb: 'status' | 'control' | 'disconnect'): string {
  return `${cast.transport}_${verb}`;
}

/// A refused command used to vanish into an empty catch, which is how a control
/// that does nothing looks exactly like a control that worked. A renderer
/// answers refusals in earnest (UPnP 606 "not authorized", 701 "transition not
/// available"), and the viewer is owed the difference.
function reportControlFailure(e: unknown) {
  console.warn('cast control failed:', e);
  showOsd(t('cast.command_failed'));
}

export function castTogglePause() {
  void invoke(controlCommand(), { action: cast.paused ? 'play' : 'pause', value: null }).catch(
    reportControlFailure,
  );
  // Optimistic, like every local control: the next status report is up to
  // half a second away and a button that lags reads as broken.
  cast.state = cast.paused ? 'playing' : 'paused';
}

export function castSeek(time: number) {
  const clamped = Math.max(0, Math.min(cast.duration || time, time));
  cast.time = clamped;
  void invoke(controlCommand(), { action: 'seek', value: clamped }).catch(reportControlFailure);
}

export function castSeekBy(delta: number) {
  castSeek(cast.time + delta);
}

/// TV volume is the receiver's own 0..1. Silent — the volume bar shows itself.
export function castSetVolume(frac: number) {
  if (!cast.volumeAdjustable) return;
  const next = Math.max(0, Math.min(1, frac));
  cast.volume = next;
  void invoke(controlCommand(), { action: 'volume', value: next }).catch(() => {});
}

/// The wheel/keys path: same value, plus the OSD (shown as percent). A device
/// whose volume is fixed (or never reported) gets the explanation instead of a
/// control that silently does nothing.
export function castNudgeVolume(delta: number) {
  if (!cast.volumeAdjustable) {
    showOsd(t('cast.volume_fixed'));
    return;
  }
  castSetVolume(cast.volume + delta);
  showOsd(t('osd.volume', { value: Math.round(cast.volume * 100) }), { progress: cast.volume });
}

/// The receiver's sound back on as its volume is raised — `unmute` in
/// player.svelte.ts says why.
export function castUnmute() {
  if (!cast.muted || !cast.volumeAdjustable) return;
  cast.muted = false;
  void invoke(controlCommand(), { action: 'mute', value: 0 }).catch(() => {});
}

export function castToggleMute() {
  // Mute is part of the volume control, not a separate capability: a receiver
  // that declares its volume fixed (or, over DLNA, never reports a usable one)
  // ignores this too, and the icon would flip and flip back on its own. The
  // button is disabled for the same reason the slider is; this is the keyboard
  // path, which gets the explanation instead.
  if (!cast.volumeAdjustable) {
    showOsd(t('cast.volume_fixed'));
    return;
  }
  const next = !cast.muted;
  cast.muted = next;
  void invoke(controlCommand(), { action: 'mute', value: next ? 1 : 0 }).catch(() => {});
  showOsd(t(next ? 'osd.sound_off' : 'osd.sound_on'));
}
