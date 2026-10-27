/**
 * The hero's torrent, downloading — the way the application shows one.
 *
 * The built page shows a still: the readout with a rate and a peer count, and
 * the track shaded to 46%. Once the hero's scene plays (`sceneplay`, from
 * motion.ts) this takes the track over and plays a download out, keeping to
 * what the player actually does rather than what would look busy:
 *
 * - **The order is librqbit's.** A stream's 32 MB lookahead from mpv's read
 *   position goes first, then the file's pieces in their natural order, each
 *   peer taking the next one it has (`piece_tracker.rs`, `streaming.rs`). So
 *   the bar grows from the playhead in one front, ragged only at its edge,
 *   where several pieces are in flight at once and land out of turn — not as
 *   pieces scattered across the file. The file's last pieces are there from the
 *   start: mpv reads a Matroska index from the end before it plays a frame.
 * - **The bar is bands, read every two seconds.** `torrent_buffered` merges
 *   the pieces into `[from, to]` runs, `SeekBar.svelte` draws each as a
 *   rounded `.seekbuffered`, and `BUFFER_POLL_MS` is 2000 — so the shading moves
 *   in steps, and a piece arriving does nothing visible on its own.
 * - **The readout is read every second** (`POLL_MS`), its bar easing over
 *   0.3 s, and it is gone — not faded — the moment the file is complete, since
 *   the application drops the chip once nothing is left to fetch (`chip` in
 *   open.svelte.ts).
 *
 * Time is compressed — a 4K episode does not arrive in twenty seconds — so the
 * rate and the peer count are plausible numbers, not the ones that would fill
 * the bar this fast.
 *
 * Under reduced motion nothing here runs and the still stays.
 */

/** Pieces in the file. The real one has a thousand or so; the bar cannot tell. */
const PIECES = 240
/** The playhead, as a share of the track — `--pos` in PlayerMock's TRACK. */
const PLAYHEAD = 0.074
/** How far past the playhead a torrent a few seconds in already reaches. */
const AHEAD = 0.03
/** The file's end that mpv read first, for the Matroska index. */
const TAIL = 0.01
/** How long after the scene starts the download does: the interface is up by then. */
const START_DELAY_MS = 1300
/** The application's own polls: `BUFFER_POLL_MS` and `POLL_MS` in torrent.svelte.ts. */
const BUFFER_POLL_MS = 2000
const STATUS_POLL_MS = 1000
/** The simulation's step, finer than either poll. */
const TICK_MS = 100
/** At most this many pieces in flight — about one per peer, up to a point. */
const MAX_IN_FLIGHT = 10
/** Chance per tick that one in-flight piece completes, at the full rate. */
const COMPLETE_P = 0.18
/** Chance that a peer lacks the next piece and takes one a little further on. */
const SKIP_P = 0.12

export function initDownload(): void {
  const player = document.querySelector<HTMLElement>('.player.hero')
  const track = player?.querySelector<HTMLElement>('.seektrack')
  const readout = player?.querySelector<HTMLElement>('.torchip')
  const figures = readout?.querySelector<HTMLElement>('.torchip-line .mono')
  if (!player || !track || !readout || !figures) return
  if (matchMedia('(prefers-reduced-motion: reduce)').matches) return
  /* Built only once the scene plays: until then the interface is at zero
     opacity, so the swap from the still is never seen — and a page whose
     motion never starts keeps the still rather than a download frozen early. */
  player.addEventListener('sceneplay', () => run(player, track, readout, figures), { once: true })
}

function run(player: HTMLElement, track: HTMLElement, readout: HTMLElement, figures: HTMLElement): void {
  const bandLayer = document.createElement('span')
  bandLayer.className = 'bands'
  track.insertBefore(bandLayer, track.querySelector('.fill'))

  const have = new Array<boolean>(PIECES).fill(false)
  const inFlight = new Set<number>()
  let count = 0
  const mark = (i: number): void => {
    if (have[i]) return
    have[i] = true
    count++
  }
  for (let i = 0; i < Math.round(PIECES * (PLAYHEAD + AHEAD)); i++) mark(i)
  for (let i = Math.round(PIECES * (1 - TAIL)); i < PIECES; i++) mark(i)

  /* `file_ranges` in torrent/status.rs: consecutive pieces merge into one band. */
  const drawBands = (): void => {
    const bands: [number, number][] = []
    for (let i = 0; i < PIECES; i++) {
      if (!have[i]) continue
      const last = bands[bands.length - 1]
      if (last && last[1] === i) last[1] = i + 1
      else bands.push([i, i + 1])
    }
    bandLayer.replaceChildren(
      ...bands.map(([from, to]) => {
        const band = document.createElement('i')
        band.style.left = `${(from / PIECES) * 100}%`
        band.style.width = `${((to - from) / PIECES) * 100}%`
        return band
      }),
    )
  }

  /* The next piece a peer would take: the first one in natural order that is
     neither on disk nor already being fetched — or, now and then, one a little
     further on, from a peer that lacks the first. */
  const nextPiece = (): number | null => {
    const free: number[] = []
    for (let i = 0; i < PIECES && free.length < 12; i++) if (!have[i] && !inFlight.has(i)) free.push(i)
    if (free.length === 0) return null
    if (free.length > 1 && Math.random() < SKIP_P) return free[1 + Math.floor(Math.random() * (free.length - 1))] ?? null
    return free[0] ?? null
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
  /* The readout's words come from the page (`data-say`, in its language);
     only the two numbers are this script's. */
  const words = (figures.dataset.say ?? '{rate} MB/s{sep}{n} peers')
    .replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;')
    .replace('{sep}', '<span class="sep"></span>')
  const say = (): void => {
    const mbs = Math.max(0.4, rate * 38 + (Math.random() - 0.5) * 2).toFixed(1)
    figures.innerHTML = words.replace('{rate}', mbs).replace('{n}', String(peers))
    player.style.setProperty('--disk', `${((count / PIECES) * 100).toFixed(1)}%`)
  }

  /* The still's numbers are a torrent in full flow; this one is starting. The
     interface is not up yet, so none of this swap is seen. */
  say()
  drawBands()
  player.classList.add('live')

  let tickTimer = 0
  let statusTimer = 0
  let bufferTimer = 0

  const start = (): void => {
    tickTimer = window.setInterval(() => {
      rate += (target() - rate) * 0.07
      const slots = Math.min(MAX_IN_FLIGHT, Math.max(1, Math.round(peers / 3)))
      while (inFlight.size < slots) {
        const i = nextPiece()
        if (i === null) break
        inFlight.add(i)
      }
      for (const i of [...inFlight]) {
        if (Math.random() < COMPLETE_P * rate * (0.5 + Math.random())) {
          inFlight.delete(i)
          mark(i)
        }
      }
      if (count >= PIECES) clearInterval(tickTimer)
    }, TICK_MS)

    statusTimer = window.setInterval(() => {
      if (count >= PIECES) {
        // Nothing left to fetch: the application drops the chip outright.
        clearInterval(statusTimer)
        readout.classList.add('done')
        return
      }
      // Connecting quickly at first, then a swarm that comes and goes.
      const step = peers < 34 ? 4 + Math.random() * 6 : Math.random() * 7 - 3.5
      peers = Math.max(3, Math.min(52, peers + Math.round(step)))
      say()
    }, STATUS_POLL_MS)

    bufferTimer = window.setInterval(() => {
      drawBands()
      if (count >= PIECES) clearInterval(bufferTimer)
    }, BUFFER_POLL_MS)
  }

  setTimeout(start, START_DELAY_MS)
}
