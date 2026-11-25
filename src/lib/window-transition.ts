/**
 * How the interface behaves around a fullscreen change, as the host decides it.
 *
 * Off by default, and nothing in this app turns any of it on: the transition goes
 * through its own black (the veil), and the bars stay as they were. A host that
 * moves the picture through the transition itself turns `hushChrome` on: the
 * bars go before the transition starts and stay away afterwards until the
 * viewer does something that raises them — the state they are in after idling
 * away, whatever the "hide the controls" setting says (`chrome.hushed`).
 *
 * A leaf: `chrome.svelte.ts` reads it and a host's own code writes it, so it
 * imports nothing — a host's module that imported the chrome instead would be
 * a cycle through whatever the chrome imports from it.
 */
export const windowTransition: {
  hushChrome: boolean;
  /// How long after the transition starts a pointer report is not taken for
  /// activity: the window changing under a resting pointer reports a move.
  graceMs: number;
  /// Called before the bars are sent away: the host's chance to take the
  /// interface off at once rather than over the bars' fade.
  begin: (() => void) | null;
  /// The host owns the fullscreen transition: it runs one at a time and,
  /// when more requests come meanwhile, goes on to the last one (an even
  /// number cancels out); while anything runs or waits, `isFullscreen()`
  /// answers the state it is heading to, the window's own state otherwise. The
  /// page then flips its state and asks at once, without its own veil, so that
  /// a second press during a transition is decided against the state just
  /// asked for rather than the one still on screen.
  queued: boolean;
  /// Fullscreen requests go through the page's request queue
  /// (fullscreen-request.ts): each press is decided against the state asked
  /// for last, and only the last of several quick ones reaches the window. Off
  /// in this app, whose toggles keep their direct path.
  requests: boolean;
} = { hushChrome: false, graceMs: 0, begin: null, queued: false, requests: false };
