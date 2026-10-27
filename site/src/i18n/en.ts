/*
  Every word the site's own templates print, in English. This file defines the
  shape: ru.ts and zh.ts are typed against it, so a string added here and not
  there is a type error rather than a gap somebody finds on the live page.

  The guide pages are not here — each is an MDX file of its own — and neither
  are the release notes, which are written once, in English, by the release.

  A string can carry a little markup, rendered by `rich()` in index.ts and
  nothing else, so a translator moves a bold phrase with the words around it:

    **bold**      <b>
    *italic*      <i>
    `mono`        <span class="mono">
    {sep}         the dot between two facts (<span class="sep"></span>)
    {name}        a value the template passes in — keep the braces as they are

  Lists (the checks under a section, the cells of the grid, the questions) are
  matched to their icons and links by position: translate them in place, never
  reorder them.
*/

export const en = {
  meta: {
    title: 'Frame Player — a video player for Windows and macOS',
    description:
      'Free, open-source video player on mpv for Windows and macOS: watch a torrent as it downloads, ' +
      'cast to the TV untouched, resume anything.',
    imageAlt: 'Frame Player: files, links, magnets. One player that gets them right.',
  },

  nav: {
    toTop: 'Frame Player, to the top',
    home: 'Frame Player, home',
    sections: 'Sections',
    open: 'Open anything',
    torrents: 'Torrents',
    tv: 'Television',
    together: 'Together',
    everything: 'Everything else',
    download: 'Download',
  },

  theme: {
    legend: 'Theme',
    systemTitle: 'Match the system',
    system: 'System',
    light: 'Light',
    dark: 'Dark',
  },

  /* The language switch, and the hint that offers this language to a reader
     whose browser asks for it — the hint is written in its own language,
     since it is shown to somebody who reads that one. */
  lang: {
    legend: 'Language',
    hint: 'This page is also in English.',
    open: 'Read in English',
    dismiss: 'Dismiss',
  },

  hero: {
    title: ['Files, links, magnets.', 'One player that gets them right.'],
    lede:
      'Frame Player is a free, open-source video player for Windows and macOS, built on mpv. It ' +
      'streams torrents while they download, casts to Chromecast and DLNA TVs without re-encoding, ' +
      'and lets friends watch together.',
    shot:
      'Frame Player playing a 4K file from a torrent: a hover preview of a later scene above the ' +
      'seekbar, chapter marks along it, and a skip-intro button over the video.',
  },

  download: {
    windows: 'Download for Windows',
    macos: 'Download for macOS',
    version: 'Version {version}',
    platforms: 'Windows 10/11{sep}Apple Silicon',
    license: 'free and open source, GPL-3.0',
    /* Rounded down to a ten (a hundred past 10 000), so `{n}` always ends in a
       zero — which in Russian always takes the same plural. */
    installs: '{n}+ installs',
  },

  open: {
    h: 'If it points at a video, it opens',
    lede:
      "Files, folders, links, magnets, `.torrent` files and a release's RSS feed all open in one window. Playback is libmpv, so anything mpv plays — MKV, HEVC, AV1, HDR, Dolby Vision — plays here.",
    checks: [
      '**One episode opens the folder** as a queue in natural order: ep2 before ep10.',
      '**Video pages open through yt-dlp**, which the player can install for you when a link first needs it.',
      '**Resume survives a restart**, even for a torrent episode.',
    ],
  },

  torrents: {
    h: 'Watch it while it is still downloading',
    lede:
      'Paste a magnet or open a `.torrent` and Frame Player starts playing while it downloads. Pieces near the playhead come first, and the seekbar shows what has arrived, so you know whether a jump will land.',
    checks: [
      '**Nothing downloads until you open it**, and seeding is off by default.',
      '**Subtitles inside the release attach themselves**, and the next episode is fetched ahead.',
      '**A stall says why:** download rate, peers and how much of the file is on disk.',
    ],
    more: 'How torrent streaming works',
  },

  tv: {
    h: 'Send it to the set, without repacking it first',
    lede:
      'Cast to Chromecast, a DLNA TV or — from a Mac — an AirPlay TV from one button. Each TV in the list says what will happen to *this* file; when the set can play the release as it is — often 4K HEVC HDR with Dolby audio — nothing is re-encoded.',
    checks: [
      '**Otherwise the file is remuxed**, with the video copied rather than re-encoded, so it is ready in seconds.',
      '**A torrent can be cast while it downloads.**',
      '**The window becomes a remote:** keys, chapters and the queue control the TV, and disconnecting resumes on the computer.',
    ],
    more: 'Casting, in detail',
  },

  together: {
    h: 'Two machines, one timeline',
    lede:
      'Share a six-character code and everyone in the room stays on the same position, pauses and episode. Seek on one and the other lands on the same frame, even on a slow connection.',
    checks: [
      '**Drift is corrected by speed, not by seeking** — ten seconds at 103% closes the gap without a jump.',
      '**The room waits for whoever is buffering** and resumes by itself.',
      '**Torrents need no pasting:** everyone joins the same swarm. Only the timeline goes through the relay, never the video.',
    ],
    more: 'How watching together works',
  },

  subs: {
    h: 'Subtitles that are already in sync',
    lede:
      "Frame Player searches OpenSubtitles by the file's hash, which matches your exact release instead of guessing from the title. Title search is the fallback, and the panel says which one you got.",
    checks: [
      '**A frame-rate mismatch is flagged before you download** — a 25 fps subtitle on a 23.976 fps file drifts further out the longer you watch.',
      '**Subtitles move above the controls** while the bar is up, and a delay you set is kept for that file.',
      '**The dub you picked in episode 1 is picked again in episode 2**, matched by language and title, not track number.',
    ],
    more: 'Why subtitles drift out of sync',
  },

  catalog: {
    h: 'Find something to watch without leaving the player',
    lede:
      'Browse posters, seasons and localized titles, then pick a release from your own Torznab indexer, such as Jackett or Prowlarr — the player ships with none. It opens as a torrent, with resume and the queue.',
    checks: [
      '**Releases are sorted by quality**, then dynamic range, then seeders.',
      "**The player carries no API key**: metadata goes through the project's own proxy.",
      '**It is the one feature that sends what you search for**, so it has an off switch, and everything else works without it.',
    ],
  },

  resume: {
    h: 'Pick up where you left off, in anything',
    lede:
      'Files, links and torrent episodes all resume where you stopped. The start screen lists what is unfinished, each with a poster frame taken from the video itself.',
    checks: [
      '**A finished video leaves the list by itself**, and a torrent remembers which episodes you watched.',
      '**A season is one row, not nine cards**, and clicking it continues the episode you were on.',
    ],
  },

  picture: {
    h: 'The picture, cropped and tuned by eye',
    lede:
      "Auto crop removes black bars baked into the video by measuring frames across the whole file, and the window takes the picture's shape. Brightness, contrast, saturation and gamma adjust live, with the menu out of the way.",
    checks: [
      "**One choice covers the season**: each episode is measured on its own, so a wider finale is not cut to the pilot's bars.",
      '**It works on a torrent still downloading.**',
      '**Set a correction for one file, its playlist or everything**; when two disagree, the player asks which to keep.',
    ],
    more: ['How auto crop finds the bars', 'Why the same video looks darker in mpv than in QuickTime'],
  },

  everything: {
    h: 'The parts you only notice when they are missing',
    lede: 'Small things other players get almost right.',
    cells: [
      { h: 'On screen in half a second', p: '**0.56 s** from click to a usable window on an Apple Silicon Mac: nothing loads before you need it.' },
      { h: 'Live media info', p: 'Codec, resolution, audio layout and the real bitrate, refreshed every second while the panel is open.' },
      { h: 'The window remembers where it was', p: 'Size and position come back at launch, or the window fits the video without leaving the screen.' },
      { h: 'The preview is the frame you land on', p: 'Seekbar thumbnails sharpen to the exact frame once the cursor stops, not the keyframe before it.' },
      { h: 'Skip intro that knows what an intro is', p: 'Chapters match by their whole title, so "Ending the war for good" is never taken for the credits.' },
      { h: 'A mini player that stays put', p: 'Small, always on top and snapped to a corner. On macOS it floats over other apps in fullscreen.' },
      { h: '47 actions, all rebindable', p: 'Hotkeys follow physical keys, so they work in any keyboard layout. J, K and L do what you expect.' },
      { h: 'HDR, Dolby Vision, anything', p: 'Decoded by libmpv, as in mpv and IINA: full format coverage, hardware decoding and HDR tone mapping.' },
      { h: 'A–B loops and frame steps', p: 'Loop the stretch between two marks, and step one frame forward or back.' },
      { h: 'Zoom and pan without a jump', p: 'Zoom and position change in the same frame, so the picture never jumps between two redraws.' },
      { h: 'Export the frame you are on', p: 'Save or copy the exact decoded frame, HDR tone mapping included.' },
      { h: 'Audio that behaves', p: 'Pick the output device, level a film that whispers then shouts, and keep an audio delay per file.' },
      { h: 'The folder is the queue', p: 'The next episode starts after a countdown you can cancel. Repeat one file, the queue, or nothing.' },
      { h: 'Folders that leave no trace', p: 'Mark a folder private and nothing about it is recorded or sent anywhere. There is no account, and the player reports one thing, once: that it was installed.' },
      { h: 'Your own mpv.conf still wins', p: 'Settings change only their own lines in mpv.conf, so options you wrote by hand survive.' },
    ],
    spare:
      'Oops — this square needed covering. Fifteen does not divide by two, so at this width the grid comes up one short. Widen the window and the corner goes away.',
  },

  faq: {
    h: 'Questions',
    /* Each answer stands on its own and names the player, because an answer
       engine quotes one answer, not the page around it. `more` is a page that
       says more, shown after the answer and left out of the structured data. */
    items: [
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
    ] as { q: string; a: string; more?: { href: string; label: string } }[],
  },

  recent: {
    h: 'Recently changed',
    lede: 'Releases come often and install themselves. This is what the last few brought.',
    andMore: 'and {n} more',
    all: 'Every release',
  },

  final: {
    title: ['Open something.', 'It will remember where you stopped.'],
    copy: 'Copy',
    copied: 'Copied',
    copyWhat: 'the install command',
    notarized: 'macOS builds are notarized',
    unsigned: 'Windows builds are unsigned and stop once at SmartScreen',
  },

  footer: {
    elsewhere: 'Elsewhere',
    source: 'Source',
    updates: 'Updates',
    docs: 'Documentation',
    licence: 'Licence',
    guides: 'Guides',
    built: 'built on libmpv, FFmpeg and librqbit',
    /* `{heart}` is the drawn heart, `{author}` the link to its author. */
    made: 'Made with {heart} by {author}',
    love: 'love',
  },

  article: {
    breadcrumb: 'Breadcrumb',
    updated: 'Updated {date}',
    questions: 'Questions',
    readNext: 'Read next',
    kind: { feature: 'Feature', guide: 'Guide', comparison: 'Comparison' },
  },

  updates: {
    title: 'Updates — every Frame Player release',
    description:
      'What each version of Frame Player changed, newest first: new features, improvements and fixes for Windows and macOS.',
    feedTitle: 'Frame Player updates',
    h: 'Updates',
    lede: 'What every version changed, newest first. The player installs updates by itself, so this is mostly a record of what arrived.',
    latest: 'Latest: {version}, {date}',
    feed: 'Atom feed',
    /* Shown above the list on a page in any language but English: the notes
       themselves are written once, in English. */
    english: 'The release notes are written in English.',
  },

  notFound: {
    title: 'Not found — Frame Player',
    description: 'There is nothing at this address. Head back to the start, or download Frame Player.',
    h: 'Seeked past the end.',
    lede:
      'There is no frame at this address. The link may be old, mistyped, or pointing at something that has since moved.',
    back: 'Back to the start',
    download: 'Download Frame Player',
    osd: 'Nothing to play',
    osdSub: '404 — failed to open this address',
    /* What the window says once the page knows the address it was asked for. */
    osdPath: '404 — failed to open /{path}',
    subline: '— Are you sure this is the right address?',
    skip: 'Skip to the start',
  },

  /*
    `/llms.txt` — the site in one file for something that reads rather than
    renders. Plain Markdown, not the markup of `rich()`; `{placeholders}` are
    addresses and versions. The summary is three lines of a quote, and stays
    three lines.
  */
  llms: {
    summary: [
      'A free, open-source video player for Windows and macOS, built on libmpv. It plays local files,',
      'links and magnet torrents, streams a torrent while it downloads, sends a file to a Chromecast or',
      'a DLNA television without re-encoding it, and keeps several people on one timeline in a room.',
    ],
    version: 'Version {version}{released}. Licence: GPL-3.0-or-later.',
    released: ', released {date}',
    platforms: 'Windows 10 and 11 (x64), macOS on Apple Silicon. No account, no tracking; one anonymous report of the first launch.',
    start: 'Start here',
    home: '[Home]({url}): what the player is, with every feature in short.',
    updates: '[Updates]({url}): what every release changed, newest first.',
    downloads: '[Download for Windows]({windows}) · [Download for macOS]({macos}): always the current release.',
    features: 'Features',
    comparisons: 'Comparisons',
    guides: 'Guides',
    project: 'Project',
    source: '[Source code]({url}): Rust, Svelte and Go, GPL-3.0-or-later.',
    docs: '[Documentation]({url}): how the player works inside, and why.',
    releases: '[Releases]({url}): installers for every version, with the same notes as /updates.',
    notes: 'Notes',
    markdown: 'Every page here is also served as Markdown: add `.md` to its address, or ask for `text/markdown`.',
    dated: 'Pages carry the date they were last checked against the player; quote that date with the page.',
    languages: 'Other languages',
    language: '[{name}]({url})',
  },

  /* A guide as Markdown (`<page>.md`): the words around the page's own text. */
  markdown: {
    figure: 'Figure: {caption}',
    diagram: 'Diagram: {caption}',
    note: 'Note.',
    source: 'Source: {url}',
    updated: 'Updated: {date}',
    questions: 'Questions',
  },

  pieceMap: {
    label:
      'A row of 48 pieces. Everything before the playhead and a little after it is on disk; the next pieces are being fetched first, the ones after them are queued, a short run further on is on disk from an earlier jump, and the rest has not been asked for.',
    playhead: 'playhead',
    have: 'on disk',
    next: 'fetched first',
    soon: 'queued behind them',
    jump: 'on disk from an earlier jump',
    none: 'not asked for',
  },

  /*
    The application's own interface, as the mocks draw it. Where the player
    has a string of its own (src/lib/i18n/), the mock says what the
    player says in that language — a mock is a picture of the program, not of
    a translation of it. Titles of files stay as they are in every language.
  */
  mock: {
    common: {
      thisFile: 'This file',
      playlist: 'Playlist',
      addFile: 'Add file…',
      left: '{t} left',
      peers: '{n} peers',
      download: 'Download',
    },
    opener: {
      title: 'Open a link',
      prompt: 'Paste a link, a magnet or a feed',
      go: 'Open',
    },
    torrent: {
      show: 'Orbital Night',
      season: 'season 2',
      watched: 'watched',
      seedingOff: 'seeding off',
    },
    cast: {
      title: 'Playing on TV',
      rooms: ['Living room', 'Kitchen', 'Bedroom'],
      asIs: 'Plays as it is',
      prepared: 'Prepared before it starts',
      checking: 'Checking how this device can be reached…',
    },
    room: {
      code: 'Code',
      seek: 'seek',
      sameFrame: 'both on the same frame',
      inSync: 'in sync',
      you: 'You',
      youInitial: 'Y',
      friend: 'Anna',
      friendInitials: 'AN',
      host: 'host',
      joined: 'joined',
      laptop: 'laptop',
      desktop: 'desktop',
    },
    subs: {
      line: '— Everything you can see out there is older than the ground you stand on.',
      title: 'Subtitles',
      off: 'Off',
      find: 'Find subtitles…',
      dialog: 'Find subtitles',
      search: 'Search',
      exactHint: 'Matched by the file itself — same release, the timing fits as is.',
      downloads: '{n} downloads',
      exact: 'exact match',
    },
    player: {
      skipIntro: 'Skip intro',
      audioTrack: 'Audio track',
      tracks: ['English', 'English (5.1)', 'Russian (dub)'],
      delay: 'Delay',
      reset: 'Reset',
      chapter: 'Chapter 2',
      chapterName: 'The Terminator Line',
      chapterCamp: 'Camp at the Shore',
    },
    picture: {
      looking: 'Looking for black bars…',
      crop: 'Crop',
      auto: 'Auto',
      none: 'None',
      color: 'Brightness and color',
      brightness: 'Brightness',
      contrast: 'Contrast',
      saturation: 'Saturation',
      gamma: 'Gamma',
      applyTo: 'Apply to',
      allFiles: 'All files',
    },
    upscale: {
      title: 'Upscaling',
      off: 'Off',
      video: 'Video',
      anime: 'Anime',
      strong: 'Strong',
      active: 'Active: the picture is 2.2× its own size',
    },
    catalog: {
      title: 'Catalog',
      posters: ['Dust Lines', 'Wet Season', 'Night Shift', 'The Crossing'],
      /* The title bar's film and its year. */
      year: '{title} ({year})',
      dubs: '2 dubs',
    },
    start: {
      hint: 'Drop a video file here, or open one with the button',
      openFile: 'Open file',
      openLink: 'Open a link',
      catalog: 'Catalog',
      continue: 'Continue watching',
      torrents: 'Torrents',
      link: 'link',
      longNight: 'The Long Night',
      highway: 'Night Highway (2023)',
      totality: 'Totality, start to finish',
      videos: '9 videos',
      onDisk: '7.4 GB on disk',
      continueAt: 'continue: S02E03 — 23:14 left',
    },
  },

  /* What each picture shows, for a reader who cannot see it. Keyed by the
     picture's name in assets/img; one picture can be described twice when two
     places show different things in it. */
  alt: {
    orionMoon: 'A spacecraft in sunlight with the Moon beyond it',
    highwayTrails: 'Star trails over a desert highway',
    highwayRoad: 'Star trails over a desert road at night',
    lakeGalaxy: 'The Milky Way reflected in a mountain lake',
    lakeTrails: 'Star trails turning above a mountain lake',
    lakeTent: 'A lit tent on the shore of a mountain lake',
    earthAurora: 'Earth at night under the aurora, seen from orbit',
    cometSea: 'A comet low over a calm sea at night',
    ridgeHiker: 'A hiker under the Milky Way on a dark ridge',
    surfGlow: 'Bioluminescent surf on a beach at night',
    moonEclipse: 'A deep red eclipsed moon over a dark ridge',
  },
}

export type Strings = typeof en
