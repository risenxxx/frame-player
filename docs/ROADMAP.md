# Roadmap

What has shipped, what is planned, and what is only being considered.

Numbers are stable: code comments refer to items by them ("ROADMAP 21"), so a
shipped item keeps its number for ever and new work continues the sequence.

The measure throughout is **value ÷ effort against what a viewer actually
does** — not feature parity with other players. Several of the entries below are
in the "considered" list precisely because they would be impressive and change
nothing about watching a film.

## Shipped

| # | Feature |
|---|---|
| 1 | Frame export — save or copy the current frame, through mpv so it keeps what the video output produced |
| 2 | Subtitle and audio delay, remembered per file |
| 3 | External subtitle and audio files, by drop or picker |
| 4 | Volume above 100 % |
| 5 | Preferred track languages, as an ordered list |
| 6 | The standard hotkey set, with an editor (18) |
| 7 | Chapters: list, navigation, marks on the seekbar, skip-intro/credits offers |
| 8 | Playlist panel and folder auto-queue around the opened file |
| 9 | A–B loop |
| 10 | Media info panel — a live readout, not a snapshot |
| 11 | Picture geometry: rotation, aspect override, pan-and-scan |
| 12 | Audio device selection |
| 13 | Volume leveling |
| 14 | Subtitle appearance: size, position, border style |
| 15 | Network streams via yt-dlp, with the binary managed by the player |
| 16 | Subtitle search and download from OpenSubtitles, by file hash first |
| 17 | Mini player — a small always-on-top window that floats over other applications' fullscreen |
| 18 | Keybinding editor |
| 19 | Per-file and per-folder track memory, matched by description rather than index |
| 20 | Source identity: one key per video, independent of how it is reached |
| 21 | Torrent streaming — piece priority from playback, buffered bands on the seekbar, season as a queue |
| 25 | Content languages beyond the two interface languages |
| 26 | Casting to a television — Google Cast and DLNA, including torrents |
| 32 | Watching together — a shared timeline over a small relay, with torrents picked up by the other players automatically |
| 33 | Catalog — browse and search films and series, then pick a release for one, with metadata through a proxy of our own so the player carries no API key |
| 28 | Subtitles lifted clear of the control bar while it is up |

Details for the larger ones: [torrents.md](torrents.md), [casting.md](casting.md),
[watch-together.md](watch-together.md), [catalog.md](catalog.md).

Everything not listed — seekbar thumbnails, resume, watch history, zoom and pan,
window preferences, HDR handling, the updater, media keys, the macOS menu bar —
predates the roadmap and is described in [architecture.md](architecture.md).

## Planned

### 27. Subtitles on the television

Casting currently sends no subtitles of its own. Both transports have a path and
they are different sizes.

**Cast** takes a WebVTT track alongside the media: extract the chosen subtitle
track, serve it from the same server with cross-origin headers, and declare it
in the load message. Image subtitles (PGS, VobSub) are out of scope — they would
need the video transcode rung that does not exist.

**DLNA** is smaller and stranger. A release goes over whole, so the television
already has every subtitle track in the file and can select one with its own
remote; the vendor extension found on one renderer is an on/off switch, not a
track chooser — its allowed values are literally `UNKNOWN`, `ON`, `OFF`. Its
value is that the companion read-back exists, so a toggle in the player can show
the truth rather than a guess. An hour of work, and only where the device
declares the extension.


### 29. Playback speed on the remote

Both transports can carry it (the Cast media namespace has a playback rate; the
UPnP play action takes a speed). Currently the speed keys refuse while casting.
Small.

### 30. Free-space preflight before preparing

Preparing a compatible copy can fill the disk, and the failure surfaces as a
generic encoder error. Check first and say so.

### 31. Fetch in the background, watch when it is ready

A slow swarm is not a broken one, and the player has exactly one answer for both
today: sit on the loading overlay and count peers. When the rate is such that
playback cannot keep up — a torrent with two distant seeders at 40 KB/s is
ordinary, not pathological — the useful offer is the opposite of streaming:
go back to the start screen, leave the torrent fetching, and say when there is
enough to start.

The pieces already exist. `torrent_prefetch` is exactly "fetch this file with
nobody reading it", bounded to one file and already used for the next episode.
`torrent_buffered` is the map, and the start-screen row already polls and prints
a line. So the work is the *rule* and the *offer*, not the mechanism:

- **When to offer.** Not a timer: a stall that clears in eight seconds must not
  raise a dialog. The signal is a rate that cannot sustain playback — the file's
  own bitrate is knowable (`file-size / duration`) and the swarm's rate is in the
  status — held for long enough that it is the swarm rather than a cold seek.
- **What "ready" means.** A percentage is the wrong measure, because a lead that
  is enough for a 2 Mbit rip is nothing for a 4K remux. It is a *time* lead:
  contiguous bytes from the start, divided by the bitrate, against the remaining
  download at the observed rate. That is the same arithmetic the cast rung's
  `STREAM_LEAD_BYTES` makes by hand, and it should be shared rather than copied.
- **Where it says so.** The torrent row on the start screen is already the place
  a season is picked up from, and it already carries a line under the name. A
  fetching torrent belongs there, with its rate and its estimate, and it becomes
  clickable when the lead is there.
- **What it must not become.** A resident BitTorrent client. One file at a time,
  only ever a file the viewer asked for, and it stops when the app does — the
  same three bounds `prefetch` is written under.

Worth doing on its own merits and *not* as a workaround: the bug that prompted
it (a release with 24 seeders that would not download at all) was a
dead tracker announce, not a slow swarm — see the tracker section of
`torrents.md` and `src-tauri/vendor/README.md`.

## Under consideration

No firm plans. Each of these is either large, dependent on somebody else's
platform, or of unproven value — kept here with the reasoning so the next
evaluation does not start from zero.

### Shell integration on Windows: thumbnails and the preview pane

A folder of `.mkv` files in Explorer is a grid of identical icons. Two of the
three mechanisms behind the word "preview" cost nothing to run — a thumbnail
provider and a preview handler are both components the shell loads on demand and
drops again, with no background process. The work is COM registration, a
separate small binary, and the shell's caching behavior, which is
unforgiving of a slow or crashing provider.

### Quick Look on Space (Windows)

Separate from the above because it is the one part that **cannot be done without
a process running all the time**, which is a product decision rather than an
implementation detail. There is no extension point: Space toggles selection,
Windows has no built-in equivalent, and the shell offers no hook for "a key
opened a preview". Every utility that does this keeps a resident process.

### AirPlay

The transport is the same URL handoff as the other two and would be about a
day's work. Everything before it is the obstacle: modern receivers gate every
endpoint behind pairing — one measured set refuses unauthenticated requests
outright and does not even advertise itself — which means implementing the
pairing and encrypted-channel stack with no maintained library to build on. The
device class it would unlock that nothing else reaches is essentially the Apple
TV, since sets with AirPlay generally also speak DLNA.

### Miracast

Not hard so much as a different product: it is screen mirroring, so the sender
encodes the display and streams it over a peer-to-peer link. This player has no
frames to encode — the video is not in the compositing tree — and macOS has no
support for the protocol at all.

### Device calibration

An automated pass that plays a few probe files on a television and records what
worked, so the compatibility ladder becomes measured fact per device rather than
inference. Its value dropped sharply once DLNA landed: a renderer publishes its
own format list, and the device check in the picker already shows it. What is
left is the part a machine cannot judge — whether surround actually reached the
room — which is two questions to a listener, not a wizard.

### Durable storage beyond the webview

Preferences and history live in the webview's local storage, which is per
application identity and outside any browser's profile — so clearing a browser
cache does not touch it. Two risks remain: an identity change orphans everything
silently (this has already happened across renames), and cleaning utilities do
sweep the directory. The proportionate fix is a mirror, not a rewrite: write
through to a file in the application's own data directory and restore on startup
when a key is missing. The whole store is a few kilobytes, and every call site
stays synchronous — which a store-plugin migration would not allow.

### Segmented streaming for a receiver without DLNA

The one case neither shipped transport reaches: a Cast-only receiver and a
source that is not fully downloaded and would need repacking. The segmenter
exists and is wired to nothing. It would carry H.264 with stereo only, which is
why it is not a priority.

### Posters for ordinary files

The start screen decodes a frame from the file when it needs one. A capture
taken while the file plays already exists for torrents, and generalising it
would give every entry a picture that outlives a rename, a deletion or an
unplugged drive. Small, and only worth doing if those cases turn out to matter.

### A render-API presenter

Replacing the child-view embedding with our own surface and mpv's render API.
Frame-exact control, sub-5 ms stepping, perfect color agreement between the
video and what is drawn over it — at the cost of the hardest native work on both
platforms. The current architecture does not preclude it, which is the reason it
can stay on this list rather than being decided now.
