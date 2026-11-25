// Skathi loads an `@font-face` split by `unicode-range` only for the subset that covers Latin: the
// player's Rubik comes as four subsets of one family (`src/app.css`), and the Cyrillic one stayed
// `unloaded` while Russian text was on screen, drawn in a fallback face (measured at Skathi a88f6683:
// `document.fonts.check('12px Rubik', 'Русский')` false, "Русский" 63 device px wide against
// Chromium's 71). A browser loads a subset when text needs it. Loading every declared face once,
// after the page's stylesheets are in, gives the same faces as WebView2 (measured: 71 px, offsets
// within 1 px). Four small files; nothing is fetched from outside the payload.
//
// Only the player's own font: `app.css` also declares "Han Punctuation" from the system's Chinese
// face (`local()`), and an explicit load turned those faces to `error` here (measured), where they
// had been left `unloaded` for Skathi's own matching.
export function loadDeclaredFonts(doc: Document = document, family = 'Rubik'): void {
  const fonts = (doc as Document & { fonts?: FontFaceSet }).fonts;
  if (!fonts) return;
  fonts.forEach((face) => {
    if (face.status === 'unloaded' && face.family.replace(/["']/g, '') === family) face.load().catch(() => {});
  });
}
