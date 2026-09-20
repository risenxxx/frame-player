/**
 * The theme switch: system | light | dark.
 *
 * The choice is stored under the key the inline script in the document head
 * reads before the first paint; this module only keeps the buttons in step and
 * writes the new choice. localStorage can throw (private mode, blocked site
 * data) — then the choice simply lives until the page is reloaded.
 *
 * There are two switches on the page, the header's and the footer's, and only
 * one state: applying a choice updates every group, not the one that was
 * clicked.
 */
const KEY = 'fp-theme'

type Choice = 'system' | 'light' | 'dark'

const isChoice = (v: string | undefined): v is Choice =>
  v === 'system' || v === 'light' || v === 'dark'

function read(): string | null {
  try {
    return localStorage.getItem(KEY)
  } catch {
    return null
  }
}

function write(choice: Choice): void {
  try {
    if (choice === 'system') localStorage.removeItem(KEY)
    else localStorage.setItem(KEY, choice)
  } catch {
    // Without storage the choice lasts until reload, which is acceptable.
  }
}

export function initTheme(): void {
  const root = document.documentElement
  const buttons = [...document.querySelectorAll<HTMLButtonElement>('[data-theme-set]')]

  const apply = (choice: Choice): void => {
    if (choice === 'system') delete root.dataset.theme
    else root.dataset.theme = choice
    for (const b of buttons) b.setAttribute('aria-pressed', String(b.dataset.themeSet === choice))
  }

  const stored = read()
  apply(stored === 'light' || stored === 'dark' ? stored : 'system')

  for (const b of buttons) {
    b.addEventListener('click', () => {
      const choice = b.dataset.themeSet
      if (!isChoice(choice)) return
      write(choice)
      apply(choice)
    })
  }
}
