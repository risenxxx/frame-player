// Tauri doesn't have a Node.js server to do proper SSR
// so we use adapter-static with a fallback to index.html to put the site in SPA mode
// See: https://svelte.dev/docs/kit/single-page-apps
// See: https://v2.tauri.app/start/frontend/sveltekit/ for more info
export const ssr = false;

/// Long enough for a file read from the bundle, short enough that a font that
/// never comes costs a beat rather than a blank window.
const FONT_WAIT_MS = 1500;

/**
 * Nothing is rendered until Rubik is in, because WebKit does not take back a
 * width it measured against the fallback font. Measured in a dev build with the
 * font served 0.7 s late: after the swap "Открыть файл (O)" kept a box 0.9px
 * short of its own text and broke onto two lines, and the caption in the
 * title bar's corner did the same. Each such box is sized by its content — a
 * flex item, an absolute box with one inset — and nothing ever resized it again.
 * With the font loaded before the first layout there is no swap to get wrong.
 *
 * The two subsets the interface draws with (`unicode-range` loads a face only
 * when text asks for it, so naming the characters is what loads it). The face
 * is variable, so one weight brings all of them.
 */
export async function load() {
  if (typeof document === 'undefined' || !document.fonts) return;
  await Promise.race([
    Promise.all([document.fonts.load('16px Rubik', 'Aa'), document.fonts.load('16px Rubik', 'Яя')]).catch(() => {}),
    new Promise((resolve) => setTimeout(resolve, FONT_WAIT_MS)),
  ]);
}
