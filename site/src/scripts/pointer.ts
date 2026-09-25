/**
 * Where a scene's pointer stops, measured rather than written down.
 *
 * The cast scene's pointer is a child of the one button it walks to, so landing
 * on it is arithmetic. A pointer that clicks through several surfaces cannot be
 * a child of all of them: the subtitle scene goes from a control-bar button to
 * a menu row to a button inside a dialog centred on the window, and the offsets
 * between those depend on the window's width, the breakpoint and the font. So
 * each stop carries `data-stop="N"` (and optionally `data-fx`/`data-fy`, the
 * point inside it as a fraction, the centre by default), and this writes its
 * position relative to the `[data-pointer]` scene as `--xN`/`--yN`. CSS does
 * the walking between them off `--p`, exactly as it does everything else.
 *
 * Layout offsets, not bounding boxes: the surfaces are measured while they are
 * faded out and scaled, and `offsetLeft` ignores transforms — which is what is
 * wanted, since each one has settled at scale 1 by the time the pointer reaches
 * it. That is also why the dialog is centred with auto margins rather than a
 * translate.
 *
 * Without this the variables are unset and the pointer is not drawn at all
 * (`.pointed` in mocks.css), which is also the state the scene ends in.
 */
export function initPointer(): void {
  for (const scene of document.querySelectorAll<HTMLElement>('[data-pointer]')) {
    const stops = [...scene.querySelectorAll<HTMLElement>('[data-stop]')]

    const measure = (): void => {
      for (const stop of stops) {
        let x = stop.offsetWidth * Number(stop.dataset.fx ?? 0.5)
        let y = stop.offsetHeight * Number(stop.dataset.fy ?? 0.5)
        // offsetLeft is measured from the offset parent's padding edge, so each
        // parent's own border has to be added on the way up.
        for (let n: HTMLElement | null = stop; n && n !== scene; ) {
          x += n.offsetLeft
          y += n.offsetTop
          const parent = n.offsetParent as HTMLElement | null
          if (parent && parent !== scene) {
            x += parent.clientLeft
            y += parent.clientTop
          }
          n = parent
        }
        scene.style.setProperty(`--x${stop.dataset.stop}`, `${x}px`)
        scene.style.setProperty(`--y${stop.dataset.stop}`, `${y}px`)
      }
      scene.classList.add('pointed')
    }

    measure()
    new ResizeObserver(measure).observe(scene)
    void document.fonts?.ready.then(measure)
  }
}
