/**
 * The fullscreen state the page asked for last, until the window has it.
 *
 * A fullscreen change is not one step: the page puts its veil up first (two
 * frames and a little more), and only then flips its mirror and asks the
 * window. A second press inside that wait used to be decided against the
 * mirror, which still showed the state before the first press — so a quick
 * in-out-in-out went out as in, out, in, in. And the mirror is also rewritten
 * from the window's own answer on every resize (`boot.ts`), which during a
 * transition is the state on screen, not the one asked for.
 *
 * So each request sets `wanted` before anything waits, a toggle is decided
 * against it, and a request overtaken by a later one while it waits sends
 * nothing: only the last one reaches the window. While anything is pending the
 * resize handler leaves the mirror alone (`pending`). A host that runs its own
 * queue reports the state it is heading to as its fullscreen state, so the
 * mirror it gives back afterwards agrees (`windowTransition.queued`).
 *
 * A leaf with its I/O handed in, so the sequence is testable without a window.
 */
export interface FullscreenIo {
  /** The page's mirror (`chrome.fullscreen`). */
  mirror(): boolean;
  setMirror(on: boolean): void;
  /** What has to happen before the window is asked (the veil); `null` when nothing does. */
  prepare(): Promise<void> | null;
  /** Ask the window (or the host) for the state. */
  send(on: boolean): Promise<unknown>;
}

export class FullscreenRequests {
  wanted: boolean | null = null;
  private seq = 0;

  constructor(private io: FullscreenIo) {}

  /** What a toggle is decided against: the last request while one is pending, the mirror otherwise. */
  current(): boolean {
    return this.wanted ?? this.io.mirror();
  }

  /** A request has not reached the window yet, or the window has not answered it. */
  pending(): boolean {
    return this.wanted !== null;
  }

  async request(on: boolean): Promise<void> {
    const seq = ++this.seq;
    this.wanted = on;
    try {
      const waiting = this.io.prepare();
      if (waiting) await waiting;
      // A later request owns the outcome: it sends its own state after its own wait.
      if (seq !== this.seq) return;
      this.io.setMirror(on);
      await this.io.send(on);
    } finally {
      if (seq === this.seq) this.wanted = null;
    }
  }
}
