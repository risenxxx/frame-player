/**
 * Locale-aware unit formatting: sizes, bitrates, transfer rates, frame rates.
 *
 * Separate from `format.ts`, which is documented as pure and importless — these
 * go through `t()`, so they read reactive state and a component calling one
 * re-renders on a language switch. That is the point: units are labeled at
 * render time rather than baked into the value, so switching the language
 * relabels a panel without waiting for its next refresh.
 */

import { t } from '$lib/i18n.svelte';

/** Bytes as GB above a gigabyte, MB below it. */
export function fmtSize(bytes: number): string {
  const gb = bytes / 1024 ** 3;
  return gb >= 1
    ? t('info.gb', { value: gb.toFixed(2) })
    : t('info.mb', { value: Math.round(bytes / 1024 ** 2) });
}

/** A media bitrate, in bits — Mbps above a megabit, kbps below. */
export function fmtRate(bitsPerSecond: number): string {
  return bitsPerSecond >= 1_000_000
    ? t('info.mbps', { value: (bitsPerSecond / 1_000_000).toFixed(1) })
    : t('info.kbps', { value: Math.round(bitsPerSecond / 1000) });
}

/**
 * A download rate, in bytes — the unit a torrent client is read in, and
 * deliberately not the bits `fmtRate` uses for a video bitrate. The two are
 * answering different questions and an eightfold difference between them would
 * be read as one of the figures being wrong.
 */
export function fmtSpeed(bytesPerSecond: number): string {
  return bytesPerSecond >= 1024 ** 2
    ? t('info.mbs', { value: (bytesPerSecond / 1024 ** 2).toFixed(1) })
    : t('info.kbs', { value: Math.round(bytesPerSecond / 1024) });
}

/** 23.976 must survive; 25 must not become "25.000". */
export function fmtFps(fps: number): string {
  const rounded = Math.round(fps * 1000) / 1000;
  return t('info.fps', { value: Number.isInteger(rounded) ? rounded : rounded.toFixed(3) });
}
