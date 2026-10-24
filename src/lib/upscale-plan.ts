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
/// for drawn animation, or the generative one that draws detail in.
export type UpscaleMode = 'off' | 'video' | 'anime' | 'strong';

/// A mode that puts a shader in.
export type ShaderMode = Exclude<UpscaleMode, 'off'>;

export const UPSCALE_MODES: readonly UpscaleMode[] = ['off', 'video', 'anime', 'strong'];

/**
 * The shader for each mode and the factor its `//!WHEN` lines demand, copied
 * from the files in `src-tauri/shaders/`.
 *
 * All three are ×2 networks. Measured on an M3 Max into 3400×2000, the GPU time
 * per frame (`vo-passes`, the sum of the averages), for a 720p and a 1080p
 * source: 2.3 / 2.9 ms without a shader; ArtCNN C4F16 DS 7.2 / 8.5; Anime4K CNN
 * M 3.9 / 6.4; Anime4K GAN x2 M 9.9 / 20.7. The heavier members of each family
 * are not a switch: ArtCNN C4F32 is 37 ms at 1080p, Anime4K's Restore GAN UL +
 * GAN x2 M 44 ms and GAN x4 UL 39 ms — past a 24 fps frame on the fastest Mac
 * there is, and an integrated GPU has a fraction of its cores.
 *
 * `video` is the DS variant (trained to denoise and sharpen as it enlarges):
 * the same cost as plain C4F16 and visibly crisper. `strong` is the only one
 * that invents texture rather than reconstructing edges, which is what reads as
 * detail across the whole frame — and it pays for it twice: faces pick up
 * artifacts, since the network was trained on animation, and the invented
 * texture differs from frame to frame (two consecutive frames of a 480p film
 * were 35.4 dB apart in PSNR plain, 33.9 dB with it). Hence `STRONG_MAX`.
 */
export const UPSCALE_SHADERS: Record<ShaderMode, { file: string; threshold: number }> = {
  video: { file: 'ArtCNN_C4F16_DS.glsl', threshold: 1.3 },
  anime: { file: 'Anime4K_Upscale_CNN_x2_M.glsl', threshold: 1.2 },
  strong: { file: 'Anime4K_Upscale_GAN_x2_M.glsl', threshold: 1.2 },
};

/**
 * The largest source the generative mode runs on, by its long and its short
 * side. Its cost follows the source's pixels, 20.7 ms a frame at 1080p on the
 * fastest Mac, and a picture that large has the least to gain from invented
 * detail; above this the mode falls back to `video`, so that a playlist set to
 * it mixes 720p and 1080p episodes without either dropping frames or losing
 * the upscaler.
 */
export const STRONG_MAX = { long: 1280, short: 720 };

/**
 * The shader a mode puts in for this source, or null for none. A source not
 * measured yet (`w`/`h` zero) takes the mode as asked: they arrive from the
 * codec before the first frame, and a later report re-decides.
 */
export function shaderFor(mode: UpscaleMode, plane: { w: number; h: number }): ShaderMode | null {
  if (mode === 'off') return null;
  if (mode !== 'strong') return mode;
  const long = Math.max(plane.w, plane.h);
  const short = Math.min(plane.w, plane.h);
  return long > STRONG_MAX.long || short > STRONG_MAX.short ? 'video' : 'strong';
}

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

/// Whether a shader runs at this ratio.
export function upscaleEngaged(mode: ShaderMode | null, ratio: { x: number; y: number } | null): boolean {
  if (!mode || !ratio) return false;
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
