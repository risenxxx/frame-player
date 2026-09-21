/**
 * Two behaviours, two observers and one loop.
 *
 * - `[data-reveal]` gets `.in` when it first crosses into view, and CSS does the
 *   rest with a transition and a `--dl` stagger.
 * - `[data-scene]` gets `--p`, a number from 0 to 1, animated once over
 *   `data-dur` milliseconds. Every mock reads it for whatever moves, so a scene
 *   is written in CSS and this file never touches a mock's markup. `--p` is
 *   eased out unless the scene asks for `data-ease="linear"` — a scene that
 *   cuts its own phases out of `--p` and gives each its own curve needs time,
 *   not a curve: sliced from an eased `--p`, a phase that owns the first third
 *   plays in the first eighth of the time.
 *
 * The two want opposite triggers, and running them off one observer was a bug
 * that only showed on the tallest section. A scene must not play while the
 * reader is still on the section before it, so it waits until it is properly in
 * view — margins cutting the viewport, a third of the element inside. A reveal
 * must be finished by the time the reader gets there, so it fires as the
 * element appears. Measured on the 15-cell grid, which is 1056px tall against
 * an 820px viewport: under the scene's own trigger it revealed with its top at
 * 144px, i.e. after it had filled the screen — a blank page scrolled into, then
 * a fade. Under its own, with a 400px lead below the fold, every section on
 * the page reveals with its top between 1165 and 1220px — off screen, and at
 * full strength by the time it arrives.
 *
 * The rAF loop runs only while a scene is in flight and stops itself afterwards.
 */
type Scene = { el: HTMLElement; start: number; dur: number; ease: (t: number) => number }

/** Cubic ease-out: fast to start, settles rather than arrives. */
const easeOut = (t: number): number => 1 - (1 - t) ** 3

export function initMotion(): void {
  const scenes = [...document.querySelectorAll<HTMLElement>('[data-scene]')]
  const reveals = [...document.querySelectorAll<HTMLElement>('[data-reveal]')]

  if (matchMedia('(prefers-reduced-motion: reduce)').matches || !('IntersectionObserver' in window)) {
    for (const el of reveals) el.classList.add('in')
    return
  }

  /*
    Only now is anything allowed to be hidden. The rule that holds a section at
    zero opacity keys on this class rather than on `html.js`, so the thing that
    hides the page and the thing that brings it back are the same statement: a
    module that fails to load, throws on its first line, or bails out above
    cannot leave a reader with a blank page it was going to reveal.

    What it does not cover is an observer that is created and never delivers —
    there the page below the first screen stays hidden. No browser a reader uses
    behaves that way; some headless renderers do.
  */
  document.documentElement.classList.add('reveal')

  const running: Scene[] = []
  let frame = 0

  const tick = (now: number): void => {
    frame = 0
    for (let i = running.length - 1; i >= 0; i--) {
      const scene = running[i]
      if (!scene) continue
      const t = Math.min(1, (now - scene.start) / scene.dur)
      scene.el.style.setProperty('--p', scene.ease(t).toFixed(4))
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
    running.push({
      el,
      start: performance.now(),
      dur: Number(el.dataset.dur) || 1400,
      ease: el.dataset.ease === 'linear' ? (t) => t : easeOut,
    })
    if (frame === 0) frame = requestAnimationFrame(tick)
  }

  const watch = (els: HTMLElement[], options: IntersectionObserverInit): IntersectionObserver => {
    const io = new IntersectionObserver((entries) => {
      for (const e of entries) {
        if (!e.isIntersecting) continue
        io.unobserve(e.target)
        play(e.target as HTMLElement)
      }
    }, options)
    for (const el of els) io.observe(el)
    return io
  }

  /*
    `threshold: 0` rather than a fraction, because a fraction is a question
    about the element and this one is about the viewport: a section taller than
    the screen can never have a third of itself inside a band smaller than the
    screen without first covering it.

    The margin reaches 400px *below* the fold, so the fade starts before the
    section does and is over by the time it is read. That is a lead rather than
    a fix: a 0.7s fade at a hurried 2000px/s covers 1400px, and no margin short
    of revealing the whole page wins that race — 400 is what an ordinary reading
    scroll (~500px/s) needs to arrive at a section already at full strength.
  */
  const revealIo = watch(reveals, { rootMargin: '0px 0px 400px 0px', threshold: 0 })
  const sceneIo = watch(scenes, { rootMargin: '-10% 0px -22% 0px', threshold: 0.32 })

  /*
    Nothing on screen may wait on an observer that never fires — a thumbnail
    capture, a print, a browser that throttles the callback. Whatever is visible
    three seconds in plays where it stands; everything below keeps waiting for
    the reader, which is the point of the margins above.

    Deliberately only what is on screen. A renderer that creates an observer and
    never delivers a callback leaves the rest of the page hidden, and that is
    accepted rather than rescued: the whole-page version of this timeout dropped
    every section in at once, which is a worse thing to do to a reader than to a
    renderer nobody is reading on.
  */
  setTimeout(() => {
    const h = innerHeight
    for (const el of [...reveals, ...scenes]) {
      if (played.has(el)) continue
      const r = el.getBoundingClientRect()
      if (r.bottom <= 0 || r.top >= h) continue
      revealIo.unobserve(el)
      sceneIo.unobserve(el)
      play(el)
    }
  }, 3000)
}
