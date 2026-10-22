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
/// 100 rather than the 150 first shipped: a 4K web release with two-second
/// groups steps back in 130–220 ms in software, which reads as slow beside the
/// 42 ms of a step forward, and its backward entry costs only 0.65 s. A film
/// with two-second groups at 1080p steps in ~60 ms and stays native.
export const SLOW_BACKSTEP_MS = 100;

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

/// Whether a session should take the decoder to software: whenever it is in
/// hardware and has reported a picture. There used to be a 1440p ceiling, set
/// on a synthetic 4K file where software won by 15 %; on a real 4K HEVC web
/// release it steps back in 130–220 ms against 230–410 ms in hardware, and the
/// other reason for the ceiling — playing on in software — went once our play
/// controls began giving the decoder back before they unpause.
export function swapsDecoder(hwdecCurrent: string | null, w: number, h: number): boolean {
  if (!hwdecCurrent || hwdecCurrent === 'no') return false;
  return w > 0 && h > 0;
}

export interface ReverseInputs {
  /// The viewer's switch in Settings → Video.
  enabled: boolean;
  /// The first native step back on this file took longer than SLOW_BACKSTEP_MS.
  slow: boolean | null;
  /// Backward playback already misbehaved on this file.
  broken: boolean;
  /// The file is on this disk: a local file, or a torrent whose file has fully
  /// arrived. A stream would decode backward through the network, and a torrent
  /// still downloading has holes that read back as zeros.
  onDisk: boolean;
  /// Both A–B loop points are set: mpv loops in the playing direction, so
  /// backward it would wrap from A to B.
  abLoop: boolean;
  /// Several audio tracks are mixed through `lavfi-complex`: the graph owns the
  /// tracks (an `aid` write is ignored while it stands), so the track could not
  /// be switched off for the turn, and a graph decoded backward was never tried.
  audioGraph: boolean;
  /// mpv is decoding in software right now. Backward decoding in hardware keeps
  /// the frames in GPU memory that the buffer does not count.
  software: boolean;
  /// What the longest group around here costs, decoded, and what may be spent.
  need: number | null;
  budget: number;
}

export function shouldReverse(i: ReverseInputs): boolean {
  if (!i.enabled || i.slow !== true || i.broken || !i.onDisk || i.abLoop || i.audioGraph || !i.software) {
    return false;
  }
  return i.need !== null && i.need > 0 && i.need <= i.budget;
}

/// How far a stretch is widened before it is looked up in a torrent's map of
/// what has arrived. The map is of *bytes* and the stretch is of *time*, and a
/// variable-bitrate file makes the two disagree — the same reasoning and the
/// same margin as the preview gate (`positionBuffered` in torrent.svelte.ts).
export const BUFFER_MARGIN = 0.004;

/**
 * Whether one downloaded band covers the whole stretch `[from, to]`, both as
 * fractions of the file, widened by BUFFER_MARGIN at each end. One band, not a
 * union of several: the bands are already merged, so a stretch spanning two of
 * them spans a hole.
 */
export function stretchInBands(bands: [number, number][], from: number, to: number): boolean {
  const a = Math.max(0, from - BUFFER_MARGIN);
  const b = Math.min(1, to + BUFFER_MARGIN);
  return bands.some(([s, e]) => s <= a && b <= e);
}

/// One frame's duration, from whichever rate mpv knows. 1/24 when it knows none.
export function frameDuration(fps: number | null | undefined): number {
  return fps && fps > 1 && fps < 1000 ? 1 / fps : 1 / 24;
}

/// Did a step land where it should have? A buffer that overflowed lands
/// seconds off (measured: 39.9 → 33.5 on a 4K file, 39.9 → 35.4 on AV1), and a
/// step that skipped frames lands a few frames off (209 ms, five frames at
/// 23.976, with an audio track decoding backward beside it). Two and a half
/// frames, or 0.1 s, catches both and still lets a variable frame rate through.
export function landedRight(expected: number, landed: number | null, frame: number): boolean {
  if (landed === null || !Number.isFinite(landed)) return false;
  return Math.abs(landed - expected) <= Math.max(2.5 * frame, 0.1);
}
