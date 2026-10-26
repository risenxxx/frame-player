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

import { LOCALE_META, type Locale } from './i18n/locales.ts'

/**
 * The marker the worker fills in. Its value is the label in the page's
 * language with `{n}` where the number goes (`download.installs` in the
 * dictionary), so the worker writes the number without knowing the words;
 * the page renders `INSTALLS_BASE` into it.
 */
export const INSTALLS_ATTR = 'data-installs'

/** Rounded down, so the number on the page is never more than was counted;
    grouped the way the page's language groups digits. */
export function installsCount(count: number, lang: Locale): string {
  const step = count < 10_000 ? 10 : 100
  const shown = Math.floor(count / step) * step
  return shown.toLocaleString(lang === 'en' ? 'en-US' : LOCALE_META[lang].intl)
}

/** The label: the template from the marker, with the number in it. */
export const installsLabel = (template: string, count: number, lang: Locale): string =>
  template.replace('{n}', installsCount(count, lang))
