/**
 * The upscaler's arithmetic, apart from mpv so that it can be tested.
 *
 * **Whether a shader runs is the shader's own decision, made on every frame.**
 * Each pass carries a `//!WHEN` line, and mpv skips the pass when it is false:
 * ArtCNN's is `OUTPUT.w LUMA.w / 1.3 > OUTPUT.h LUMA.h / 1.3 > *`, Anime4K's
 * the same with 1.2. So a picture drawn at its own size, or smaller, is never
 * enlarged and costs nothing, whatever the switch says — which is the common
 * case for a 1080p film on a 1080p screen, and for anything 4K. The panel says
 * which of the two it is (`upscaleRatio`), or the switch would look broken
 * exactly where it does nothing.
 *
 * What the two sides of that comparison are, measured on mpv 0.41 with
 * gpu-next by counting render passes (`vo-passes`) — and two of the answers
 * are not what the names suggest:
 * - `LUMA`/`MAIN` is the **whole decoded plane**: `video-crop` does not change
 *   it. A 1280×720 frame cropped to 640×360 in a 1000×562 window is drawn
 *   1.56 times its size and the shader does not run, because against the
 *   whole frame that window is 0.78.
 * - `OUTPUT` is the **part of the picture inside the window**: `video-zoom`
 *   does not grow it. The same frame at zoom 1 in a 1600×900 window stays
 *   below the threshold (1.25), and runs at 1760×990 (1.375).
 * - The two are compared **per axis in the frame's own orientation**: turned
 *   by 90° the plane's width is measured against the screen's height.
 *   `video-aspect-override` stretches one axis only, and both must clear.
 *
 * `osd-dimensions` gives that window part: the picture's box is its size less
 * the margins, and a margin goes negative when the picture overflows.
 */

/// Which shader enlarges the picture: none, the one for filmed video, the one
/// for drawn animation.
export type UpscaleMode = 'off' | 'video' | 'anime';

export const UPSCALE_MODES: readonly UpscaleMode[] = ['off', 'video', 'anime'];

/**
 * The shader for each mode and the factor its `//!WHEN` lines demand, copied
 * from the files in `src-tauri/shaders/`.
 *
 * Both are ×2 networks, and the lightest of their families. Measured on an M3
 * Max, 1080p into 3400×2000, the GPU time per frame (`vo-passes`, the sum of
 * the averages): 2.5 ms without a shader, 8.8 ms for ArtCNN C4F16, 37 ms for
 * C4F32 — past a 24 fps frame on the fastest Mac there is — and 6.4 / 8.6 /
 * 15 ms for Anime4K M / L / VL. An integrated GPU has a fraction of that
 * machine's cores, so the light ones are the only ones that can be offered as a
 * switch rather than as a gamble.
 */
export const UPSCALE_SHADERS: Record<Exclude<UpscaleMode, 'off'>, { file: string; threshold: number }> = {
  video: { file: 'ArtCNN_C4F16.glsl', threshold: 1.3 },
  anime: { file: 'Anime4K_Upscale_CNN_x2_M.glsl', threshold: 1.2 },
};

/// `osd-dimensions/{w,h,ml,mr,mt,mb}`: the VO's surface in device pixels and
/// the margins between it and the picture.
export interface OsdBox {
  w: number;
  h: number;
  ml: number;
  mr: number;
  mt: number;
  mb: number;
}

/**
 * How many times the picture is enlarged on each axis, the way the shaders
 * measure it; null with nothing to measure.
 */
export function upscaleRatio(
  osd: OsdBox,
  plane: { w: number; h: number },
  rotate: number,
): { x: number; y: number } | null {
  const w = osd.w - Math.max(0, osd.ml) - Math.max(0, osd.mr);
  const h = osd.h - Math.max(0, osd.mt) - Math.max(0, osd.mb);
  const turned = ((rotate % 180) + 180) % 180 === 90;
  const pw = turned ? plane.h : plane.w;
  const ph = turned ? plane.w : plane.h;
  if (w <= 0 || h <= 0 || pw <= 0 || ph <= 0) return null;
  return { x: w / pw, y: h / ph };
}

/// Whether the mode's shader runs at this ratio.
export function upscaleEngaged(mode: UpscaleMode, ratio: { x: number; y: number } | null): boolean {
  if (mode === 'off' || !ratio) return false;
  const { threshold } = UPSCALE_SHADERS[mode];
  return ratio.x > threshold && ratio.y > threshold;
}

/// What a file plays with: its own choice, else its playlist's, else nothing
/// — and with nothing stored, the panel speaks about this file.
export function resolveUpscale(
  file: UpscaleMode | null,
  folder: UpscaleMode | null,
): { mode: UpscaleMode; scope: 'file' | 'folder'; stored: boolean } {
  if (file) return { mode: file, scope: 'file', stored: true };
  if (folder) return { mode: folder, scope: 'folder', stored: true };
  return { mode: 'off', scope: 'file', stored: false };
}

/**
 * What the file's own record should hold for `mode`. 'off' is a choice worth
 * keeping only when the playlist would otherwise turn the shader on — the one
 * episode of a season that is already sharp enough; anywhere else it is no
 * record at all.
 */
export function fileRecord(mode: UpscaleMode, folder: UpscaleMode | null): UpscaleMode | null {
  return mode === 'off' && (folder ?? 'off') === 'off' ? null : mode;
}

/// The next mode for the hotkey, wrapping round to off.
export function nextUpscale(mode: UpscaleMode): UpscaleMode {
  return UPSCALE_MODES[(UPSCALE_MODES.indexOf(mode) + 1) % UPSCALE_MODES.length];
}
