/**
 * The languages a viewer can ask for in a file — ROADMAP 25.
 *
 * Every language choice in the player used to offer exactly two, and that was
 * never a decision about languages: it was **the UI translation set leaking
 * into content preferences**. The interface is localised in Russian and English
 * because there are two translations; what is being *watched* has nothing to do
 * with that, and anyone with Japanese audio and Polish subtitles was simply not
 * served.
 *
 * Three things this file is deliberately shaped by.
 *
 * **Each language is named in itself.** That is what every other player does,
 * and it is the only naming that works for the person looking for it: someone
 * hunting for Polish subtitles is looking for "Polski", not for "Польский".
 * It also keeps the list out of the `t()` dictionary, where 80 languages in two
 * translations would be 160 strings all saying the same thing — and where the
 * one entry a viewer actually needs would be the one nobody translated.
 *
 * **The 639-2 codes live here too, and they are the point.** A container tags
 * its tracks with whatever the muxer felt like: `rus`, `ru`, `ger` and `deu`
 * are all the same language, and mpv matches `--alang` entries against those
 * tags. So a chosen language expands to *all* of its spellings, adjacent, and
 * `LANG_ALIASES` — which the track-memory scoring in player.svelte.ts uses to
 * decide that the dub it remembered is the same one this episode calls
 * something else — is derived from the same rows rather than kept as a second
 * copy that drifts.
 *
 * **The list is finite and unapologetic about it.** These are the languages
 * OpenSubtitles serves and that rips are actually tagged with; a language not
 * here can still be typed into mpv.conf by hand, which is the escape hatch the
 * whole settings dialog is built around.
 */

export interface Language {
  /// ISO 639-1, the code mpv and OpenSubtitles both prefer.
  code: string;
  /// The language named in itself.
  name: string;
  /// ISO 639-2 spellings a container might carry. Both B and T variants where
  /// they differ (ger/deu, fre/fra) — a muxer picks one and there is no telling
  /// which.
  iso2?: string[];
}

export const LANGUAGES: Language[] = [
  { code: 'ar', name: 'العربية', iso2: ['ara'] },
  { code: 'az', name: 'Azərbaycan', iso2: ['aze'] },
  { code: 'be', name: 'Беларуская', iso2: ['bel'] },
  { code: 'bg', name: 'Български', iso2: ['bul'] },
  { code: 'bn', name: 'বাংলা', iso2: ['ben'] },
  { code: 'bs', name: 'Bosanski', iso2: ['bos'] },
  { code: 'ca', name: 'Català', iso2: ['cat'] },
  { code: 'cs', name: 'Čeština', iso2: ['ces', 'cze'] },
  { code: 'da', name: 'Dansk', iso2: ['dan'] },
  { code: 'de', name: 'Deutsch', iso2: ['deu', 'ger'] },
  { code: 'el', name: 'Ελληνικά', iso2: ['ell', 'gre'] },
  { code: 'en', name: 'English', iso2: ['eng'] },
  { code: 'eo', name: 'Esperanto', iso2: ['epo'] },
  { code: 'es', name: 'Español', iso2: ['spa'] },
  { code: 'et', name: 'Eesti', iso2: ['est'] },
  { code: 'eu', name: 'Euskara', iso2: ['eus', 'baq'] },
  { code: 'fa', name: 'فارسی', iso2: ['fas', 'per'] },
  { code: 'fi', name: 'Suomi', iso2: ['fin'] },
  { code: 'fr', name: 'Français', iso2: ['fra', 'fre'] },
  { code: 'gl', name: 'Galego', iso2: ['glg'] },
  { code: 'he', name: 'עברית', iso2: ['heb'] },
  { code: 'hi', name: 'हिन्दी', iso2: ['hin'] },
  { code: 'hr', name: 'Hrvatski', iso2: ['hrv'] },
  { code: 'hu', name: 'Magyar', iso2: ['hun'] },
  { code: 'hy', name: 'Հայերեն', iso2: ['hye', 'arm'] },
  { code: 'id', name: 'Bahasa Indonesia', iso2: ['ind'] },
  { code: 'is', name: 'Íslenska', iso2: ['isl', 'ice'] },
  { code: 'it', name: 'Italiano', iso2: ['ita'] },
  { code: 'ja', name: '日本語', iso2: ['jpn'] },
  { code: 'ka', name: 'ქართული', iso2: ['kat', 'geo'] },
  { code: 'kk', name: 'Қазақша', iso2: ['kaz'] },
  { code: 'km', name: 'ភាសាខ្មែរ', iso2: ['khm'] },
  { code: 'ko', name: '한국어', iso2: ['kor'] },
  { code: 'ky', name: 'Кыргызча', iso2: ['kir'] },
  { code: 'lt', name: 'Lietuvių', iso2: ['lit'] },
  { code: 'lv', name: 'Latviešu', iso2: ['lav'] },
  { code: 'mk', name: 'Македонски', iso2: ['mkd', 'mac'] },
  { code: 'ml', name: 'മലയാളം', iso2: ['mal'] },
  { code: 'ms', name: 'Bahasa Melayu', iso2: ['msa', 'may'] },
  { code: 'nl', name: 'Nederlands', iso2: ['nld', 'dut'] },
  { code: 'no', name: 'Norsk', iso2: ['nor', 'nob'] },
  { code: 'pl', name: 'Polski', iso2: ['pol'] },
  { code: 'pt', name: 'Português', iso2: ['por'] },
  { code: 'pt-br', name: 'Português (Brasil)', iso2: ['pob'] },
  { code: 'ro', name: 'Română', iso2: ['ron', 'rum'] },
  { code: 'ru', name: 'Русский', iso2: ['rus'] },
  { code: 'si', name: 'සිංහල', iso2: ['sin'] },
  { code: 'sk', name: 'Slovenčina', iso2: ['slk', 'slo'] },
  { code: 'sl', name: 'Slovenščina', iso2: ['slv'] },
  { code: 'sq', name: 'Shqip', iso2: ['sqi', 'alb'] },
  { code: 'sr', name: 'Српски', iso2: ['srp'] },
  { code: 'sv', name: 'Svenska', iso2: ['swe'] },
  { code: 'ta', name: 'தமிழ்', iso2: ['tam'] },
  { code: 'te', name: 'తెలుగు', iso2: ['tel'] },
  { code: 'th', name: 'ไทย', iso2: ['tha'] },
  { code: 'tl', name: 'Tagalog', iso2: ['tgl'] },
  { code: 'tr', name: 'Türkçe', iso2: ['tur'] },
  { code: 'uk', name: 'Українська', iso2: ['ukr'] },
  { code: 'ur', name: 'اردو', iso2: ['urd'] },
  { code: 'uz', name: 'Oʻzbekcha', iso2: ['uzb'] },
  { code: 'vi', name: 'Tiếng Việt', iso2: ['vie'] },
  { code: 'zh', name: '中文', iso2: ['zho', 'chi'] },
];

const BY_CODE = new Map(LANGUAGES.map((l) => [l.code, l]));

/**
 * ISO 639-2 → 639-1, derived from the table above rather than written twice.
 *
 * Used by the remembered-track scoring, where `jpn` and `ja` have to compare
 * equal — comparing by prefix would not do it, since they share one letter.
 */
export const LANG_ALIASES: Record<string, string> = Object.fromEntries(
  LANGUAGES.flatMap((l) => (l.iso2 ?? []).map((code) => [code, l.code])),
);

/// The language named in itself, or the raw tag when it is not one we know.
/// Never a placeholder: a tag we cannot name is still what the file says.
export function languageName(code: string): string {
  return BY_CODE.get(code.toLowerCase())?.name ?? code;
}

/**
 * A preference list as mpv wants it.
 *
 * Each language expands to every spelling it has, and the spellings stay
 * **adjacent**: `--alang` is an ordered priority list, so `ru,rus,en,eng` means
 * "Russian however it is tagged, else English however it is tagged", while
 * grouping by spelling instead would let an `eng` track beat a `rus` one.
 */
export function mpvLangValue(codes: string[]): string {
  return codes
    .flatMap((code) => {
      const lang = BY_CODE.get(code);
      return lang ? [lang.code, ...(lang.iso2 ?? [])] : [code];
    })
    .join(',');
}

/**
 * Read a stored `alang`/`slang` line back into a list of languages.
 *
 * Has to survive whatever is in the user's own mpv.conf, not only what this
 * dialog wrote: entries may be 639-2, may repeat, may be regional (`pt-BR`) and
 * may be a language this table has never heard of. Unknown codes are **kept**,
 * because the line is theirs and dropping an entry would silently change what
 * the player selects; they simply show up under their own tag.
 */
export function parseLangList(value: string | null): string[] {
  if (!value) return [];
  const out: string[] = [];
  for (const raw of value.split(',')) {
    const tag = raw.trim().toLowerCase();
    if (!tag) continue;
    const code = BY_CODE.has(tag) ? tag : (LANG_ALIASES[tag] ?? tag);
    if (!out.includes(code)) out.push(code);
  }
  return out;
}

/**
 * Languages matching what has been typed, best first.
 *
 * Matched on the endonym *and* on the codes, because both are things people
 * type: someone who knows the file is tagged `jpn` will type that rather than
 * hunt for 日本語 in a list they cannot read.
 *
 * Ranked rather than left in table order, because prefix-matching codes puts
 * near-misses above the obvious answer: measured, typing `ru` matched Romanian
 * first, through its own `rum` — alphabetically ahead of Russian and almost
 * certainly not what was meant. An exact code, then a name that starts with
 * what was typed, then everything else.
 */
export function searchLanguages(query: string, exclude: string[] = []): Language[] {
  const q = query.trim().toLowerCase();
  const rank = (l: Language): number => {
    if (l.code === q || (l.iso2 ?? []).includes(q)) return 0;
    if (l.name.toLowerCase().startsWith(q)) return 1;
    return 2;
  };
  return LANGUAGES.filter((l) => {
    if (exclude.includes(l.code)) return false;
    if (!q) return true;
    return (
      l.name.toLowerCase().includes(q) ||
      l.code.startsWith(q) ||
      (l.iso2 ?? []).some((c) => c.startsWith(q))
    );
  }).sort((a, b) => rank(a) - rank(b));
}
