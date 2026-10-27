// ---- Mode and cache preferences ---------------------------------------------
// localStorage like every player-owned preference: nothing here is an mpv
// option, and the settings tab reads/writes through these helpers.

const MODE_KEY = 'frameplayer.castMode';
const CAP_KEY = 'frameplayer.castCacheCap';
const CAP_CHOICES = [0, 5, 20, 50];

export type CastMode = 'auto' | 'prepare' | 'hls';

/**
 * How a file reaches a **Chromecast**. DLNA is chosen per device in the picker,
 * because that choice depends on the device; this one is now internal.
 *
 * **HLS is no longer offered as a setting**, and that is a conclusion rather
 * than a simplification. It was built to stream an incomplete torrent, and both
 * halves of that job went elsewhere — DLNA carries the release untouched and a
 * direct-play file streams over Cast as it is — while measurement showed HLS
 * itself carries only H.264 with stereo on this receiver. So as a *choice* it
 * was strictly worse on everything it could carry and unable to carry the rest:
 * a menu entry whose every selection makes the result worse. What survives is
 * the code path, for the one case neither of the others reaches (a receiver
 * with no DLNA, an incomplete torrent that would need repacking) — a decision
 * the player makes from facts, not a question for the viewer — and as a
 * debugging knob: `localStorage.setItem('frameplayer.castMode', 'hls')` still
 * forces it, which is how the receiver's HLS behavior gets tested.
 */
export function castMode(): CastMode {
  const stored = localStorage.getItem(MODE_KEY);
  return stored === 'hls' || stored === 'prepare' ? stored : 'auto';
}

export function setCastMode(mode: CastMode) {
  try {
    localStorage.setItem(MODE_KEY, mode);
  } catch {
    // not critical: the choice simply will not survive a restart
  }
}

/// Cap in whole gigabytes; 0 means "keep nothing past the session".
export function castCacheCapGb(): number {
  const value = Number(localStorage.getItem(CAP_KEY) ?? '20');
  return CAP_CHOICES.includes(value) ? value : 20;
}

export function setCastCacheCapGb(gb: number) {
  try {
    localStorage.setItem(CAP_KEY, String(gb));
  } catch {
    // not critical
  }
}
