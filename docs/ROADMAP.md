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
| 13 | Volume levelling |
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

Details for the larger ones: [torrents.md](torrents.md), [casting.md](casting.md).

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

### 28. Lift the subtitles when the interface is up

The bottom bar covers subtitles while it is visible. The fix is one property and
one trap:

- `sub-margin-y` looks like the right control and **does not affect ASS
  subtitles at all** — measured on the same frame, plain subtitles moved 191 px
  and ASS did not move.
- `sub-pos` moves both, because mpv's subtitle override default lets it reach
  ASS. It is also **the slider the settings dialog exposes**, so the shift has to
  be computed from the viewer's own value, restored exactly, and never written
  to the configuration file.

The amount has to come from the bar's height as a fraction of the window rather
than a constant — 20 points of `sub-pos` moved the line 206 px in a 1080-tall
frame, so a point is about one percent, and a constant would be wrong in the
mini player and on a large display. One measurement is still open: how it
behaves when the video is letterboxed and shorter than the window.

### 29. Playback speed on the remote

Both transports can carry it (the Cast media namespace has a playback rate; the
UPnP play action takes a speed). Currently the speed keys refuse while casting.
Small.

### 30. Free-space preflight before preparing

Preparing a compatible copy can fill the disk, and the failure surfaces as a
generic encoder error. Check first and say so.

## Under consideration

No firm plans. Each of these is either large, dependent on somebody else's
platform, or of unproven value — kept here with the reasoning so the next
evaluation does not start from zero.

### Watch together

A room on a small relay that syncs position, pause, speed and which file is
playing; no media through the server. The protocol is the easy half. The work is
that every gesture guard in the player — dragging, scrubbing, settling,
advancing — is a place a remote event can land badly, and that drift must be
corrected by nudging speed rather than by seeking, which on a slow file costs
nearly two seconds and desynchronises worse than the drift did.

Rule of thumb if it is ever built: **sync the timeline, not the presentation.**
Position, pause, speed and the playing file are shared; tracks, subtitle
appearance, volume and window state stay personal.

It pairs naturally with torrents, because an infohash plus an index identifies
content exactly where local files would have to be matched by size and a partial
hash. The asymmetry is what keeps it here: torrents pay off for one person
immediately, this only once somebody else installs the player.

### Shell integration on Windows: thumbnails and the preview pane

A folder of `.mkv` files in Explorer is a grid of identical icons. Two of the
three mechanisms behind the word "preview" cost nothing to run — a thumbnail
provider and a preview handler are both components the shell loads on demand and
drops again, with no background process. The work is COM registration, a
separate small binary, and the shell's caching behaviour, which is
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
Frame-exact control, sub-5 ms stepping, perfect colour agreement between the
video and what is drawn over it — at the cost of the hardest native work on both
platforms. The current architecture does not preclude it, which is the reason it
can stay on this list rather than being decided now.
