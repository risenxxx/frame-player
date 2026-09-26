/**
 * Removing the black bars baked into a picture: mpv's `video-crop`, chosen by
 * measuring the file or by naming a shape.
 *
 * **Auto measures the file, not the frame on screen.** The sidecar decodes
 * keyframes from across the whole length (`crop_detect`, crop.rs), so a night
 * scene cannot pass its dark edges off as bars. That is also why it is offered
 * for local files only. Over a stream or a torrent, eight seeks spread across
 * the file are eight requests for data nobody has fetched yet.
 *
 * **A crop is per file and remembered.** It is stored with the tracks and the
 * delays, since like them it answers "how does this file have to be played",
 * and it is stored as the rectangle, so reopening costs no decode. Like
 * rotation, `video-crop` survives a `loadfile` (measured on 0.41), so every
 * file gets its own value or none on `file-loaded`, through `setPicture` so the
 * reset stays behind the same "only if we touched it" guard as the rest of the
 * picture geometry.
 *
 * `dwidth`/`dheight` follow the crop (measured), so fit-to-video and the hover
 * preview's shape come along without anything here telling them. What does
 * need telling is where the kept part sits inside the full-frame thumbnail,
 * which is `objectPosition`.
 */

import { invoke } from '@tauri-apps/api/core';
import { getProperty } from 'tauri-plugin-libmpv-api';

import { aspectRect, formatRect, objectPosition, parseRect, type Frame, type Rect } from './crop-geometry';
import { cropFor, rememberCrop } from './history.svelte';
import { t } from './i18n.svelte';
import { latest } from './latest';
import { osdSeq, showOsd } from './osd.svelte';
import { isNetworkSource, player, setPicture } from './player.svelte';

export const CROP_MODES = ['none', 'auto', '2.39', '1.85', '4:3'] as const;
export type CropMode = (typeof CROP_MODES)[number];

/// The named shapes. 2.39 and 1.85 are the two theatrical ratios a film is
/// letterboxed to; 4:3 is the one a television programme is pillarboxed to.
const RATIOS: Record<Exclude<CropMode, 'none' | 'auto'>, number> = {
  '2.39': 2.39,
  '1.85': 1.85,
  '4:3': 4 / 3,
};

class CropState {
  mode = $state<CropMode>('none');
  /// The frame size the crop was measured against, for placing the preview.
  full = $state<{ w: number; h: number } | null>(null);
  /// Detections in flight. A count, not a flag: a second request can start
  /// before the first one returns, and the first to finish must not declare
  /// the menu idle.
  inflight = $state(0);

  get busy(): boolean {
    return this.inflight > 0;
  }

  /// Where the kept rectangle sits inside the full frame, as an
  /// `object-position`. The hover thumbnails are full frames with their bars,
  /// and `object-fit: cover` centres them in the cropped box, which is only
  /// right while the bars are equal.
  get objectPosition(): string {
    const r = parseRect(player.videoCrop);
    return r && this.full ? objectPosition(r, this.full) : '50% 50%';
  }
}

export const crop = new CropState();

/// A detection answers for the file it was asked about, and a newer choice or
/// a file change makes it moot.
const attempts = latest();

export function cropLabel(mode: CropMode): string {
  if (mode === 'none') return t('crop.none');
  if (mode === 'auto') return t('crop.auto');
  return mode;
}

/// Whether "auto" can run on what is playing.
export function canDetectCrop(): boolean {
  return !!player.filePath && !isNetworkSource(player.filePath);
}

async function frameSize(): Promise<Frame | null> {
  const [w, h, par] = await Promise.all([
    getProperty('video-params/w', 'int64').catch(() => null),
    getProperty('video-params/h', 'int64').catch(() => null),
    getProperty('video-params/par', 'double').catch(() => null),
  ]);
  if (!w || !h) return null;
  return { w, h, par: par && par > 0 ? par : 1 };
}

function apply(path: string, mode: CropMode, rect: Rect | null, full: Frame | null) {
  setPicture('video-crop', rect ? formatRect(rect) : '');
  crop.mode = rect ? mode : 'none';
  crop.full = rect && full ? { w: full.w, h: full.h } : null;
  rememberCrop(
    path,
    rect && full ? { mode, rect: formatRect(rect), fw: full.w, fh: full.h } : null,
  );
}

export async function setCrop(mode: CropMode) {
  const path = player.filePath;
  if (!path) return;
  const run = attempts.begin();
  if (mode === 'none') {
    apply(path, 'none', null, null);
    showOsd(t('osd.crop'), { sub: cropLabel('none') });
    return;
  }
  const frame = await frameSize();
  if (run.stale || !frame) return;

  let rect: Rect | null;
  if (mode === 'auto') {
    if (!canDetectCrop()) return;
    // Sticky: about a second on a 4K file, and a menu pill that does nothing
    // visible for a second reads as a pill that did not work.
    showOsd(t('osd.crop_detecting'), { sticky: true });
    const mine = osdSeq();
    crop.inflight++;
    let failed = false;
    try {
      rect = await invoke<Rect | null>('crop_detect', { path });
    } catch (e) {
      console.warn('[crop] detection failed:', e);
      rect = null;
      failed = true;
    } finally {
      crop.inflight--;
    }
    if (run.stale) {
      // The sticky popup is still this detection's, and nothing else is coming
      // to replace it.
      if (osdSeq() === mine) showOsd(t('osd.crop'), { sub: t('crop.canceled') });
      return;
    }
    if (!rect) {
      showOsd(t('osd.crop'), { sub: t(failed ? 'crop.failed' : 'crop.no_bars') });
      return;
    }
  } else {
    rect = aspectRect(frame, RATIOS[mode], player.aspectOverride, player.videoRotate);
    if (!rect) {
      showOsd(t('osd.crop'), { sub: t('crop.already') });
      return;
    }
  }
  apply(path, mode, rect, frame);
  showOsd(t('osd.crop'), { sub: cropLabel(mode) });
}

/// Put this file's own crop back, or none. Runs on `file-loaded`, after
/// `resetPicture`, and cancels a detection still running for the file before.
export function restoreCrop() {
  attempts.begin();
  crop.mode = 'none';
  crop.full = null;
  const path = player.filePath;
  if (!path) return;
  const saved = cropFor(path);
  if (!saved || !parseRect(saved.rect)) return;
  setPicture('video-crop', saved.rect);
  crop.mode = (CROP_MODES as readonly string[]).includes(saved.mode) ? (saved.mode as CropMode) : 'auto';
  crop.full = { w: saved.fw, h: saved.fh };
}
