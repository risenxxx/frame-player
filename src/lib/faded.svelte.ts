/// How long a surface takes to fade out: the 0.25 s the bars and the chips
/// fade over, and a little more, so the switch lands after the last frame of
/// the fade rather than on it.
const FADE_MS = 300;

/// `hidden()`, once the fade that follows it is over — for taking a surface
/// that has faded out off the page (`visibility: hidden`) rather than leaving
/// it transparent.
///
/// Transparent is not the same as not drawn. WebKit repaints a layer at zero
/// opacity exactly as it repaints a visible one, and the control bar's seekbar
/// is written on every video frame; measured on macOS, its GPU process held
/// ~200 MB for the whole of playback with the bars hidden, and 16 MB once they
/// were `visibility: hidden` — the same as with the film paused. Anything that
/// changes even once a second keeps it there (the torrent chip's rate does).
///
/// A timer rather than a delayed `visibility` transition, which was the first
/// attempt and is the shape that suggests itself. Measured: while the window
/// belongs to an app that is not frontmost, a `visibility` transition started
/// together with `opacity`/`transform` hangs at `currentTime` 0 and takes those
/// two with it, so the bars stayed up at full opacity with their `hidden`
/// class on. A timer does not depend on WebKit's animation timeline.
///
/// Call during a component's initialization: it owns an effect.
export function faded(hidden: () => boolean) {
  const state = $state({ on: false });
  $effect(() => {
    if (!hidden()) {
      state.on = false;
      return;
    }
    const timer = setTimeout(() => (state.on = true), FADE_MS);
    return () => clearTimeout(timer);
  });
  return state;
}
