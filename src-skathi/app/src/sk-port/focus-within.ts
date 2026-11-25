// `:focus-within` as a class. Skathi matches the pseudo-class (`matches(':focus-within')` is true
// while the field has focus) but does not paint a rule written with it: in a minimal page the ring
// never appeared, and on the playback tab's "other" field (`.step-custom:focus-within`, the one
// such rule in `src/`) it was drawn at rest and gone on focus (measured at Skathi a88f6683). A class
// changed from `focusin`/`focusout` does repaint (measured), so the focused element's ancestors
// carry `sk-focus-within` and `Overrides.svelte` restates the rule on it.
export const FOCUS_WITHIN = 'sk-focus-within';

export function installFocusWithin(doc: Document = document): void {
  let marked: Element[] = [];
  const clear = () => {
    for (const el of marked) el.classList.remove(FOCUS_WITHIN);
    marked = [];
  };
  doc.addEventListener('focusin', (e) => {
    clear();
    for (let el = e.target as Element | null; el && el !== doc.documentElement; el = el.parentElement) {
      el.classList.add(FOCUS_WITHIN);
      marked.push(el);
    }
  }, true);
  doc.addEventListener('focusout', (e) => {
    // Focus moving to another element sends its own focusin, which re-marks.
    if (!(e as FocusEvent).relatedTarget) clear();
  }, true);
}
