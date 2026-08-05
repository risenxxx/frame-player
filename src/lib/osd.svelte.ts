/**
 * The on-screen popup.
 *
 * IINA-style: a "label: value" line, an optional bar under it and an optional
 * smaller second line. No icons — text labels give every action a similar
 * width, whereas an icon took space and said nothing about the point.
 *
 * Its own module because practically every other module raises one, and having
 * them all import the page component would be the dependency cycle to end them
 * all.
 */

export type OsdState = { text: string; sub?: string; progress?: number };

class Osd {
  current = $state<OsdState | null>(null);
  /**
   * Bumped by every popup. Asynchronous refinements (the actual position after
   * a keyframe seek) check it before writing, so they cannot overwrite a popup
   * that already outlived them.
   */
  seq = $state(0);
}

const state = new Osd();
let timer: ReturnType<typeof setTimeout> | undefined;

/** Current popup, or null. Read from markup. */
export function osd(): OsdState | null {
  return state.current;
}

/** Sequence number of the popup on screen right now. */
export function osdSeq(): number {
  return state.seq;
}

/**
 * Raise a popup. It clears itself after a moment — unless `sticky`, which is for
 * an operation whose length is unknown: a "saving…" that vanished halfway would
 * read as the operation having finished, and finishing is exactly what has not
 * happened. A sticky popup waits for the next `showOsd` to replace it, so the
 * caller must always follow it with one, in a `finally` if need be.
 */
export function showOsd(
  text: string,
  opts: { sub?: string; progress?: number; sticky?: boolean } = {},
) {
  state.current = { text, sub: opts.sub, progress: opts.progress };
  state.seq++;
  clearTimeout(timer);
  if (!opts.sticky) timer = setTimeout(() => (state.current = null), 1200);
}
