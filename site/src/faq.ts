/**
 * The home page's questions — the ones people ask an assistant before they ask
 * a search box. Each answer stands on its own and names the player, because an
 * answer engine quotes one answer, not the page around it. The same list is
 * rendered by `components/Faq.astro` and published as `FAQPage` structured data
 * by `pages/index.astro`, so the two cannot disagree.
 */
export interface Question {
  q: string
  a: string
  /** A page that says more, shown after the answer and left out of the structured data. */
  more?: { href: string; label: string }
}

export const FAQ: Question[] = [
  {
    q: 'What is Frame Player?',
    a: 'Frame Player is a free, open-source video player for Windows 10, Windows 11 and Apple Silicon Macs, built on libmpv. It plays local files, links and torrents, streams a torrent while it downloads, casts to Chromecast and DLNA TVs, and lets several people watch on one timeline.',
  },
  {
    q: 'Is Frame Player free?',
    a: 'Yes. Frame Player is free and open source under the GPL-3.0, with no account, no subscription and no ads.',
  },
  {
    q: 'Can Frame Player play a torrent before it finishes downloading?',
    a: 'Yes. Paste a magnet link or open a .torrent file and playback starts while it downloads; the pieces near the playhead are fetched first. Seeding is off by default.',
    more: { href: '/torrent-streaming', label: 'How torrent streaming works' },
  },
  {
    q: 'Can Frame Player cast to a TV?',
    a: 'Yes, to Chromecast and other Google Cast devices and to DLNA TVs, and from a Mac to an Apple TV or a TV with AirPlay. When the TV can play the file as it is, Frame Player sends it without re-encoding; otherwise it remuxes the file and copies the video stream.',
    more: { href: '/cast-to-tv', label: 'Casting to a TV' },
  },
  {
    q: 'How is Frame Player different from VLC or IINA?',
    a: 'Frame Player decodes with libmpv, the engine behind mpv and IINA, and runs on both Windows and macOS, while IINA is macOS only. It has torrent streaming and watching together built in, which neither VLC nor IINA has, and casts to Chromecast and DLNA TVs without re-encoding; VLC casts only to Chromecast, converting the video on the fly, and IINA does not cast.',
    more: { href: '/compare/vlc', label: 'Frame Player compared with VLC' },
  },
  {
    q: 'Does Frame Player collect data?',
    a: 'No tracking and no account. Besides the update check and the services you use yourself, such as subtitle search, Frame Player sends one anonymous report on its first launch that names only the platform.',
  },
  {
    q: 'Does Frame Player run on Linux or Intel Macs?',
    a: 'Not yet. Frame Player is built for Windows 10 and 11 (x64) and for Macs with Apple Silicon.',
  },
]
