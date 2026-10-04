/**
 * A sound offset per output device.
 *
 * A Bluetooth speaker or headset adds a delay of its own — the codec's
 * buffer, the radio — and the system reports it to the player only in part,
 * so with one connected the picture leads the sound by a fixed amount. That
 * amount belongs to the *speaker*, not to the film: the per-file audio delay
 * (`tracks.svelte.ts`) is for a file muxed out of sync, and asking somebody to
 * dial a speaker's lag into every file they open, and out again when the sound
 * goes back to the laptop, is no answer to it. So the correction is kept per
 * device, found again when that device is the one playing, and dropped when it
 * is not.
 *
 * mpv has one `audio-delay`. What it holds is the sum — this file's own delay
 * plus this device's offset — and the two halves are kept apart by the one
 * module that writes it (`tracks.svelte.ts`, through `onDeviceOffset`), so a
 * file never remembers a speaker's lag as its own.
 *
 * Which device is playing: with a device chosen in the settings, mpv names it
 * (`coreaudio/<UID>`, `wasapi/<id>`); left on `auto` — the default — mpv plays
 * through the system's default output and does not say which that is, so the
 * native side watches it (`audio_output.rs`). The id after mpv's prefix is the
 * system's own id for the device, so one speaker has one key either way.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getProperty } from 'tauri-plugin-libmpv-api';

import { t } from './i18n.svelte';
import { showOsd } from './osd.svelte';
import { readList } from './player.svelte';

const OFFSETS_KEY = 'frameplayer.audioOffsets';

/// Nothing a speaker does takes longer than this, and a value past it is a
/// slip of the hotkey rather than a correction.
export const OFFSET_LIMIT = 2;

type Device = { id: string; name: string };
type Record = { name: string; offset: number };

class AudioOutput {
  /// The device playing now, or null when nothing could tell.
  id = $state<string | null>(null);
  /// What the system calls it — the speaker's own name, for the menu.
  name = $state<string | null>(null);
  /// Its offset in seconds; positive delays the sound, like mpv's `audio-delay`.
  offset = $state(0);
}

export const output = new AudioOutput();

/// The system's default output, as last reported by the native watcher.
let systemDefault: Device | null = null;

function load(): { [id: string]: Record } {
  try {
    return JSON.parse(localStorage.getItem(OFFSETS_KEY) ?? '{}');
  } catch {
    return {};
  }
}

function save(map: { [id: string]: Record }) {
  try {
    localStorage.setItem(OFFSETS_KEY, JSON.stringify(map));
  } catch {
    // localStorage unavailable — the offset still applies for this session
  }
}

/// Rounded to the 10 ms the player shows, and zero stored as nothing.
function tidy(seconds: number): number {
  const clamped = Math.max(-OFFSET_LIMIT, Math.min(OFFSET_LIMIT, seconds));
  const rounded = Math.round(clamped * 100) / 100;
  return Math.abs(rounded) < 0.005 ? 0 : rounded;
}

let listener: ((from: number, to: number) => void) | null = null;

/// Told whenever the offset in force changes — another device, or the same one
/// re-dialled. The one writer of `audio-delay` registers here.
export function onDeviceOffset(fn: (from: number, to: number) => void) {
  listener = fn;
}

function adopt(device: Device | null) {
  const before = output.offset;
  output.id = device?.id ?? null;
  output.name = device?.name ?? null;
  output.offset = device ? (load()[device.id]?.offset ?? 0) : 0;
  if (output.offset !== before) listener?.(before, output.offset);
}

/// The id mpv's device name carries after its output's prefix.
function idOf(mpvName: string): string {
  const slash = mpvName.indexOf('/');
  return slash >= 0 ? mpvName.slice(slash + 1) : mpvName;
}

async function chosenDevice(): Promise<Device | null> {
  const chosen = await getProperty('audio-device', 'string').catch(() => null);
  if (!chosen || chosen === 'auto') return null;
  const list = await readList('audio-device-list', async (base) => {
    const name = await getProperty(`${base}/name`, 'string').catch(() => null);
    if (name !== chosen) return null;
    return (await getProperty(`${base}/description`, 'string').catch(() => null))?.trim() || null;
  }).catch(() => []);
  return { id: idOf(chosen), name: list[0] ?? idOf(chosen) };
}

/// Work out which device is playing and put its offset in force. Called when
/// the system's default changes and when the settings choose a device.
export async function refreshAudioOutput() {
  adopt((await chosenDevice()) ?? systemDefault);
}

export function setDeviceOffset(seconds: number) {
  if (!output.id || !output.name) return;
  const value = tidy(seconds);
  const map = load();
  if (value === 0) delete map[output.id];
  else map[output.id] = { name: output.name, offset: value };
  save(map);
  const before = output.offset;
  output.offset = value;
  if (value !== before) listener?.(before, value);
}

/// One press of the menu's −/+, with the popup the file's delay gets.
export function nudgeDeviceOffset(delta: number) {
  if (!output.name) return;
  setDeviceOffset(output.offset + delta);
  showOsd(
    t('osd.device_offset', {
      device: output.name,
      value: (output.offset > 0 ? '+' : '') + output.offset.toFixed(2),
    }),
  );
}

/// The standing half: the native watcher's reports. Must be called from a
/// component's initialisation (it installs an effect).
export function initAudioOutput() {
  $effect(() => {
    let gone = false;
    let unlisten: (() => void) | null = null;
    void listen<Device | null>('frameplayer://audio-output', (e) => {
      systemDefault = e.payload;
      void refreshAudioOutput();
    }).then((un) => {
      if (gone) un();
      else unlisten = un;
    });
    // The watcher may have spoken before the listener was in place.
    void invoke<Device | null>('audio_output_default')
      .then((device) => {
        if (device && !systemDefault) systemDefault = device;
        return refreshAudioOutput();
      })
      .catch(() => {});
    return () => {
      gone = true;
      unlisten?.();
    };
  });
}
