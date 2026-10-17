/**
 * The cast mock's second walk: from the button, once the panel is open, to the
 * first device — whose row lights the moment the pointer is over it, as a menu
 * row does in the application (`.menu-item:hover`, no transition).
 *
 * Where that row is, seen from the button, depends on the layout (the panel is
 * placed against the window, the button sits in the control row, and both move
 * at every breakpoint), so it is measured rather than written down. Offsets
 * rather than bounding boxes: they ignore transforms, and the panel is still
 * sliding in when this runs.
 *
 * Everything lands in custom properties on the player that `mocks.css` reads:
 *
 *   --to-x, --to-y  where the hotspot ends, in the button's own coordinates
 *                   (the space `.walk` already moves in)
 *   --h-out         how far along the walk the hotspot leaves the button
 *   --h-in          …and enters the panel, the stretch between being video,
 *                   where the pointer is an arrow
 *   --row-in        …and enters the row, which is when it lights
 *   --in, --out     on each row it crosses on the way, the stretch it is
 *                   over that row — which lights for exactly that long, as a
 *                   row under a passing pointer does
 *
 * Without this the defaults in mocks.css keep the pointer on the button with
 * the row lit, which is the still the page had before.
 */

/** How far down the row the hotspot comes to rest; the hand hangs below its tip. */
const ON_ROW_Y = 0.32

/**
 * Where across the row the hotspot comes to rest: halfway between the end of
 * the longer of its two lines and the gear, so the hand covers neither the
 * device's name nor what it says about the file. Measured from rendered boxes
 * relative to the row's own, so the panel's slide cancels out, and scaled back
 * into layout pixels in case anything above the mock is scaled.
 */
function restX(row: HTMLElement): number {
  const box = row.getBoundingClientRect()
  const scale = box.width / row.offsetWidth || 1
  let textEnd = 0
  for (const line of row.querySelectorAll('.cast-name, .cast-model')) {
    const range = document.createRange()
    range.selectNodeContents(line)
    textEnd = Math.max(textEnd, range.getBoundingClientRect().right - box.left)
  }
  const gear = row.querySelector('svg, .spin')
  const gearStart = gear ? gear.getBoundingClientRect().left - box.left : box.width
  return (textEnd + gearStart) / 2 / scale
}

function offsetIn(el: HTMLElement, root: HTMLElement): { x: number; y: number } {
  let x = 0
  let y = 0
  let at: HTMLElement | null = el
  while (at && at !== root) {
    x += at.offsetLeft
    y += at.offsetTop
    at = at.offsetParent as HTMLElement | null
  }
  return { x, y }
}

type Box = { x: number; y: number; w: number; h: number }
const inside = (b: Box, x: number, y: number): boolean => x >= b.x && x <= b.x + b.w && y >= b.y && y <= b.y + b.h

export function initCastWalk(): void {
  for (const player of document.querySelectorAll<HTMLElement>('.sc-cast .player.mini')) walk(player)
}

function walk(player: HTMLElement): void {
  const button = player.querySelector<HTMLElement>('.b.cast')
  const panel = player.querySelector<HTMLElement>('.castpop')
  const row = panel?.querySelector<HTMLElement>('.castrow.pick')
  if (!button || !panel || !row) return
  const others = [...panel.querySelectorAll<HTMLElement>('.castrow:not(.pick)')]

  const measure = (): void => {
    const b = offsetIn(button, player)
    const p = offsetIn(panel, player)
    const r = offsetIn(row, player)
    const rel = (o: { x: number; y: number }, el: HTMLElement): Box => ({ x: o.x - b.x, y: o.y - b.y, w: el.offsetWidth, h: el.offsetHeight })
    const own: Box = { x: 0, y: 0, w: button.offsetWidth, h: button.offsetHeight }
    const panelBox = rel(p, panel)
    const rowBox = rel(r, row)

    // The first walk ends on the button's centre; `.walk` in mocks.css.
    const from = { x: own.w / 2, y: own.h / 2 }
    const to = { x: rowBox.x + restX(row), y: rowBox.y + rowBox.h * ON_ROW_Y }
    const at = (t: number): { x: number; y: number } => ({ x: from.x + (to.x - from.x) * t, y: from.y + (to.y - from.y) * t })

    let out = 1
    let into = 1
    let rowIn = 1
    for (let i = 200; i >= 0; i--) {
      const t = i / 200
      const { x, y } = at(t)
      if (!inside(own, x, y)) out = Math.min(out, t)
      if (inside(panelBox, x, y)) into = t
      if (inside(rowBox, x, y)) rowIn = t
    }

    for (const other of others) {
      const box = rel(offsetIn(other, player), other)
      let enter = 2
      let leave = 2
      for (let i = 0; i <= 200; i++) {
        const t = i / 200
        const { x, y } = at(t)
        const over = inside(box, x, y)
        if (over && enter === 2) enter = t
        if (!over && enter !== 2 && leave === 2) leave = t
      }
      other.style.setProperty('--in', enter.toFixed(3))
      other.style.setProperty('--out', leave.toFixed(3))
    }

    player.style.setProperty('--to-x', `${to.x.toFixed(1)}px`)
    player.style.setProperty('--to-y', `${to.y.toFixed(1)}px`)
    player.style.setProperty('--h-out', out.toFixed(3))
    player.style.setProperty('--h-in', into.toFixed(3))
    player.style.setProperty('--row-in', rowIn.toFixed(3))
  }

  measure()
  new ResizeObserver(measure).observe(player)
  // The text's width depends on the web font, which may arrive after this ran.
  void document.fonts?.ready.then(measure)
}
