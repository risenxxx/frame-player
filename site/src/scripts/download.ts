/**
 * The hero's torrent, downloading.
 *
 * The built page shows a still: the readout with a rate and a peer count, and
 * the track shaded to 46%. Once the hero's scene plays (`sceneplay`, from
 * motion.ts) this takes the track over and plays a download out the way the
 * application draws one: the stretch ahead of the playhead first, a few pieces
 * the swarm hands over out of order, the file's last pieces early (mpv reads a
 * Matroska index from its end), the rate climbing as peers connect and easing
 * off in the endgame. When the last piece lands the readout goes away, as the
 * application's does once nothing is left to fetch (`chip` in open.svelte.ts).
 *
 * Time is compressed — a 4K episode does not arrive in fifteen seconds — so the
 * rate and the peer count are plausible numbers, not the ones that would fill
 * the bar this fast.
 *
 * Under reduced motion nothing here runs and the still stays.
 */

/** Pieces on the track. Fine enough that one arriving is a sliver, not a block. */
const PIECES = 200
/** The playhead, as a share of the track — `--pos` in PlayerMock's TRACK. */
const PLAYHEAD = 0.074
/** How long after the scene starts the download does: the interface is up by then. */
const START_DELAY_MS = 1300
/** How often pieces land, and how often the readout's numbers change. */
const PIECE_TICK_MS = 90
const READOUT_TICK_MS = 900
/** How long the finished readout stays before it goes. */
const LINGER_MS = 700
/** Average pieces per tick at the full rate; the whole run is about fifteen seconds. */
const PIECES_PER_TICK = 1.7
/** Of the pieces taken in order, how many come from the frontier rather than anywhere. */
const IN_ORDER = 0.72

export function initDownload(): void {
  const player = document.querySelector<HTMLElement>('.player.hero')
  const track = player?.querySelector<HTMLElement>('.seektrack')
  const readout = player?.querySelector<HTMLElement>('.torchip')
  const figures = readout?.querySelector<HTMLElement>('.torchip-line .mono')
  if (!player || !track || !readout || !figures) return
  if (matchMedia('(prefers-reduced-motion: reduce)').matches) return
  /* Built only once the scene plays: until then the interface is at zero
     opacity, so the swap from the still is never seen — and a page whose
     motion never starts keeps the still rather than a download frozen at 16%. */
  player.addEventListener('sceneplay', () => run(player, track, readout, figures), { once: true })
}

function run(player: HTMLElement, track: HTMLElement, readout: HTMLElement, figures: HTMLElement): void {
  const strip = document.createElement('span')
  strip.className = 'pieces'
  const cells: HTMLElement[] = []
  for (let i = 0; i < PIECES; i++) cells.push(strip.appendChild(document.createElement('i')))
  track.insertBefore(strip, track.querySelector('.fill'))

  const have = new Array<boolean>(PIECES).fill(false)
  let count = 0
  const mark = (i: number, fresh: boolean): void => {
    if (have[i]) return
    have[i] = true
    count++
    cells[i]?.classList.add(fresh ? 'new' : 'on')
  }

  /* What a torrent a few seconds in already has: everything up to just past the
     playhead, the index at the end, and a few early arrivals. */
  const head = Math.round(PIECES * (PLAYHEAD + 0.045))
  for (let i = 0; i < head; i++) mark(i, false)
  for (let i = PIECES - 3; i < PIECES; i++) mark(i, false)
  for (const at of [0.135, 0.22, 0.31]) mark(Math.round(PIECES * at), false)

  const showDisk = (): void => player.style.setProperty('--disk', `${((count / PIECES) * 100).toFixed(1)}%`)
  showDisk()
  player.classList.add('live')

  let frontier = head
  const next = (): number | null => {
    while (frontier < PIECES && have[frontier]) frontier++
    if (frontier < PIECES && Math.random() < IN_ORDER) return frontier
    const missing: number[] = []
    for (let i = 0; i < PIECES; i++) if (!have[i]) missing.push(i)
    if (missing.length === 0) return null
    return missing[Math.floor(Math.random() * missing.length)] ?? null
  }

  /* The rate is a share of the full one: it climbs as peers connect, wanders,
     and falls away over the last tenth, when only a few slow peers hold what
     is left. */
  let peers = 5
  let rate = 0.08
  const target = (): number => {
    const left = 1 - count / PIECES
    const endgame = left < 0.1 ? 0.45 + left * 5.5 : 1
    return Math.min(1, peers / 36) * endgame
  }
  const say = (): void => {
    const mbs = Math.max(0.4, rate * 38 + (Math.random() - 0.5) * 2).toFixed(1)
    figures.innerHTML = `${mbs} MB/s<span class="sep"></span>${peers} peers`
  }
  /* The still's numbers are a torrent in full flow; this one is starting. The
     interface is not up yet, so the change is not seen. */
  say()

  let carry = 0
  let pieceTimer = 0
  let readoutTimer = 0

  const finish = (): void => {
    clearInterval(pieceTimer)
    clearInterval(readoutTimer)
    setTimeout(() => readout.classList.add('done'), LINGER_MS)
  }

  const start = (): void => {
    readoutTimer = window.setInterval(() => {
      // Connecting quickly at first, then a swarm that comes and goes.
      const step = peers < 34 ? 4 + Math.random() * 6 : Math.random() * 7 - 3.5
      peers = Math.max(3, Math.min(52, peers + Math.round(step)))
      say()
    }, READOUT_TICK_MS)
    pieceTimer = window.setInterval(() => {
      rate += (target() - rate) * 0.07
      carry += PIECES_PER_TICK * rate * (0.6 + Math.random() * 0.8)
      while (carry >= 1) {
        carry--
        const i = next()
        if (i === null) break
        mark(i, true)
      }
      showDisk()
      if (count >= PIECES) finish()
    }, PIECE_TICK_MS)
  }

  setTimeout(start, START_DELAY_MS)
}
