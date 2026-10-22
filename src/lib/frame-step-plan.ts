/**
 * The arithmetic behind frame stepping, kept apart from mpv so it can be tested.
 *
 * Every number here was measured, and the measurements are in
 * docs/rules/mpv-playback.md (the frame-stepping line). The short form:
 *
 * - A native step back (`frame-back-step`) is an exact seek, so it costs a
 *   decode from the previous keyframe. Hardware decoding makes that **2–10×
 *   slower** than software, because VideoToolbox/D3D11 reinitialise and return
 *   frames with latency that a one-frame seek cannot hide. Hence the decoder is
 *   switched to software for the duration of a stepping session.
 * - mpv's backward playback decodes that group once and hands the frames out in
 *   reverse, so every press after the first is one frame period — but it holds
 *   the whole group, decoded, in memory, and when that does not fit its buffer
 *   it discards frames **silently** and the position lands seconds away.
 *   Hence the budget, and the landing check after every step.
 */

/// A native step back slower than this means the file is worth decoding
/// backward. Any step on the file can say so; one fast step never takes it back.
export const SLOW_BACKSTEP_MS = 150;

/// The decoder goes to software for a stepping session only up to this many
/// pixels. Above it software decoding is no longer the faster way back (4K HEVC
/// measured 1.85 s in hardware against 1.6 s in software) and playing on in
/// software afterwards is a real cost on an ordinary machine.
export const SWAP_MAX_PIXELS = 2560 * 1440;

const GiB = 1024 ** 3;

/// How much memory backward stepping may hold: a quarter of the machine, never
/// more than 4 GiB. A machine that will not say gets mpv's own default.
export function reversalBudget(ram: number): number {
  if (!(ram > 0)) return GiB;
  return Math.min(Math.floor(ram / 4), 4 * GiB);
}

export interface PictureFormat {
  /// Decoded size (`video-params/w`, `/h`) — not the display size.
  w: number;
  h: number;
  /// `video-params/average-bpp`: 12 for 8-bit 4:2:0, 24 for 10-bit 4:2:0.
  bpp: number;
  fps: number;
}

/// What holding one group of pictures costs: every frame of the longest group,
/// decoded. The tenth on top is the decoder's stride and line padding — measured
/// against mpv's resident size, a 4K 8-bit group of 240 frames came to 3.0 GB
/// where 3840 × 2160 × 1.5 × 240 says 2.99.
export function reversalNeed(format: PictureFormat, longestGopSecs: number): number {
  const frameBytes = (format.w * format.h * format.bpp) / 8;
  const frames = Math.ceil(longestGopSecs * format.fps) + 1;
  return frameBytes * frames * 1.1;
}

/// Whether a session should take the decoder to software (see SWAP_MAX_PIXELS).
export function swapsDecoder(hwdecCurrent: string | null, w: number, h: number): boolean {
  if (!hwdecCurrent || hwdecCurrent === 'no') return false;
  return w > 0 && h > 0 && w * h <= SWAP_MAX_PIXELS;
}

export interface ReverseInputs {
  /// The viewer's switch in Settings → Video.
  enabled: boolean;
  /// The first native step back on this file took longer than SLOW_BACKSTEP_MS.
  slow: boolean | null;
  /// Backward playback already misbehaved on this file.
  broken: boolean;
  /// A local file. A stream would decode backward through the network.
  local: boolean;
  /// Both A–B loop points are set: mpv loops in the playing direction, so
  /// backward it would wrap from A to B.
  abLoop: boolean;
  /// mpv is decoding in software right now. Backward decoding in hardware keeps
  /// the frames in GPU memory that the buffer does not count.
  software: boolean;
  /// What the longest group around here costs, decoded, and what may be spent.
  need: number | null;
  budget: number;
}

export function shouldReverse(i: ReverseInputs): boolean {
  if (!i.enabled || i.slow !== true || i.broken || !i.local || i.abLoop || !i.software) return false;
  return i.need !== null && i.need > 0 && i.need <= i.budget;
}

/// One frame's duration, from whichever rate mpv knows. 1/24 when it knows none.
export function frameDuration(fps: number | null | undefined): number {
  return fps && fps > 1 && fps < 1000 ? 1 / fps : 1 / 24;
}

/// Did a step land where it should have? A buffer that overflowed lands
/// seconds off (measured: 39.9 → 33.5 on a 4K file, 39.9 → 35.4 on AV1), so
/// the tolerance can be generous — three frames, or a quarter of a second —
/// and still never mistake a variable frame rate for an overflow.
export function landedRight(expected: number, landed: number | null, frame: number): boolean {
  if (landed === null || !Number.isFinite(landed)) return false;
  return Math.abs(landed - expected) <= Math.max(3 * frame, 0.25);
}
