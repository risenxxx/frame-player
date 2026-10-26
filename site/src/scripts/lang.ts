/**
 * The language switch remembers a choice, and the hint offers a language once.
 *
 * Nothing here redirects. A reader whose browser asks for Russian and who lands
 * on an English page — from a search result, from a link somebody sent — is
 * shown one line, in Russian, saying the page is there too, and stays where
 * they are. A redirect by `Accept-Language` would also send a search engine's
 * crawler, which asks in English, away from every other language, and leave a
 * reader who wanted the English page no way to keep it.
 *
 * A choice is anything the reader did about it: following the switch, or
 * closing the hint. After that the hint is not shown again. localStorage can
 * throw (private mode, blocked site data); then the hint comes back next visit.
 */
export const KEY = 'fp-lang'

function read(): string | null {
  try {
    return localStorage.getItem(KEY)
  } catch {
    return null
  }
}

function write(lang: string): void {
  try {
    localStorage.setItem(KEY, lang)
  } catch {
    // Without storage the hint comes back on the next visit, which is acceptable.
  }
}

/** The first language of the browser's list that the page offers, as `en`/`ru`/`zh`. */
function preferred(offered: string[]): string | null {
  for (const tag of navigator.languages ?? [navigator.language]) {
    const primary = tag.toLowerCase().split('-')[0] ?? ''
    if (offered.includes(primary)) return primary
  }
  return null
}

export function initLang(): void {
  const current = document.documentElement.dataset.lang ?? 'en'

  for (const link of document.querySelectorAll<HTMLAnchorElement>('[data-lang-set]')) {
    link.addEventListener('click', () => write(link.dataset.langSet ?? current))
  }

  const hints = [...document.querySelectorAll<HTMLElement>('[data-lang-hint]')]
  if (hints.length === 0 || read() !== null) return

  const want = preferred([current, ...hints.map((h) => h.dataset.langHint ?? '')])
  const hint = hints.find((h) => h.dataset.langHint === want)
  if (!hint) return

  hint.hidden = false
  hint.querySelector('[data-lang-dismiss]')?.addEventListener('click', () => {
    write(current)
    hint.hidden = true
  })
  hint.querySelector('a')?.addEventListener('click', () => write(want ?? current))
}
