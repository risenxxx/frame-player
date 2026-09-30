/**
 * The click that brings the window forward is not a command.
 *
 * Pure, so that it can be tested: both ways of getting it wrong are silent. A
 * gate that lets the activating click through pauses the film every time the
 * viewer comes back to the player; one that swallows too much reads as a click
 * that did nothing. The story behind it — why the window accepts that click at
 * all, and why the page cannot rely on the order it hears about the click and
 * the activation in — is with the caller in `input.svelte.ts`.
 */
export class ActivationGate {
  private activatedAt = -Infinity;

  /** `enabled` is the platform: only macOS withholds the activating click. */
  constructor(private readonly enabled: boolean) {}

  /** The window became active — the `focus` event, however it was caused. */
  activated(now: number): void {
    this.activatedAt = now;
  }

  /**
   * Whether a click at `now` is the one that activated the window, or its
   * partner in a double click on it. `hasFocus` is `document.hasFocus()` at
   * the click: false is the activating click arriving before the page has
   * been told it is focused, which is the only time it is false. `interval` is
   * the system's double-click interval.
   */
  swallows(now: number, hasFocus: boolean, interval: number): boolean {
    if (!this.enabled) return false;
    if (!hasFocus) this.activatedAt = now;
    return now - this.activatedAt < interval;
  }
}
