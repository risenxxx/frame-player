/**
 * Wide tables in a guide scroll sideways, and this is what says so.
 *
 * Overflow already scrolls them — what overflow does not do is admit there is
 * more: a comparison cut off at its third column reads as a comparison of
 * three. So each `[data-pan-frame]` gets a fade on whichever side still has
 * table under it, a thin bar showing which slice is on screen, and dragging for
 * a mouse, which has no swipe. The same arrangement as the reference figure on
 * risen.dev, for the same reason.
 *
 * It runs whatever the reduced-motion setting says: none of it is motion.
 * Without it the table still scrolls with the browser's own bar, and the CSS
 * keeps that bar until `measured` says this script is in charge.
 */
export function initPan(): void {
  for (const scroller of document.querySelectorAll<HTMLElement>('[data-pan]')) {
    const frame = scroller.closest<HTMLElement>('[data-pan-frame]')
    if (!frame) continue
    const thumb = frame.querySelector<HTMLElement>('[data-pan-bar]')

    const update = (): void => {
      const slack = scroller.scrollWidth - scroller.clientWidth
      frame.classList.add('measured')
      frame.classList.toggle('pannable', slack > 1)
      frame.classList.toggle('at-start', scroller.scrollLeft <= 1)
      frame.classList.toggle('at-end', slack - scroller.scrollLeft <= 1)
      if (!thumb) return
      // The thumb is the share of the track that the visible slice is of the
      // table, and it travels whatever width that leaves.
      const share = scroller.clientWidth / scroller.scrollWidth
      const done = slack > 0 ? scroller.scrollLeft / slack : 0
      thumb.style.setProperty('--w', `${share * 100}%`)
      thumb.style.setProperty('--x', `${done * (1 - share) * 100}%`)
    }

    update()
    scroller.addEventListener('scroll', update, { passive: true })
    addEventListener('resize', update)

    // Touch pans the scroller on its own; a mouse has to be handed the grab.
    // Not from a link, which a press on is a click.
    let from = -1
    let at = 0

    scroller.addEventListener('pointerdown', (e) => {
      if (e.pointerType !== 'mouse' || e.button !== 0 || !frame.classList.contains('pannable')) return
      if ((e.target as Element).closest('a')) return
      from = e.clientX
      at = scroller.scrollLeft
      scroller.setPointerCapture(e.pointerId)
      frame.classList.add('dragging')
    })

    scroller.addEventListener('pointermove', (e) => {
      if (from < 0) return
      e.preventDefault()
      scroller.scrollLeft = at - (e.clientX - from)
    })

    const drop = (): void => {
      from = -1
      frame.classList.remove('dragging')
    }

    scroller.addEventListener('pointerup', drop)
    scroller.addEventListener('pointercancel', drop)
  }
}
