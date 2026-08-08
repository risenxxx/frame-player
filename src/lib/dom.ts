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
