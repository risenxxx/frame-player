/// Small DOM predicates shared between the page and the dialog shell.
///
/// These live outside a component because both sides need the same answer and
/// a second copy is the kind that drifts: `blockContextMenu` is what every
/// dialog backdrop hands to `oncontextmenu`, while the page's own
/// `onContextMenu` has to make the identical text-field exception.

/// The system's own context menu is the only way to copy or paste in a text
/// field once the app has replaced the menu bar's Copy item (see the macOS menu
/// notes in CLAUDE.md), so the player must never take that offer away.
/// Suppressing it there is how the link field ended up with no way to paste at
/// all.
export function inTextField(e: Event): boolean {
  const target = e.target as HTMLElement | null;
  return !!target?.closest('input, textarea, [contenteditable="true"]');
}

/// Dialog backdrops swallow the context menu so the player's does not open
/// behind them — but not over a text field, for the reason above.
export function blockContextMenu(e: MouseEvent) {
  if (inTextField(e)) return;
  e.preventDefault();
  e.stopPropagation();
}

/**
 * Follow a slider drag from its `pointerdown` to its release, wherever the
 * pointer goes in between.
 *
 * Two things a drag on a floating surface has to be defended against, both
 * found on the picture sliders. The release lands wherever the pointer is, and
 * a release outside the surface produces a `click` there — on the video or a
 * dialog's backdrop, which reads it as "dismiss". And the input keeps focus
 * afterwards, and a focused input is a text field to the hotkeys
 * (`inTextField`), so Space and the arrows would stop working until something
 * else was clicked.
 */
export function holdSlider(e: PointerEvent, onEnd: () => void) {
  const input = e.currentTarget as HTMLInputElement;
  const surface = input.closest('[role="menu"], [role="dialog"]');
  const end = (up: PointerEvent) => {
    window.removeEventListener('pointerup', end, true);
    window.removeEventListener('pointercancel', end, true);
    onEnd();
    input.blur();
    if (surface && !surface.contains(up.target as Node)) {
      const swallow = (c: Event) => c.stopPropagation();
      window.addEventListener('click', swallow, { capture: true, once: true });
      // The click, when there is one, is dispatched in the same task as the
      // release; anything later is a click of its own.
      setTimeout(() => window.removeEventListener('click', swallow, true), 0);
    }
  };
  window.addEventListener('pointerup', end, true);
  window.addEventListener('pointercancel', end, true);
}
