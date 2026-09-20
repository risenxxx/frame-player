/**
 * Two behaviours and one loop.
 *
 * - `[data-reveal]` gets `.in` when it first crosses into view, and CSS does the
 *   rest with a transition and a `--dl` stagger.
 * - `[data-scene]` gets `--p`, a number from 0 to 1, animated once over
 *   `data-dur` milliseconds. Every mock reads it for whatever moves, so a scene
 *   is written in CSS and this file never touches a mock's markup.
 *
 * A scene starts when it is properly in view rather than when its first pixel
 * is: the observer's margins cut the top and bottom of the viewport, so nothing
 * plays out while the reader is still on the section before it. The rAF loop
 * runs only while a scene is in flight and stops itself afterwards.
 */
type Scene = { el: HTMLElement; start: number; dur: number }

/** Cubic ease-out: fast to start, settles rather than arrives. */
const easeOut = (t: number): number => 1 - (1 - t) ** 3

export function initMotion(): void {
  const scenes = [...document.querySelectorAll<HTMLElement>('[data-scene]')]
  const reveals = [...document.querySelectorAll<HTMLElement>('[data-reveal]')]

  if (matchMedia('(prefers-reduced-motion: reduce)').matches || !('IntersectionObserver' in window)) {
    for (const el of reveals) el.classList.add('in')
    return
  }

  const running: Scene[] = []
  let frame = 0

  const tick = (now: number): void => {
    frame = 0
    for (let i = running.length - 1; i >= 0; i--) {
      const scene = running[i]
      if (!scene) continue
      const t = Math.min(1, (now - scene.start) / scene.dur)
      scene.el.style.setProperty('--p', easeOut(t).toFixed(4))
      if (t >= 1) running.splice(i, 1)
    }
    if (running.length > 0) frame = requestAnimationFrame(tick)
  }

  /* Parked at the beginning of their own animation, never at opacity 0: what a
     scene shows at `--p` 0 is the product before the viewer touched it. */
  for (const el of scenes) el.style.setProperty('--p', '0')

  const played = new Set<HTMLElement>()

  const play = (el: HTMLElement): void => {
    if (played.has(el)) return
    played.add(el)
    el.classList.add('in')
    if (!el.hasAttribute('data-scene')) return
    running.push({ el, start: performance.now(), dur: Number(el.dataset.dur) || 1400 })
    if (frame === 0) frame = requestAnimationFrame(tick)
  }

  const io = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        if (!e.isIntersecting) continue
        io.unobserve(e.target)
        play(e.target as HTMLElement)
      }
    },
    { rootMargin: '-10% 0px -22% 0px', threshold: 0.32 },
  )

  const watched = [...new Set([...scenes, ...reveals])]
  for (const el of watched) io.observe(el)

  /*
    Nothing above the fold may wait on an observer that never fires — a
    thumbnail capture, a print, a browser that throttles the callback. Whatever
    is on screen three seconds in plays where it stands; everything below keeps
    waiting for the reader, which is the point of the margins above.
  */
  setTimeout(() => {
    const h = innerHeight
    for (const el of watched) {
      if (played.has(el)) continue
      const r = el.getBoundingClientRect()
      if (r.bottom > 0 && r.top < h) {
        io.unobserve(el)
        play(el)
      }
    }
  }, 3000)
}
