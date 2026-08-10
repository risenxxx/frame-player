# Frame Player

Local files, links and magnet torrents in one window, on **Tauri 2 + libmpv** —
frame-accurate, instant, and unusually careful about the details.

<p align="center">
  <img src="docs/intro.jpg" width="900"
       alt="Frame Player playing a file: a hover preview of a distant scene above the seekbar, chapter marks along it, and a skip-intro button over the video">
</p>

<p align="center">
  <sub>Hovering the seekbar previews the frame you would land on; the chapter is
  an opening, so the player offers to skip it.</sub>
</p>

Decoding is libmpv, the same engine as mpv and IINA, so format coverage,
hardware acceleration and HDR are not compromises. mpv renders into a native
child view *behind* a transparent webview and the entire interface is HTML
composited on top, which is what lets it be a real interface rather than
whatever an OSC script can draw.

## Contents

- [Highlights](#highlights)
- [Install](#install) — [Windows](#windows) · [macOS](#macos)
- [Features](#features)
- [Watch together](#watch-together)
- [Configuration](#configuration)
- [Hotkeys](#hotkeys)
- [Building](#building) — [Windows](#windows-1) · [macOS](#macos-1) · [Tests](#tests)
- [Project structure](#project-structure)
- [Releases](#releases)
- [License](#license)

## Highlights

Most of what follows exists somewhere else. What is unusual is how much of it is
*correct* in the cases where other players are merely plausible.

**Watch a torrent while it is still downloading.** Paste a magnet link and a
whole season becomes a queue you can start playing in seconds. Pieces are
prioritized around the playhead, the seekbar shades what has already arrived so
you can see whether a jump will land or wait, subtitles shipped inside the
torrent are attached automatically, and the next episode is fetched ahead once
the current one is complete. Nothing is downloaded until you actually ask for
it, and **seeding is off by default**.

**Watch it with somebody who is not in the room.** A six-character code puts two
players on one timeline: the same position, the same pauses, the same episode.
Drift is corrected by nudging playback speed rather than by seeking — a seek to
fix a tenth of a second costs a decode from the previous keyframe and lands you
further out than you started — and a room pauses itself for anybody still
buffering. If what you are watching is a torrent, everybody else fetches it from
the same swarm and follows you through the season without pasting anything.
Nothing but the timeline goes through the relay; no media ever does.

**Send it to the television, without repacking it first.** The player speaks
both Google Cast and DLNA and picks per device and per file: where the set can
take the release as it is — which for a DLNA renderer usually means a 4K HEVC
HDR MKV with Dolby audio, untouched — that is what it gets, with seeking and
surround intact. Where it cannot, the file is remuxed first, with the video
stream-copied so a film is ready in seconds rather than in an hour. A torrent
that is still downloading can be cast too, fed from the same piece-priority
stream that feeds local playback. While it plays the window is a remote: the
queue moves the session from episode to episode, chapters and skip buttons act
on the television, and disconnecting hands playback back at the position it had.

**Skip the intro.** Chapter navigation, a chapter list, and a skip button for
openings, recaps, "previously on", credits and ads. The button is matched on the
whole chapter title rather than a substring hidden inside it — so "Ending the
war for good" never turns into the closing credits, and the offer stays up long
enough to act on without hanging over the picture for a whole minute.

**Subtitles that are already in sync.** Search OpenSubtitles by the file's own
hash, which identifies the exact release rather than guessing which rip you
have, with a title search as the fallback — and the panel tells you which of the
two produced the list, because they are not the same kind of answer. Downloads
land beside the video and are picked up by themselves next time.

**Previews that show the frame you land on**, not the keyframe before it.
Measured on a 24-minute film, mapping the cursor to a preview the obvious way
put a different scene under it 60% of the time; here the thumbnail sharpens to
the exact frame the moment the cursor stops moving. Seeking is measured too: an
exact seek costs 1.9 s on one file and 0.05 s on another and nothing at all on
most, so the player times the first one and decides from the measurement instead
of trading away precision everywhere to be safe.

**Pick up where you left off, in anything.** Files, links and individual torrent
episodes all resume, behind a start screen of unfinished videos with their own
poster frames. The dub you chose in episode 1 is found again in episode 2 by
language, title and codec — never by track number, which moves between releases.

**A mini player that stays where you put it.** A small always-on-top window that
snaps to the corners and, on macOS, floats over other applications' fullscreen
spaces — the one thing an ordinary always-on-top window cannot do.

The interface is localised **Russian / English**, and every hotkey can be
rebound.

## Install

Both platforms are built and published by CI on every version bump — take the
files from
[the latest release](https://github.com/risenxxx/frame-player/releases/latest):

| Platform | File | Requirements |
|---|---|---|
| Windows | `FramePlayer_<version>_x64-setup.exe` | Windows 10/11, x64 |
| macOS | `FramePlayer_<version>_aarch64.dmg` | Apple Silicon |

macOS is also a `brew install --cask` away — see [macOS](#macos) below.

The macOS build is signed with an Apple Developer ID and notarised, so it opens
like any other application. **The Windows build is not signed**, and stops the
first launch with a SmartScreen warning — that is a fact about a certificate
rather than about the binary, which is built in the open from this repository by
[the release workflow](.github/workflows/release.yml). Updates are a separate
mechanism and are verified on both platforms: every package is signed with the
project's own key and the player refuses one whose signature does not match.

Once installed, the player updates itself — it checks for a new version at
startup and every six hours, and the update reopens the current video where it
was.

### Windows

1. Run `FramePlayer_<version>_x64-setup.exe`. SmartScreen shows a blue
   **"Windows protected your PC"** dialog with only a *Don't run* button.
2. Click **More info** — the publisher line appears, and with it a **Run
   anyway** button.
3. Click it; the installer proceeds normally.

SmartScreen is judging the file's *reputation* as much as its signature, and a
new version is a new file, so expect the warning again after an update installed
by hand. Updates applied from inside the player do not go through it.

### macOS

Download `FramePlayer_<version>_aarch64.dmg` from
[the latest release](https://github.com/risenxxx/frame-player/releases/latest),
open it and drag **Frame Player** to *Applications*.

With [Homebrew](https://brew.sh) instead:

```bash
brew install --cask risenxxx/tap/frame-player
```

The fully qualified name taps the repository on the way past, so that is the
whole installation; afterwards the cask answers to `frame-player` alone.

The cask is in [a tap of its own](https://github.com/risenxxx/homebrew-tap)
rather than in `homebrew-cask`, whose casks have to clear a popularity bar this
project has not reached; it is bumped by the release workflow, so it names the
current version within a minute of one being published. It declares the player
as self-updating, which means `brew upgrade` deliberately leaves it alone: the
player fetches its own signed updates, and Homebrew is the way in and the way
out rather than the update channel. It also declares the build as Apple Silicon
only, so an Intel machine is refused with a reason instead of receiving an
application it cannot run.

`brew uninstall --cask frame-player` removes the player and leaves watch
positions, remembered tracks and the thumbnail cache where they are; adding
`--zap` removes those too.

A build you produce yourself from this repository is not signed the way the
releases are: with no certificate it is signed ad-hoc, which seals the bundle
but certifies nothing.
macOS then refuses the first launch with an **Open Anyway** button in *System
Settings → Privacy & Security* — the path that has a way out, as opposed to the
*"is damaged"* refusal an unsealed binary gets, which has none.

## Features

**Playback**

- **Format coverage on par with VLC** — libmpv/FFmpeg decoding with hardware
  acceleration (D3D11VA on Windows, VideoToolbox on macOS), 10-bit,
  HDR10/HLG/HDR10+ and Dolby Vision profiles 5/8 via `vo=gpu-next`.
- **Frame stepping** `,` / `.` in both directions with no freeze.
- **Seeking that adapts to the file** — an exact seek costs decoding from the
  preceding keyframe, which is free on most files and expensive on a few; the
  mode is probed once per file rather than assumed.
- **Seekbar thumbnails** — hover previews decoded by a dedicated FFmpeg session,
  with a background storyboard pass, a disk cache for reopened files, and an
  exact frame fetched when the cursor comes to rest.
- **Chapters** with a chapter list, and a skip button for openings, recaps,
  previews, credits and ads.
- **A–B loop**, three-state repeat (off / all / one), playback speed.
- **Playback queue** built from the folder of the file you opened, in natural
  order, with the file names taken from container titles where they exist.

**Sources**

- **Links** — anything yt-dlp resolves, plus direct stream URLs.
- **Torrent streaming** — a magnet link becomes a playable queue served from a
  loopback HTTP server, with piece priority following the playhead, buffered
  ranges on the seekbar, embedded subtitles attached and the next episode
  prefetched. Storage is managed from inside the player, and seeding is off by
  default (a compile-time librqbit feature, not a rate limit).
- **Subtitle search** (OpenSubtitles) — matched by file hash, with a title
  search as the fallback, sign-in optional and only to raise the daily limit.

**Television**

- **Two transports, chosen for you** — Google Cast and DLNA, discovered
  together and merged into one row per device. The row says what will happen to
  *this* file on *that* device ("plays as it is", "prepared before it starts"),
  and the choice can be pinned per device.
- **No preparation where none is needed** — a renderer that lists the container
  plays the original file, so HEVC, HDR and surround survive; where a copy is
  required the video is stream-copied and only the audio re-encoded.
- **Torrents while they download** — served to the television from the same
  blocking stream that feeds the player, with a buffered lead before the load.
- **The window becomes a remote** — the queue advances the session, the seekbar
  and keys drive the television, and what a device cannot do (its own volume,
  its own audio-track choice) is said in words rather than left inert.
- **A device check** — one button produces a report of what the device answered:
  reachability, formats it accepts, whether it will seek, what it says about
  volume. Copyable, and it never starts anything on the screen.

**Interface**

- **Frameless UI** — custom title bar, auto-hiding controls over the video,
  custom tooltips, icon OSD; native window chrome and menu bar on macOS.
- **Resume playback** — every file, link and torrent episode reopens where you
  left it, behind a start screen of unfinished videos and poster frames.
- **Remembered tracks** — the audio and subtitle track you chose is scored
  against the next episode's tracks by language, title, codec and forced flag,
  and applied only above a confidence floor, so an episode with no Russian dub
  falls back to mpv's own `alang`/`slang` instead of picking something wrong.
- **Mini player** — a small always-on-top window that floats over other apps'
  fullscreen spaces on macOS, and snaps to screen corners.
- **Zoom and pan** — Ctrl+wheel anchored at the cursor, drag to pan.
- **Frame export** — save or copy the current frame exactly as the VO decoded
  it, HDR tone mapping included, with or without subtitles.
- **Media info panel** — a live readout of codecs, bitrate, color and channels.
- **Customizable hotkeys** bound to *physical* keys, so they work in any
  keyboard layout.
- **Privacy** — folders can be excluded from the watch history, and excluding
  one also erases what was already recorded for it (positions, remembered
  tracks, thumbnails on disk). History can be turned off entirely.

**Platform**

- File associations, single instance, media keys and taskbar progress on
  Windows; Apple-Event file opening and a native menu bar on macOS.
- **Auto-updates** — signed installers; the player checks a manifest at startup
  and every 6 hours and offers a one-click update that reopens the current video
  at the same position.

## Watch together

A room holds a **timeline** — what is playing, whether it is paused, where in it
and at what speed. Everyone fetches the film themselves, so no media passes
between the players and none passes through the relay.

Open the context menu → **Watch together**. Create a room and
you get a six-character code and a link; either one gets somebody else in. A
`frameplayer://join/ABC123` link opens the player straight into the join dialog
with the code filled in — it is *offered* rather than obeyed, because a custom
scheme is a surface any web page can aim at.

**What is shared, and what is not.** Position, pause, speed and which file is
playing are the room's. Volume, subtitle appearance, zoom, delays and window
state are yours. Track choices sit in between and are a *rule of the room*, set
by the host beside "only the host controls playback": the audio track is shared
by default (a room is watching one film and listening to one soundtrack) and
subtitles are not (one viewer needs them and another does not — sharing that
choice would turn them off for somebody who cannot follow the film without
them). A track travels as a *description* rather than a track number, so it
still resolves correctly when two people have different rips.

**What everybody else opens** depends on where your film came from:

| source | what happens on the other machines |
|---|---|
| torrent | the magnet and the file index travel; everyone opens the same episode themselves, and switching episode carries the room with it |
| link | the URL travels and each player resolves it |
| local file | nothing can be sent, so the room shows the name, length and release hash — open your own copy and the player says whether it is the same release, a different rip, or a different film |
| a file in a hidden folder | the timeline still syncs and the name does not leave your machine |

**Anybody can control playback** by default, and the host can turn that off. A
room pauses itself while any member is still opening a file or buffering, and
says who it is waiting for; a member who never reports stops holding it up after
45 seconds.

**The relay** is a small Go server in [`services/relay/`](services/relay/) — no database,
nothing written to disk, and a room ceases to exist a few minutes after the last
person leaves. Builds point at a default instance; the address is a field in
**Settings → General**, so running your own is a setting rather than a fork.
See [services/relay/README.md](services/relay/README.md) to deploy one and
[docs/watch-together.md](docs/watch-together.md) for the design.

## Configuration

The player reads an `mpv.conf` of its own, created with a commented template on
first run:

| Platform | Path |
|---|---|
| Windows | `%APPDATA%\app.frameplayer\mpv.conf` |
| macOS | `~/Library/Application Support/app.frameplayer/mpv.conf` |

The format and options are mpv's own (`option=value`,
[full list](https://mpv.io/manual/stable/#options)); values are applied on top of
the player's defaults at startup. Useful examples from the template:
`target-colorspace-hint=auto` (true HDR output on an HDR display),
`tone-mapping`, `audio-spdif` (bitstream to an AV receiver), `hwdec=no`.

A settings dialog (right-click → *Settings*) covers the common options and writes
to the same file surgically — manual edits, comments and unknown options are
preserved, and changes apply live. The bottom of the dialog links to the file
itself, and reports which decoder is actually in use, so a silent fallback to
software decoding is visible.

## Hotkeys

Bound to physical keys, so they work in any keyboard layout, and all of them can
be reassigned in the settings dialog. macOS defaults differ where the system or
the menu bar owns the combination.

| Key | Action |
|---|---|
| `Space` / `K` | Play / pause |
| `,` / `.` | Frame back / forward |
| `←` / `→` | −5 s / +5 s (`Shift` — ±1 s, exact) |
| `J` / `L` | −10 s / +10 s |
| `Ctrl`+`←` / `→` | Previous / next chapter (`⌃⌘` on macOS) |
| `Home` / `End` | Start / end of file |
| `0`–`9` | Jump to 0–90 % of the file |
| `↑` / `↓` / wheel | Volume ±5 |
| `M` | Mute |
| `[` / `]` | Slower / faster |
| `Shift`+`L` | Repeat mode |
| `A` / `Shift`+`A` | Mark / clear an A–B loop |
| `PgUp` / `PgDn` | Previous / next item in the queue |
| `Z` / `Shift`+`Z` | Subtitle delay ∓ |
| `Ctrl`+`-` / `Ctrl`+`=` | Audio delay ∓ |
| `F` / double click | Fullscreen |
| `P` | Mini player |
| `I` | Media info |
| `O` | Open files |
| `Ctrl`+`L` | Open a link (`⌘L` on macOS) |
| `S` / `Shift`+`S` | Save the frame / with subtitles |
| `Ctrl`+`C` | Copy the frame (`⌘C` on macOS) |
| `Ctrl` + wheel | Zoom, anchored at the cursor |
| `Ctrl`+`0` | Reset zoom |
| Drag on video | Moves the window; pans when zoomed |
| Horizontal wheel | Scrub |

## Building

Requirements: Node.js 20+, Rust (stable), and Windows 10/11 x64 or macOS on
Apple Silicon (release builds are aarch64-only; an Intel build is a second CI
entry away but is not currently produced). Binary SDKs are not committed — they are fetched into `src-tauri/{lib,ffmpeg,tools}`
by the scripts below, and the build paths are wired in
[.cargo/config.toml](.cargo/config.toml).

### Windows

```bash
npm install
npm run fetch-libs   # libmpv (LGPL) + FFmpeg SDK (LGPL, for the sidecar) + libclang (for bindgen)
npm run tauri dev    # development run
npm run tauri build  # NSIS installer
```

### macOS

macOS needs a **patched libmpv**: mpv's macOS backend does not implement `--wid`
at all, so stock libmpv opens a window of its own instead of rendering into
ours. The patch is in [patches/](patches/) (+162/−14 across four files).

The libraries are prebuilt and fetched, the same way Windows fetches its SDKs —
no Homebrew and no compiler needed to build the app itself:

```bash
npm install
scripts/fetch-macos-libs.sh     # libmpv (patched, LGPL) + FFmpeg + the dylib closure
npm run tauri:macos             # development run
npm run tauri:macos:build       # .app bundle
npm run tauri:macos:dmg         # disk image
```

To rebuild that set — when changing the patch or a pinned version — run
`scripts/build-macos-libs.sh` (Xcode Command Line Tools and Homebrew required;
it compiles FFmpeg and mpv, twenty minutes or so) and publish it through the
**macOS libs** workflow, which re-checks the licenses before uploading.

FFmpeg is built here rather than taken from Homebrew because Homebrew's is a
GPL-3 build: it enables the GPL parts and links x264 and x265, and mpv built
against it is GPL too. The player needs none of that — it never encodes video
(`-c:v copy` everywhere; only audio is transcoded, to AAC or E-AC-3, both
native) — so `--disable-gpl` plus `-Dgpl=false` costs no functionality and keeps
the distributed application LGPL, matching Windows.

### Tests

The sidecar decode paths have smoke tests. Generate a fixture with the bundled
ffmpeg first (the LGPL build has no GPL encoders, hence `mpeg4` rather than
libx264):

```bash
src-tauri/ffmpeg/bin/ffmpeg.exe -f lavfi -i testsrc2=duration=60:size=1280x720:rate=30 -c:v mpeg4 -q:v 5 test.mp4
cd src-tauri && FP_TEST_VIDEO=/path/to/test.mp4 cargo test --lib -- --nocapture
```

On macOS a test binary cannot find the bundled dylibs on its own, so add
`DYLD_FALLBACK_LIBRARY_PATH=$PWD/lib`.

The watch-together relay is its own tree and its own run — the Node gates do not
reach it, and `shared/sync-protocol.txt` is a contract only one half of which
they check:

```bash
go vet frameplayer/... && go test -race frameplayer/...  # both services, from the repo root
cd services/relay && go vet ./... && go test -race ./...
```

Testing a *room* needs two players, and the player is single-instance, so a
second `npm run tauri dev` signals the first rather than starting one.
`cmd/probe` is the other end of a room — it joins, follows the timeline and
prints where it thinks playback is, so the real player can be driven by hand and
watched from a terminal:

```bash
go run ./services/relay &
go run ./services/relay/cmd/probe -room ABC123 -drive
go run ./services/relay/cmd/probe -room ABC123 -skew 300ms   # a clock that is wrong on purpose
go run ./services/relay/cmd/probe -room ABC123 -hold 20s     # hold the room, on purpose
```

## Project structure

| Path | What lives there |
|---|---|
| `src/routes/+page.svelte` | The player UI: markup, styles, gestures, hotkeys, menus |
| `src/lib/*.svelte.ts` | State modules — mpv mirrors, watch history, window prefs, thumbnails, zoom, torrents, i18n, the hotkey table |
| `src/routes/veil/+page.svelte` | The black shutter window used to mask fullscreen transitions |
| `src-tauri/src/lib.rs` | Plugin registration, window and app lifecycle, commands |
| `src-tauri/src/thumb_service.rs` | Seekbar thumbnails: decode, background storyboard, disk cache |
| `src-tauri/src/step_engine.rs` | FFmpeg sidecar frame-stepper (kept working, disabled by default) |
| `src-tauri/src/torrent.rs` | Torrent session and the loopback HTTP server mpv opens |
| `src-tauri/src/cast.rs` · `dlna.rs` | Casting: the Cast client, the LAN file server, the UPnP transport |
| `src-tauri/src/opensubtitles.rs` | Subtitle search and download |
| `src/lib/sync/` | Watching together: the wire, the clock estimate, content identity, drift correction |
| `services/` | The Go services: `relay/` (watching together) and `tmdb/` (the metadata proxy)
| `services/relay/` | The watch-together relay (Go) and `cmd/probe`, a headless peer for testing it alone |
| `src-tauri/src/macos_*.rs` | Native window chrome and menu bar on macOS |
| `src-tauri/lua/zoompan.lua` | Atomic zoom+pan applied on mpv's core thread |
| `patches/` | The mpv `--wid` embedding patch for macOS |
| `scripts/` | SDK fetching, the macOS libmpv build, dylib bundling, DMG layout |
| `shared/` | Contracts two languages have to keep, read by both test suites |
| `docs/` | Design notes: why the architecture, the transports and the shipping story look like this |
| `external-issues-backlog/` | Upstream bugs found here, written up for filing |

[CLAUDE.md](CLAUDE.md) is the engineering state of record: how each subsystem
works and which invariants must not be broken. [docs/](docs/) is the reasoning
behind it — the measurements, the alternatives that were tried, and the dead
ends worth not repeating. Code comments and build scripts cite those documents
by name (`architecture.md`, `macos.md`, a numbered `ROADMAP` item).

## Releases

Pushing a version bump in `src-tauri/tauri.conf.json` to `main` triggers CI:
Windows and macOS artifacts are built, signed with the updater key, uploaded
together with `latest.json` to Cloudflare R2 and published as a GitHub Release.
Installed players pick the update up automatically.

The macOS bundle and its disk image are signed with an Apple Developer ID and
notarised as part of that run. The Windows installer is not code-signed yet, so
it still shows a SmartScreen warning on first run; [Install](#install) has the
way past it. The updater signature is a separate thing and is always verified,
on both platforms.

The last step of the run bumps the [Homebrew cask](https://github.com/risenxxx/homebrew-tap)
to the release that has just been published. It is last because a stale tap
hands out the previous version while a missing release is an artifact nobody can
obtain, and a step must not stand in front of something more important than
itself. The cask points at the GitHub Release asset rather than at R2, which
keeps only the five newest versions — a download that 404s is worse than a
version behind.

## License

The application is [GPL-3.0-or-later](LICENSE) licensed: you may use, study,
change and redistribute it, and anything you distribute that is built from it
has to come with the same freedoms and its source.

That is a deliberate choice rather than a default. The bundled libraries are
kept strictly LGPL (see below), which is what leaves the option of a closed or
paid build open — and a permissive license on this code would have handed that
same option to a closed fork while the original kept doing the maintenance.

Distributed builds link against **libmpv** and **FFmpeg** under the **LGPL
v2.1+**, on both platforms. They are separate dynamic libraries, so the right the
LGPL reserves for you — to replace one with your own build and keep the player
working — is a matter of swapping a file in `Contents/Resources/lib` (macOS) or
beside the executable (Windows).

Their sources: FFmpeg and mpv upstream at the versions pinned in
[scripts/build-macos-libs.sh](scripts/build-macos-libs.sh) and
[scripts/fetch-libs.ps1](scripts/fetch-libs.ps1), plus the one modification this
project makes — the macOS `--wid` embedding patch in [patches/](patches/), which
is why libmpv is built here rather than taken as a binary. Neither build enables
the GPL parts: no x264, no x265, no librubberband, and `-Dgpl=false` for mpv.
[scripts/check-macos-licenses.sh](scripts/check-macos-licenses.sh) is what
enforces that, and it runs before anything is published.

The bundle also includes libass, libplacebo, MoltenVK, LuaJIT, dav1d and others,
each under its own license. Torrent streaming uses
[librqbit](https://github.com/ikatson/rqbit) (Apache-2.0).

Every one of them, with the license each is used under and its full text, is in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) — which ships inside the
application as well, reachable from the bottom of the settings dialog. It is
generated by `npm run notices` from [licenses/](licenses/), and `npm run gates`
fails if a shipped library has no entry there or if the committed file is stale.

Frame Player is not affiliated with the mpv, FFmpeg or OpenSubtitles projects.
