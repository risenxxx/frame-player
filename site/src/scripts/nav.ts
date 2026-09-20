/**
 * The header's backdrop, and the one control that copies a line of text.
 *
 * Whether the page has left the top is answered by an observer watching a 1px
 * marker at the very top of the document, not by a scroll listener: the header
 * blurs what is behind it, and running script on every scroll event next to a
 * `backdrop-filter` is the combination that makes a page feel heavy.
 */
export function initNav(): void {
  const nav = document.querySelector<HTMLElement>('[data-nav]')
  const top = document.querySelector<HTMLElement>('[data-nav-top]')

  if (nav && top && 'IntersectionObserver' in window) {
    new IntersectionObserver(
      ([entry]) => nav.classList.toggle('scrolled', !entry?.isIntersecting),
      { threshold: 0 },
    ).observe(top)
  }

  for (const button of document.querySelectorAll<HTMLButtonElement>('[data-copy]')) {
    const label = button.querySelector('.lbl')
    button.addEventListener('click', async () => {
      const text = button.dataset.copy
      if (!text) return
      try {
        await navigator.clipboard.writeText(text)
      } catch {
        return
      }
      button.classList.add('done')
      if (label) label.textContent = 'Copied'
      setTimeout(() => {
        button.classList.remove('done')
        if (label) label.textContent = 'Copy'
      }, 1600)
    })
  }
}
