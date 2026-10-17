/**
 * The installation count in the hero, shared by the page, the site's worker and
 * the counter behind it (`installs/`).
 *
 * The player reports its first launch once (`src-tauri/src/install_count.rs`);
 * the counter adds those to `INSTALLS_BASE`. The base stands for every
 * installation made before the player could report, and those installations
 * never report — they recognise themselves and stay quiet. It was set by hand
 * from the download numbers the day counting began, which is the one thing
 * here that is an estimate rather than a count.
 */
export const INSTALLS_BASE = 870

/** The marker the worker fills in; the page renders `INSTALLS_BASE` into it. */
export const INSTALLS_ATTR = 'data-installs'

/** Rounded down, so the number on the page is never more than was counted. */
export function installsLabel(count: number): string {
  const step = count < 10_000 ? 10 : 100
  const shown = Math.floor(count / step) * step
  return `${shown.toLocaleString('en-US')}+ installs`
}
