/**
 * Removing the black bars baked into a picture: mpv's `video-crop`, chosen by
 * measuring the file or by naming a shape.
 *
 * **Auto measures the file, not the frame on screen.** The sidecar decodes
 * keyframes from across the whole length (`crop_detect`, crop.rs), so a night
 * scene cannot pass its dark edges off as bars. A torrent is measured from its
 * file in the cache, and while that is still arriving only inside the stretches
 * already downloaded (`detectionTarget`). A plain stream is not offered it at
 * all: eight seeks across the file would be eight requests for data nobody
 * has fetched yet.
 *
 * **The mode is the playlist's, the rectangle is each file's own.** Picking a
 * mode on one episode records it for the folder or the torrent, and every file
 * there applies it *to itself* when it opens: "auto" measures that file, a
 * named shape is computed from that file's frame. Episodes of one release
 * almost always share their bars, and a folder of unrelated videos is where
 * nobody sets a crop on one of them; but a rectangle copied from one file to
 * another would cut picture off wherever they differ, so it never is. Each
 * file caches the rectangle it came to (an empty one for "no bars here"),
 * which is what makes reopening free, and the cache counts only while its mode
 * is still the playlist's. Like
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
import { cropFor, cropModeFor, hasPlaylistScope, rememberCrop, rememberCropMode } from './history.svelte';
import { t } from './i18n.svelte';
import { latest } from './latest';
import { osdSeq, showOsd } from './osd.svelte';
import { isNetworkSource, player, setPicture } from './player.svelte';
import { parseTorrentUrl } from './source';
import { positionBuffered } from './torrent.svelte';

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
  /// The file belongs to a playlist the mode is kept for (a link does not).
  playlist = $state(false);

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

/// Whether "auto" can run on what is playing: a file on disk, or a torrent,
/// whose pieces are a file on disk too.
export function canDetectCrop(): boolean {
  const path = player.filePath;
  return !!path && (!isNetworkSource(path) || !!parseTorrentUrl(path));
}

/// mpv reports the frame size after the first decoded frame, not on
/// `file-loaded`: poll for it this often, this many times.
const FRAME_WAIT_MS = 100;
const FRAME_WAIT_TRIES = 50;
/// "Auto" on a file that opens: wait this long first, then try this many times
/// in all, this far apart when a torrent has too little downloaded.
const AUTO_DELAY_MS = 1500;
const AUTO_RETRY_MS = 30_000;
const AUTO_RETRIES = 4;

/// How many points of the file the band search tries before picking its
/// samples. Fine enough that a band of a few percent is found at all.
const BAND_GRID = 200;
/// Samples asked for: the sidecar's own number for a whole file.
const BAND_SAMPLES = 8;

/**
 * What the sidecar should decode for this source.
 *
 * A local file is read whole and sampled evenly. A torrent is read from its
 * file in the cache, and while that file is still arriving it has holes: the
 * samples are then taken **only inside what has been downloaded**
 * (`positionBuffered`, the same test the hover previews pass), spread over
 * all of it. Usually that is the start and the stretch around the playhead,
 * which is fewer scenes than a whole film, and that costs only confidence, not
 * correctness: `combine` keeps the smallest bar per edge, so fewer scenes can
 * make it crop less, never more. `'early'` means too little is down to sample
 * three distinct points.
 */
async function detectionTarget(
  path: string,
): Promise<{ file: string; positions?: number[] } | 'early' | null> {
  if (!isNetworkSource(path)) return { file: path };
  const ref = parseTorrentUrl(path);
  if (!ref) return null;
  const local = await invoke<{ path: string; complete: boolean } | null>('torrent_local_path', {
    infoHash: ref.infoHash,
    index: ref.index,
  }).catch(() => null);
  if (!local) return null;
  if (local.complete) return { file: local.path };
  const duration = player.duration;
  if (!(duration > 0)) return 'early';
  const have: number[] = [];
  for (let i = 0; i < BAND_GRID; i++) {
    const f = 0.02 + (0.96 * i) / (BAND_GRID - 1);
    if (positionBuffered(f)) have.push(f);
  }
  if (have.length < 3) return 'early';
  const n = Math.min(BAND_SAMPLES, have.length);
  const picks = Array.from({ length: n }, (_, i) => have[Math.round((i * (have.length - 1)) / (n - 1))]);
  return { file: local.path, positions: picks.map((f) => f * duration) };
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

/// Wait for mpv to report the frame size, which it does only once the first
/// frame is decoded — after `file-loaded`, where the restore starts.
async function waitFrame(run: { stale: boolean }): Promise<Frame | null> {
  for (let i = 0; i < FRAME_WAIT_TRIES; i++) {
    const frame = await frameSize();
    if (run.stale) return null;
    if (frame) return frame;
    await new Promise((r) => setTimeout(r, FRAME_WAIT_MS));
  }
  return null;
}

type Measured = { rect: Rect | null; frame: Frame } | 'early' | 'failed' | null;

/// The rectangle `mode` comes to on this file: measured for auto, computed for
/// a named shape. Null when the answer no longer matters (a newer attempt, or
/// no frame to measure).
async function measure(path: string, mode: Exclude<CropMode, 'none'>, run: { stale: boolean }): Promise<Measured> {
  const frame = await waitFrame(run);
  if (!frame) return null;
  if (mode !== 'auto') {
    return { rect: aspectRect(frame, RATIOS[mode], player.aspectOverride, player.videoRotate), frame };
  }
  if (!canDetectCrop()) return 'failed';
  crop.inflight++;
  try {
    const target = await detectionTarget(path);
    if (target === 'early') return 'early';
    if (!target) return 'failed';
    const rect = await invoke<Rect | null>('crop_detect', {
      path: target.file,
      positions: target.positions ?? null,
    });
    return run.stale ? null : { rect, frame };
  } catch (e) {
    console.warn('[crop] detection failed:', e);
    return 'failed';
  } finally {
    crop.inflight--;
  }
}

/// Put `rect` in force for this file and cache it. The cache is kept for "no
/// bars here" as well (an empty rectangle), so an episode that had none is not
/// decoded again every time it is opened.
function apply(path: string, mode: CropMode, rect: Rect | null, frame: Frame | null) {
  setPicture('video-crop', rect ? formatRect(rect) : '');
  crop.mode = mode;
  crop.full = rect && frame ? { w: frame.w, h: frame.h } : null;
  rememberCrop(
    path,
    mode === 'none' ? null : { mode, rect: rect ? formatRect(rect) : '', fw: frame?.w ?? 0, fh: frame?.h ?? 0 },
  );
}

/**
 * The viewer picked a mode in the menu. It applies to this file at once and
 * is recorded for the playlist, where every other file will apply it to
 * itself when it opens (`restoreCrop`).
 */
export async function setCrop(mode: CropMode) {
  const path = player.filePath;
  if (!path) return;
  const run = attempts.begin();
  rememberCropMode(path, mode);
  if (mode === 'none') {
    apply(path, 'none', null, null);
    showOsd(t('osd.crop'), { sub: cropLabel('none') });
    return;
  }
  let mine = -1;
  if (mode === 'auto') {
    // Sticky: about a second on a 4K file, and a menu pill that does nothing
    // visible for a second reads as a pill that did not work.
    showOsd(t('osd.crop_detecting'), { sticky: true });
    mine = osdSeq();
  }
  const r = await measure(path, mode, run);
  if (run.stale || r === null) {
    // A sticky popup still standing is this attempt's, and nothing else is
    // coming to replace it.
    if (mine >= 0 && osdSeq() === mine) showOsd(t('osd.crop'), { sub: t('crop.canceled') });
    return;
  }
  if (r === 'early' || r === 'failed') {
    // The mode stands (the playlist has it, and this file tries again when
    // reopened); only this file's answer is missing.
    setPicture('video-crop', '');
    crop.mode = mode;
    crop.full = null;
    showOsd(t('osd.crop'), { sub: t(r === 'early' ? 'crop.too_early' : 'crop.failed') });
    return;
  }
  apply(path, mode, r.rect, r.frame);
  showOsd(t('osd.crop'), {
    sub: r.rect ? cropLabel(mode) : t(mode === 'auto' ? 'crop.no_bars' : 'crop.already'),
  });
}

function validMode(m: string | null | undefined): CropMode | null {
  return m && (CROP_MODES as readonly string[]).includes(m) ? (m as CropMode) : null;
}

/**
 * Put this file's crop in force: the playlist's mode, applied to this file.
 * Runs on `file-loaded`, after `resetPicture`, and cancels anything still
 * running for the file before.
 *
 * A rectangle cached for this file under the same mode is used as it is.
 * Otherwise it is worked out here, in the background: a named shape as soon
 * as mpv knows the frame size, "auto" a moment later so the detection's decode
 * does not compete with the file's own first frames. A torrent with too little
 * downloaded is tried again a few times while it plays.
 */
export function restoreCrop() {
  const run = attempts.begin();
  crop.mode = 'none';
  crop.full = null;
  const path = player.filePath;
  if (!path) return;
  crop.playlist = hasPlaylistScope(path);
  const cached = cropFor(path);
  const mode = validMode(cropModeFor(path)) ?? validMode(cached?.mode) ?? 'none';
  if (mode === 'none') return;
  crop.mode = mode;
  if (cached && cached.mode === mode) {
    if (parseRect(cached.rect)) {
      setPicture('video-crop', cached.rect);
      crop.full = { w: cached.fw, h: cached.fh };
    }
    return;
  }
  void (async () => {
    for (let tries = 0; tries < AUTO_RETRIES; tries++) {
      const wait = mode === 'auto' ? (tries === 0 ? AUTO_DELAY_MS : AUTO_RETRY_MS) : 0;
      if (wait) await new Promise((r) => setTimeout(r, wait));
      if (run.stale) return;
      const r = await measure(path, mode, run);
      if (run.stale || r === null || r === 'failed') return;
      if (r === 'early') continue;
      apply(path, mode, r.rect, r.frame);
      // The picture changed shape on its own; say why.
      if (r.rect) showOsd(t('osd.crop'), { sub: cropLabel(mode) });
      return;
    }
  })();
}
