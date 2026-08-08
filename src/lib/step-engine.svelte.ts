/**
 * The FFmpeg-sidecar frame stepper.
 *
 * **Disabled** — `USE_STEP_ENGINE` is false, and every entry point here falls
 * back to mpv's own `frame-step`. It is kept working and smoke-tested because
 * mpv cannot beat the keyframe ceiling on pathological files and a decoded
 * frame source is the only way past it (see architecture.md).
 *
 * While it is on, frames come from the sidecar and mpv sits paused behind the
 * canvas overlay, walked to the displayed frame by a background pre-seek so
 * that resuming playback is instant.
 */

import { invoke } from '@tauri-apps/api/core';
import { command, getProperty, setProperty } from 'tauri-plugin-libmpv-api';

import { player, togglePause as mpvTogglePause, waitPlaybackSettled } from './player.svelte';
import { seek } from './seek.svelte';

/// Sidecar stepping is off: mpv's native steps give the correct picture (proper
/// HDR tone mapping) and are fast enough. The code stays for a possible
/// comeback behind an option.
export const USE_STEP_ENGINE: boolean = false;

class Step {
  /// The canvas overlay is showing a sidecar frame.
  on = $state(false);
  pts = $state(0);
  canvas = $state<HTMLCanvasElement | null>(null);
}

export const step = new Step();

/// One step at a time: the sidecar is a single decode session.
let busy = false;
let preSeekTimer: ReturnType<typeof setTimeout> | undefined;
let prewarmTimer: ReturnType<typeof setTimeout> | undefined;

function drawStepFrame(buf: ArrayBuffer) {
  const dv = new DataView(buf);
  const w = dv.getUint32(0, true);
  const h = dv.getUint32(4, true);
  const pts = dv.getFloat64(8, true);
  if (step.canvas) {
    if (step.canvas.width !== w) step.canvas.width = w;
    if (step.canvas.height !== h) step.canvas.height = h;
    const px = new Uint8ClampedArray(buf, 16);
    step.canvas.getContext('2d')!.putImageData(new ImageData(px, w, h), 0, 0);
  }
  step.pts = pts;
  seek.value = pts;
}

function schedulePreSeek() {
  clearTimeout(preSeekTimer);
  preSeekTimer = setTimeout(() => {
    void command('seek', [step.pts, 'absolute+exact']).catch(() => {});
  }, 150);
}

export function schedulePrewarm() {
  clearTimeout(prewarmTimer);
  prewarmTimer = setTimeout(() => {
    if (player.filePath && !step.on) {
      void invoke('step_prewarm', { path: player.filePath, pos: player.timePos }).catch(() => {});
    }
  }, 150);
}

export async function stepBy(delta: number) {
  if (!player.hasFile || busy) return;
  if (!USE_STEP_ENGINE) {
    void command(delta < 0 ? 'frame-back-step' : 'frame-step', []);
    return;
  }
  busy = true;
  try {
    if (!step.on) {
      if (!player.filePath) throw new Error('no path');
      await setProperty('pause', true);
      const pos = (await getProperty('time-pos', 'double').catch(() => null)) ?? player.timePos;
      const entry = await invoke<ArrayBuffer>('step_enter', { path: player.filePath, pos });
      step.on = true;
      drawStepFrame(entry);
    }
    const buf = await invoke<ArrayBuffer>('step_move', { delta });
    drawStepFrame(buf);
    schedulePreSeek();
  } catch {
    // the sidecar could not cope (exotic format etc.) — fall back to mpv
    cancelStep();
    void command(delta < 0 ? 'frame-back-step' : 'frame-step', []);
  } finally {
    busy = false;
  }
}

/** Exit without seeking: used when the user is about to set the position. */
export function cancelStep() {
  if (!step.on) return;
  step.on = false;
  clearTimeout(preSeekTimer);
  void invoke('step_exit').catch(() => {});
}

/** Syncs mpv to the displayed frame and exits (before a relative seek). */
export function flushStepThenCancel() {
  clearTimeout(preSeekTimer);
  void command('seek', [step.pts, 'absolute+exact']).catch(() => {});
  cancelStep();
}

/** Exit and resume: the canvas hides only once mpv is on the frame. */
export async function exitStep(unpause: boolean) {
  if (!step.on) return;
  clearTimeout(preSeekTimer);
  try {
    const settled = waitPlaybackSettled(250);
    await command('seek', [step.pts, 'absolute+exact']);
    await settled;
  } catch {
    // ignore
  }
  step.on = false;
  void invoke('step_exit').catch(() => {});
  if (unpause) await setProperty('pause', false).catch(() => {});
}

/**
 * Play/pause with the frame-step overlay taken into account.
 *
 * While the overlay is up mpv is parked behind it, so the first press means
 * "come back to playing" rather than "pause". It lives here because that is the
 * only fact it encodes; every caller — the key, the click on the video, the OSC
 * button — wants the same answer.
 */
export async function togglePlayback() {
  if (!player.hasFile) return;
  if (step.on) {
    await exitStep(true);
    return;
  }
  await mpvTogglePause();
}
