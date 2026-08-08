/**
 * Zoom into a region of the video (Ctrl+wheel) and pan around it.
 *
 * Mirrors of mpv's video-zoom / video-pan-x / video-pan-y. The three have to
 * land in one frame or the picture jerks near the edges, which is why they go
 * through a Lua handler on mpv's core thread — even back-to-back property sets
 * from outside let the VO redraw in between.
 */

import { invoke } from '@tauri-apps/api/core';
import { command, setProperty } from 'tauri-plugin-libmpv-api';

import { t } from './i18n.svelte';
import { showOsd } from './osd.svelte';
import { player } from './player.svelte';

/// log2 scale (0 = 100%), and the pan offsets as a fraction of the scaled size.
let zoomZ = 0;
let panX = 0;
let panY = 0;

/// zoompan.lua is loaded, so the atomic path is available.
let luaOk = false;

export function markZoomLuaLoaded() {
  luaOk = true;
}

/** Is the video magnified? Decides whether a video drag pans or moves the window. */
export function isZoomed(): boolean {
  return zoomZ > 0.0001;
}

/** Video size at zoom=0 (contain-fitted into the window). */
function baseSize(): { w: number; h: number } | null {
  if (player.videoW <= 0 || player.videoH <= 0) return null;
  const W = window.innerWidth;
  const H = window.innerHeight;
  const s = Math.min(W / player.videoW, H / player.videoH);
  return { w: player.videoW * s, h: player.videoH * s };
}

/**
 * Pan within the content: the video covers the window on the axis where it is
 * larger and stays centered on the axis where it is smaller, so no black bars
 * appear.
 */
function clampPan() {
  const base = baseSize();
  if (!base) return;
  const sx = base.w * Math.pow(2, zoomZ);
  const sy = base.h * Math.pow(2, zoomZ);
  const maxPx = sx > window.innerWidth ? (sx - window.innerWidth) / (2 * sx) : 0;
  const maxPy = sy > window.innerHeight ? (sy - window.innerHeight) / (2 * sy) : 0;
  panX = Math.min(maxPx, Math.max(-maxPx, panX));
  panY = Math.min(maxPy, Math.max(-maxPy, panY));
}

function applyZoomPan() {
  const z = Math.round(zoomZ * 1e6) / 1e6;
  const x = Math.round(panX * 1e6) / 1e6;
  const y = Math.round(panY * 1e6) / 1e6;
  if (luaOk) {
    // The zoompan.lua handler applies all three properties on mpv's core thread
    // — the only genuinely atomic path (no intermediate frame, which is what
    // made the picture jerk near the edges). Specifically -to (addressed to the
    // script): a broadcast script-message also reaches the plugin's own client,
    // whose wrapper crashes serializing client-message (an AV in ucrtbase).
    void command('script-message-to', ['zoompan', 'zoompan', String(z), String(x), String(y)]);
    return;
  }
  // fallback: three property sets in a row from Rust (microsecond gaps)
  void invoke('zoom_pan', { z, x, y }).catch(() => {
    void setProperty('video-zoom', z);
    void setProperty('video-pan-x', x);
    void setProperty('video-pan-y', y);
  });
}

/** Zoom that keeps the point under the cursor in place. */
export function zoomAt(cx: number, cy: number, dz: number) {
  const base = baseSize();
  if (!base) return;
  const oldZ = zoomZ;
  const newZ = Math.min(3, Math.max(0, oldZ + dz));
  if (newZ === oldZ) return;
  const k = Math.pow(2, newZ - oldZ);
  const sx = base.w * Math.pow(2, oldZ);
  const sy = base.h * Math.pow(2, oldZ);
  const dx = cx - window.innerWidth / 2;
  const dy = cy - window.innerHeight / 2;
  zoomZ = newZ;
  panX -= (dx / sx) * (1 - 1 / k);
  panY -= (dy / sy) * (1 - 1 / k);
  clampPan();
  applyZoomPan();
  showOsd(t('osd.zoom', { value: Math.round(Math.pow(2, newZ) * 100) }));
}

/** Drag the magnified region by a pixel delta. */
export function panBy(dxPixels: number, dyPixels: number) {
  const base = baseSize();
  if (!base) return;
  panX += dxPixels / (base.w * Math.pow(2, zoomZ));
  panY += dyPixels / (base.h * Math.pow(2, zoomZ));
  clampPan();
  applyZoomPan();
}

/** Re-clamp after a window resize: the bounds depend on the window size. */
export function reclampPan() {
  if (zoomZ <= 0) return;
  clampPan();
  applyZoomPan();
}

export function resetZoom(silent = false) {
  if (zoomZ === 0 && panX === 0 && panY === 0) return;
  zoomZ = 0;
  panX = 0;
  panY = 0;
  applyZoomPan();
  if (!silent) showOsd(t('osd.zoom', { value: 100 }));
}
