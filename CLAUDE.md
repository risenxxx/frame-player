# CLAUDE.md — Frame Player

Windows and macOS video player: **Tauri 2 + Svelte 5 (SvelteKit static) + in-process libmpv** via `tauri-plugin-libmpv` (wid embedding: mpv renders into a child HWND / NSView *behind* the transparent webview; the entire UI is HTML on top). On macOS that needs a patched libmpv — stock mpv does not implement `--wid` there at all; see `patches/` and `scripts/build-macos-libs.sh`.

The UI is **localised (ru/en)** — every user-facing string goes through `t()`, see "Localisation" below. Code comments and docs are **English**.

## Where the documentation is

This file is the index and the short form of the rule book: how the code works
now, and which rules must not be broken, one line each. The **whole** of a rule —
its identifiers, its measurements and the failure it was written against — lives
in **`docs/rules/`**, and every line below that has a rule chapter links to it.
The reasoning *behind* the rules — dead ends, options tried and rejected — lives
in **`docs/`**. All three are part of the repository.

Read the chapter before touching the area it covers; the one-liner here is
enough to know a rule exists, never enough to change the code under it.

| `docs/rules/…` | The rules for |
|---|---|
| `mpv-playback.md` | Driving libmpv: state mirrors, the seek contract, the playlist and the end of a file, chapters, picture geometry, the A–B loop, audio |
| `thumbnails.md` | The seekbar storyboard and every frame decoded outside playback: the budget, the colour space, which frame a hover must show |
| `tracks-and-subtitles.md` | Choosing a track and remembering the choice, subtitle search and placement, closed captions, content languages |
| `torrents-core.md` | The session, adding a torrent, where the data lives, what may be deleted — and the macOS descriptor budget under it |
| `torrents-network.md` | The swarm: DHT, trackers, encryption, the proxy, port forwarding, and the preferences that rebuild the session |
| `torrents-playback.md` | Playing one: the queue, the buffer map, the readouts, subtitles inside a release, replacing a torrent with its re-upload |
| `casting.md` | Google Cast and DLNA: the ladders, the LAN server, the remote-control rules, what one television taught us |
| `sync.md` | Watching together: the wire, the reconciler, readiness, what a room may know |
| `catalog.md` | The metadata proxy, the indexer, and what leaves this machine when somebody searches |
| `sources-and-privacy.md` | What a source *is* against how it is reached, files arriving from the system, the watch history and the seven privacy enforcement points |
| `window.md` | The transparent window, the macOS title bar, fullscreen, the mini player, geometry, the idle and cursor rules |
| `ui-surfaces.md` | The settings sheet, the context menu, the start screen, tooltips, the OSD, and the arithmetic that places anything floating |
| `css.md` | The box model, the cascade between borrowed classes, and the rendering traps measured in both engines |
| `frontend.md` | Where state lives and which way it may depend, the split, hotkeys, localisation, what is worth a test |
| `build-and-release.md` | What ships inside the app, the LGPL/GPL obligations, signing on both platforms, the release workflow |

| `docs/…` | What it holds |
|---|---|
| `architecture.md` | Stack decisions, the embedding model and the constraint list — mandatory reading before touching mpv interop, seeking, fullscreen or zoom |
| `ROADMAP.md` | Shipped, planned and considered features; a comment saying "ROADMAP 21" means a numbered item there |
| `macos.md` | Why stock `--wid` cannot work on macOS, what the patch does, and the platform's own traps |
| `casting.md` | Casting to a television: the two transports, what each can carry, the measured limits and the decision rules |
| `watch-together.md` | A shared timeline over a small relay: why the wire carries state rather than actions, why drift is corrected with speed rather than a seek, and what a room may know about what you are watching |
| `catalog.md` | The catalog: why the TMDB key is in a service and not in the player, what the poster traffic costs and who decides where it comes from, how the indexer's fuzzy search is filtered, and the measurements that say a translation-type filter cannot be built |
| `torrents.md` | Torrent streaming: piece priority from playback, what a partial file can be used for, casting one |
| `distribution.md` | Shipping: signing, Gatekeeper, SmartScreen, updates, stores |

A finding earns a line in `docs/rules/` when breaking it would break the player;
it stays in `docs/` when its value is saving the next investigation. **The
repository is public**: nothing committed — code, comments or documents — may
name a host, an account, a machine, a person or an address on somebody's
network. Device behavior is recorded by model and by what it did, never by
whose device it was.

## Commands

```bash
npm install
npm run fetch-libs        # one-time: downloads libmpv, FFmpeg SDK, libclang into src-tauri/{lib,ffmpeg,tools}
npm run tauri dev         # dev run (vite on :1420 + cargo)
npm run check             # svelte-check — run after ANY frontend change (expect 0 errors; a few known a11y warnings are OK)
npm run gates             # check + check-runes + check-imports + css-orphans + test, in one go
npm test                  # vitest, the pure end of the codebase (see "Tests" below)
npm run check-runes       # a `$effect` at a .svelte.ts top level (throws on import), a `$state` its own file never writes, a declaration whose code left
npm run check-imports     # import cycles: no error, no warning, a module-evaluation order nobody chose
npm run css-orphans       # a CSS rule whose markup moved into another component
npm run tauri build       # NSIS installer (no updater signature locally unless TAURI_SIGNING_PRIVATE_KEY is set)
npm run set-version       # report the version in all five files; exits 1 if they disagree
npm run set-version 0.21.0   # …or `patch` / `minor` / `major`. See "Releases" below
```

The macOS bundle, and the one decision its build makes for you:

```bash
npm run tauri:macos:build     # → scripts/macos-build.sh. With no APPLE_SIGNING_IDENTITY in the
                              # environment it signs ad-hoc and turns the hardened runtime OFF —
                              # see the macOS bullet below for why those two are one decision
scripts/macos-sign.sh         # signs the ~35 dylibs + the ffmpeg CLI; run BEFORE the build
scripts/macos-notarize.sh <p> # notarise + staple a .app or a .dmg (and repack .app.tar.gz)
```

A signed build locally is the same three commands with the identity exported, which is also the
cheapest way to test a change to any of them without spending a release on it:

```bash
export APPLE_SIGNING_IDENTITY="Developer ID Application: … (TEAMID)"
export APPLE_API_KEY=… APPLE_API_ISSUER=… APPLE_API_KEY_PATH=…/AuthKey_….p8
scripts/macos-sign.sh && npm run tauri:macos:build
scripts/macos-notarize.sh "src-tauri/target/release/bundle/macos/Frame Player.app"
```

Rust validation while the user's dev instance may be running (target/ is locked by the running exe):

```bash
cd src-tauri && CARGO_TARGET_DIR=/some/tmp/target-check cargo check
```

Sidecar smoke tests (both engines: stepping + thumbnails). Generate the fixture with the bundled ffmpeg:

```bash
src-tauri/ffmpeg/bin/ffmpeg.exe -f lavfi -i testsrc2=duration=60:size=1280x720:rate=30 -c:v mpeg4 -q:v 5 test.mp4
cd src-tauri && FP_TEST_VIDEO=<abs path to test.mp4> cargo test --lib -- --nocapture
```

(The LGPL ffmpeg build has **no GPL encoders** — use `mpeg4`, not libx264.)

A colour change cannot be checked by an assertion — a thumbnail of the right size in the wrong hue passes every one that can be written — so `thumb_smoke` takes the positions and writes the JPEGs out for looking at:

```bash
cd src-tauri && FP_TEST_VIDEO=<abs path> FP_TEST_POS=600,1200,2100 FP_TEST_DUMP=/tmp/thumbs \
  cargo test --lib thumb_smoke -- --nocapture
```

Torrent streaming against a real swarm (ROADMAP 21's spike, kept as a test — off
by default because it needs the network, live seeders and up to a minute):

```bash
cd src-tauri && FP_TEST_MAGNET=1 cargo test --lib torrent::tests::sintel_smoke -- --nocapture
```

And its sibling for when a torrent does *not* download — a minute of peers seen
vs peers connected vs bytes, then per-peer counters (attempts, errors, chunks).
`sintel_smoke` can only report that as a read which never returns; this says why:

```bash
cd src-tauri && FP_TEST_SWARM='magnet:?xt=…' cargo test --lib torrent::tests::swarm_probe -- --nocapture
```

The Go services (`services/`) are their own tree and their own test run — Node's
gates do not touch them. The workspace at the repository root is what lets one
command cover both; note the pattern is `frameplayer/...` and never `./...`,
because the root is not itself a module:

```bash
go vet frameplayer/... && go test -race frameplayer/...
go run ./services/relay                                  # :8080
go run ./services/relay/cmd/probe -play -drive           # a second peer, so one machine is enough
```

Casting smoke test against whatever Cast devices the LAN has (browse + TLS +
receiver GET_STATUS; deliberately never launches an app on the device):

```bash
cd src-tauri && FP_TEST_CAST=1 cargo test --lib cast::tests::discover_smoke -- --nocapture
```

The unicast half of discovery, no multicast involved — one address, or one whole
round over the LAN with its timing (`sweep`, or an address instead of it):

```bash
cd src-tauri && FP_TEST_CAST_IP=sweep cargo test --lib cast::tests::unicast_probe_smoke -- --nocapture
```

On macOS a test binary in a custom target dir cannot find the bundled dylibs
(`@executable_path/lib` resolves next to `deps/`, where build.rs did not copy
them), so any `cargo test` there also needs `DYLD_FALLBACK_LIBRARY_PATH=$PWD/lib`.

## Layout

| Path | What lives there |
|---|---|
| `src/routes/+page.svelte` | What is left after the split: hotkeys, the fullscreen veil, the idle/cursor rules, the wheel dispatch, the StepEngine overlay, and the `onPlayerProperty`/`onFileLoaded` bridge that wires mpv state to the UI. It composes the components below rather than drawing them |
| `src/lib/components/` | The UI, one file per surface: `Dialog` (the shell all seven dialogs share), `SettingsDialog`, `SubsDialog`, `LinkDialog`, `MediaInfoDialog`, `DiagnosisDialog`, `TorrentPickDialog`, `TorrentUpdateDialog`, `StartScreen`, `EndScreen`, `ContextMenu`, `TopBar`, `Osd`, `Tooltip`, `SkipButton`, `LoadingOverlay`, `CastScreen`, `StepOverlay`. **A rule and the markup it styles must live in the same file** — Svelte scopes `<style>`, and the compiler's `unused CSS selector` warning does *not* catch a rule left behind, because a `<Child />` tag is opaque to that analysis. `npm run css-orphans` is what catches it, in both directions — a rule left behind, and the mirror image, a class whose **base** rule stayed in another component while one of its users moved out. The second half is the one a diff of the built stylesheet structurally cannot see, because such a comparison has to normalize the scope hash away to be readable at all, and to it `.speedopt.svelte-a` and `.speedopt.svelte-b` are the same rule — which is how five controls (the delay stepper's −/+, three dialog footers, the external-subtitle rows) came to render as unstyled native widgets under a clean report |
| `src/lib/input.svelte.ts`, `overlays.svelte.ts` | Everything the viewer does with a key, a wheel or a pointer, and what is on top of the video. `input` decides *which* thing was asked for; who answers it is `playback`'s (see the row below), which is why this file no longer spells the casting rule out — the one branch it keeps is the click, where the two transports want genuinely different gestures rather than the same gesture aimed elsewhere. `overlays` is the surface stack: at most one OSC menu, and Escape unwinding innermost first. Its existence is what lets `input` take **no hooks from the page at all** — everything that reacts to an overlay was reaching for a different variable before |
| `src/lib/resume.svelte.ts`, `tracks.svelte.ts` | The two things a file needs before it can play the way the viewer left it. `resume` starts it at the remembered position — via mpv's **`start` option before `loadfile`**, not a seek afterwards, which is why the `beforeLoad` hook is awaited — and holds the seekbar honest meanwhile: `barDuration` borrows the remembered length until mpv reports its own (display only; seek arithmetic must keep using `player.duration`), and `seek.settling` pins the knob because `time-pos` reports zero for the whole of that window. `tracks` is the remembered audio/subtitle choice and the per-file delays, which share a module for the same reason they share a store — both answer "how does this source have to be played", and a manual pick has to cancel a restore still waiting for its track. Its restore is a **standing effect**, not a one-shot on `file-loaded`: external subtitles arrive a beat later |
| `src/lib/open.svelte.ts` | How a source becomes a playing file: the link box, torrents (resolve, pick, the start-screen list, replacing one with its re-upload) and the indicator that says something slow is under way. **One module rather than the two it was planned as**, because the halves depend on each other by design — a magnet is pasted into the box that takes links, so `submitLink` dispatches into the torrent flow, and the torrent flow reports its failures back through `linkBox.torrentError`, which raises that same dialog for a dropped `.torrent` that has nowhere else to speak. A boundary between them would be a cycle with a hook across it. It composes `loadFiles`, the queue and the position store rather than reimplementing any of them, and takes the one thing it cannot reach — the file-dialog dimming flag — through `initOpen` |
| `src/lib/chrome.svelte.ts`, `endscreen.svelte.ts`, `step-engine.svelte.ts`, `updater.svelte.ts` | What came out of the page's script. `chrome` is the window shell — fullscreen and its veil, the title bar, the window buttons, the resize edges and the idle rules; it is one module rather than two because the chrome's visibility *is* the idle state. `endscreen` is the end of a file and the skip offer, which belong together because on the last chapter one becomes the other and `Enter` answers either. Each of them takes its inputs (`chrome.overlayOpen`, `chrome.sheetOpen`) rather than importing a dialog, and each exposes an `initX()` for its standing effects — see the `$effect` rule in [rules/frontend.md](docs/rules/frontend.md) |
| `src/lib/playback.svelte.ts` | **Playback as one thing, whichever machine is decoding it.** Position, duration, paused, volume, the current chapter and a `can` record of what the session supports, plus the verbs (`togglePause`, `seekBy`, `seekFraction`, `jumpToChapter`, `stepChapter`, `advance`, `openEntry`, `setVolume`, `nudgeVolume`, `toggleMute`) that route themselves. It exists because the rule "while `cast.remote`, read `cast.time` rather than mpv's" is one that has to be *remembered*, and forgetting it is silent: mpv is parked **paused** on the frame it handed over, so its mirrors do not look wrong, they look frozen at a plausible value. Five places had been written without it and each was found by a viewer — the chapter list, the skip button, its guard, the Windows taskbar and the digit jumps. Two things it deliberately does not do. It does not cover the **seekbar gesture**: a local drag is a stream of preview seeks with the exact/keyframe probe behind it and a remote one is a single command on release, so hiding that would leave the caller unable to see what a gesture costs — those branches stay written out in `seek.svelte.ts`, which therefore stays *below* this module (importing it there is a cycle through `step-engine`). And it does not route hotkeys case by case: `CAST_BEHAVIOR` is a `Record<ActionId, …>`, so **a new action is a compile error until somebody classifies it** — the same device as `SKIP_LABEL`, and the answer to a bug class that was entirely omissions. Everything routable is a verb rather than a table entry, because a verb serves the buttons and the menus as well as the key; `refusedBySession` only says no |
| `src/lib/seek.svelte.ts` | Everything that moves the playhead — seekbar drag, wheel scrub, arrows, digit jumps — and the per-file exact/keyframe probe they share. Moved as one unit on purpose: this is the most invariant-dense code in the player and the least covered by tests. It reaches the frame-stepper through `initSeek` hooks so it need not know the sidecar exists |
| `src/app.css` + `src/routes/+layout.svelte` | Document-level styles: the `box-sizing: border-box` reset, fonts, focus rings, the `button, input` font reset, and the `.scrollable` scrollbar skin. They live outside a component because Svelte scopes `<style>` — a rule written in the page stops applying the moment its markup moves into a child component |
| `src/lib/player.svelte.ts` | mpv state mirrors, `initPlayer` (options + observers + events), commands, tracks, file loading. Knows nothing about the UI; the page subscribes through hooks |
| `src/lib/history.svelte.ts` | Watch positions, "continue watching", privacy roots, the update-resume snapshot |
| `src/lib/window-prefs.svelte.ts` | Geometry persistence, fit-to-video, always-on-top, macOS menu check marks |
| `src/lib/thumbs.svelte.ts`, `zoom.svelte.ts`, `screenshot.ts`, `osd.svelte.ts`, `format.ts`, `platform.ts` | Hover thumbnails; zoom/pan maths; frame export; the OSD popup; pure formatting helpers; `IS_MAC` |
| `src/lib/latest.ts` | "Only the newest attempt may write." A counter behind `latest()` → `begin()` → `run.stale`, guarding the six places where a read outlives what it was about — the track and chapter lists, the queue read, the recents rail, the UPnP probe and the torrent poll. **Clearing a timer does nothing about this**: a promise already in flight resolves regardless, and a slower earlier attempt then lands last and overwrites the newer answer, permanently, since nothing is left to correct it. It had been hand-written in all six, and what a copy loses is never the counter — it is a check after the *second* await, which is why the shape makes each check one word |
| `src/lib/i18n.svelte.ts` | ru/en dictionaries, `t()`, `locale()`/`setLocale()`; `ru` defines the key set, so a missing English string is a type error |
| `src/lib/languages.ts` | The **content** languages (ROADMAP 25) — one table of 639-1 code + endonym + the 639-2 spellings, from which `LANG_ALIASES`, the `alang`/`slang` picker and the subtitle-search filter are all derived. Nothing to do with `i18n.svelte.ts`, which is the *interface* language |
| `src/lib/keys.svelte.ts` | The hotkey table: `ACTIONS` (36 rows, per-platform defaults, `noRepeat`/`quiet`), chord normalization and labels, the localStorage overrides, and `hint()`/`hintPair()` that every tooltip and menu row reads |
| `src/routes/veil/+page.svelte` | Black shutter window content (fullscreen transition masking) |
| `src-tauri/src/lib.rs` | Plugin registration, window/app lifecycle, commands: `initial_files`/`open_file_ready` (see "Opening files" below), `zoom_pan`, `user_mpv_conf`, `mpv_conf_set`, `double_click_time` (Win: `GetDoubleClickTime`; mac: `NSEvent.doubleClickInterval`, default 0.5 s), `hdr_status` (Win: per-monitor HDR, 24H2 ACM-aware; mac: `macos_chrome::edr_headroom` — there is no HDR on/off switch on macOS, only a *dynamic* EDR headroom that collapses to ~1.0 when SDR brightness is maxed, so `enabled` legitimately fluctuates) |
| `src-tauri/src/step_engine.rs` | FFmpeg sidecar frame-stepper (GOP ring). **Disabled** via `USE_STEP_ENGINE=false` in +page.svelte; kept working + smoke-tested |
| `src-tauri/src/thumb_service.rs` | Seekbar thumbnails: seek to keyframe → decode forward to the exact grid position *only when the keyframe would duplicate the neighbouring cell* (files whose GOP is longer than the grid step used to show one frame for seconds of seekbar) → JPEG, background storyboard, disk cache (`app_cache_dir/thumbs`, magic-versioned, LRU-pruned to 256 MB on every save). A hover maps to the **nearest** cell, and a *resting* cursor gets the frame at its exact position instead of the cell's (`exact` flag on `thumb_get`) — see "The preview must show the frame you land on" below |
| `src-tauri/src/color.rs` | Turning a decoded frame into something a person can look at: the PQ and HLG transfer functions, the IPT inverse Dolby Vision profile 5 needs, the exposure anchors and BT.2020 → BT.709. Pure arithmetic and unit-tested, because every one of its errors is a *plausible* picture in the wrong colour — see "A preview is a decoded frame, and a decoded frame has a colour space" below |
| `src-tauri/src/torrent.rs` | Torrent streaming: the lazy librqbit session, the loopback HTTP server mpv opens (`/t/<infohash>/<index>/<name>`, Range + HEAD), and `torrent_add`/`torrent_status`/`torrent_release`/`torrent_clear_cache`. Frontend half in `src/lib/torrent.svelte.ts` |
| `src-tauri/src/feed.rs` + `src/lib/feed.ts` + `FeedPickDialog.svelte` | Torrent RSS feeds: `feed_read` (RSS/Atom in their tracker dialects), `feed_torrent` (fetch an item's `.torrent` into the metadata cache), and the pure matching that decides which item is a re-upload of which release. See "An RSS feed is a link to a release" below |
| `src/lib/sync/` | Watching together, in a handful of pieces with a direction that is load-bearing. `protocol.ts` is the wire and a **leaf** (its field lists are compiler-proved complete and compared against `shared/sync-protocol.txt`); `clock.ts` is the offset estimate; `wire.svelte.ts` is the socket, the room and `publish()` — also a leaf, which is what lets `playback` and `seek` publish without a cycle; `content.ts` says what is playing and is the **only** place a `ContentRef` is built (hence the privacy gate); `drift.ts` is the three correction bands; `ready.ts` is whether this player is holding the room up, pure and tested because both ways of being wrong are silent — a member stuck "loading" freezes the film, one who claims to be ready while nothing is on their screen is left behind; `apply.svelte.ts` sits **above everything** and is what applies an arriving timeline, opens the room's content and runs the reconciler. Publishing goes down into the leaf, applying goes up — see `docs/watch-together.md` |
| `services/` | The Go services, one directory each, deployed apart and developed together behind the root `go.work`. `relay/` is watching together; `tmdb/` is the metadata proxy the catalog reads (README there for the capacity numbers and why the key lives in it) |
| `services/relay/` | The relay: rooms, a shared timeline, nothing else. Go, one dependency, no database, nothing written to disk. Content travels as opaque JSON and is never parsed there, so a new kind of source never means a redeploy. `cmd/probe` is a headless peer, which is how this is tested without a second machine |
| `src-tauri/src/lan_sweep.rs` | Finding a television multicast does not reach: the sweep's address list (remembered hosts, then the private LAN subnets, tunnels and VM switches skipped) and the paced datagram send DLNA's unicast `M-SEARCH` uses. The Cast half (`probe_cast`: `:8009`, then `eureka_info`) lives in cast.rs |
| `src-tauri/src/net_route.rs` | Which interface the swarm leaves by: the interface list (getifaddrs + SystemConfiguration on macOS, `GetAdaptersAddresses` on Windows), which of them is a tunnel, which is the way out past it (`Route::Direct`), and where the system route goes right now — a UDP `connect` to a public address, which sends nothing. It routes nothing itself; librqbit's `bind_device_name` does |
| `src-tauri/src/upnp.rs` | Asking the router whether the BitTorrent port is really forwarded: SSDP for the gateway, then `GetSpecificPortMappingEntry` for the mapping librqbit made. A read, never a write — the mapping is librqbit's to renew. Exists because its forwarder is a `run_forever` task that reports to nobody, so the setting would otherwise be a claim rather than a fact |
| `src-tauri/vendor/` | Three librqbit crates vendored via `[patch.crates-io]` (its README has the details, and is the thing to read before bumping librqbit). `librqbit-dualstack-sockets` carries the Windows half of binding a socket to an interface, which is what "past the VPN" rests on there — see `net_route.rs`. `librqbit-dht` carries a dozen lines so the DHT does not die on a UDP recv error, which Windows produces routinely — see "Windows kills the stock DHT" below. `librqbit` carries **Message Stream Encryption**, taken whole from an unmerged upstream PR and to be dropped the day it lands — see "A network can refuse a torrent by where it is going or by what it is". The `librqbit-tracker-comms` copy that used to be here went back to the registry with the bump to 9.x, where all three of its fixes now live |
| `src-tauri/src/opensubtitles.rs` | Subtitle search and download: the API key's lookup order, the OpenSubtitles file hash, `subs_search` (hash and/or title, privacy-gated) and `subs_download` (ticket → file → saved beside the video) |
| `src-tauri/src/catalog.rs` + `src/lib/catalog.svelte.ts` + `CatalogDialog.svelte` | Finding something to watch: TMDB for *what* (posters, localised titles, seasons) and a Torznab-compatible indexer for *where from* (trackers, quality, dubs, seeders, a magnet). On by default with a switch to turn it off; the metadata proxy and the indexer are both settings. See "The catalog" below |
| `src-tauri/src/cast.rs` | Casting to TV (Google Cast): mDNS browse (`mdns-sd`, on-demand), a hand-rolled CASTV2 client (TLS to the device's 8009, 4-byte-BE-framed protobuf envelopes with JSON payloads, tokio `select!` pump + dedicated reader task), and the LAN file server (one registered file behind a random token, bound to the interface on the device's subnet). Frontend half in `src/lib/cast.svelte.ts`; see "Casting" below and `casting.md` |
| `src-tauri/src/screenshot.rs` | Frame export: destination dir (created on demand), temp path + clipboard hand-off. mpv writes the PNG; this only supplies what mpv cannot do |
| `src-tauri/src/window_guard.rs` | Clamp window into visible monitor area at startup |
| `src-tauri/lua/zoompan.lua` | Atomic zoom+pan on mpv's core thread (copied next to the dev exe by build.rs; bundled via resources) |
| `licenses/` + `THIRD-PARTY-NOTICES.md` | The attribution that ships inside the app: `manifest.json` (one entry per third-party project, with the license of the artifact **as built**), `text/` (each project's own license, verbatim), `spdx/` (canonical texts for the ids the Rust and npm graphs declare). `scripts/gen-notices.mjs` renders the notices; `npm run notices:check` is a gate |
| `scripts/fetch-libs.ps1` | Downloads all binary SDKs (also used by CI with an actions/cache keyed on this file's hash). Its two `releases/latest` lookups **must be authenticated in CI**: the anonymous GitHub API allowance is 60 an hour *per IP*, a hosted runner shares its address, and a release therefore fails on traffic that is not ours — `GITHUB_TOKEN` (automatic in Actions, optional locally) takes it to 1000. The header goes on the API calls only, never on an asset download: those redirect to a storage host that rejects a GitHub `Authorization` header, and PowerShell forwards headers across redirects |
| `scripts/macos-{build,sign,notarize}.sh` | The macOS signing chain, in the order they run: `sign` puts the Developer ID on the native libraries (and, in CI, imports the certificate into a throwaway keychain), `build` decides whether the hardened runtime goes on at all, `notarize` submits and staples an `.app` or a `.dmg` and repacks the updater archive around the stapled bundle. Each one is a no-op or an ad-hoc fallback without `APPLE_SIGNING_IDENTITY`, so a checkout with no Apple account still builds |
| `.github/workflows/release.yml` | Version-bump-gated release: build → sign → notarise → R2 upload (installer + latest.json) → GitHub Release |
| `site/` | The landing page: Astro, static output, Cloudflare Workers static assets. Its own npm project and its own workflow (`site.yml`) — the root gates do not reach it, exactly as they do not reach `services/`. Every mock on the page is one of the application's own surfaces rebuilt in HTML, with the glyphs and the measurements taken from `src/lib/components/` rather than from a screenshot; the frames inside them are placeholders and `site/README.md` says what has to happen to them before the site is announced. `site/assets/img` holds one source per picture and `scripts/images.mjs` (`npm run images`) builds the AVIF/WebP/JPEG ladder into `public/gen` under hashed names, which is what lets that path be cached for a year. The output and `src/img-manifest.json` are **committed** — a clean encode outlasts the Site workflow's ten minutes — and `typecheck`/`build` only run it with `--check`, which fails when they are out of date. Beyond the home page are the guide pages — one MDX file each in `site/src/content/pages/`, rendered by one layout; how to add one is in `site/README.md` |

## Critical gotchas (violating these causes crashes or subtle breakage)

1. **libmpv-wrapper bugs (upstream, unfixed)** — see architecture.md for details:
   - Never `getProperty(..., 'node')` and never observe properties in `node` format → stack corruption crash. Use string/flag/int64/double; read track-list via `track-list/N/...` sub-properties — and observe `track-list/count` (int64) to know *when* to re-read it, because tracks keep appearing after `file-loaded` (external subs via `sub-auto`, lazily parsed containers): reading once on that event left the subtitles button missing until something else happened to refresh the list. `loadTracks()` is a long chain of sequential reads with several possible triggers, so it needs a sequence guard or a slow earlier run finishes last and overwrites the fresh list.
   - Never send broadcast `script-message` → AV crash in the wrapper's client-message serializer. Only targeted `script-message-to <script> ...`.
2. **ffmpeg-the-third seeking**: always `ictx.seek(ts, ..=ts)` (inclusive). `..ts` silently fails with EPERM.
3. **mpv state mirrors go stale** (event queue overflow drops property-change events). Toggles must use `command('cycle', [prop])`, never `setProperty(prop, !mirror)`. Mirrors are resynced every 1 s and on `queue-overflow` in `resyncState()`, and **which** mirrors is `RESYNC`, a `Record<ObservedName, boolean>` — a newly observed property is a compile error until somebody says whether losing its event is survivable. Both paths into the mirrors go through one `applyProperty`; the `property` hook stays outside it. → [mpv-playback](docs/rules/mpv-playback.md)
4. **Multiple mpv properties are never visually atomic** from outside — even back-to-back Rust `set_property` calls let the VO redraw in between. If several properties must land in one frame (zoom+pan), go through `zoompan.lua`-style script handlers.
5. **Seekbar**: the original rule was "click = one `absolute+exact`; drag = `absolute+keyframes` only, *including the final one*", because a trailing exact seek after keyframe previews caused a frame-blink. That has since been deliberately narrowed — see "Seek flags are a performance contract" below for what replaced it and why. If the blink ever returns, `onSeekUp` is the first place to look.
6. **Fullscreen transitions**: order matters — set veil state → wait double-rAF + ~30 ms (frame must be presented) → show shutter window → `setFullscreen`. Do not "simplify" this sequence; each step masks a distinct Windows artifact.
7. **Hidden `veil` window keeps the app alive** — main-window `Destroyed` explicitly calls `app.exit(0)` in lib.rs. Removing that resurrects zombie processes that swallow Explorer "open with" requests via single-instance.
8. **Hotkeys use `e.code`** (physical keys) so they work in any keyboard layout. Keep it that way — the editor stores codes for the same reason. They live in `ACTIONS` in [keys.svelte.ts](src/lib/keys.svelte.ts), not in a `switch`: `onKeydown` resolves a chord to an action and calls `runAction`, and every place that *shows* a key reads `hint()` from the same table. Two defaults worth knowing: `L` is seek-forward (the J/K/L trio) and **loop is `Shift+L`** — mpv's own binding; the A–B loop that would have taken mpv's plain `l` therefore lives on `A` (`Shift+A` clears it). `noRepeat` and `quiet` are fields of the *action*, not lists of key codes, because neither is a property of a key once the key can be moved.
9. **A bar's gradient tail must not be part of its hit area.** `.topbar` and `.osc` extend well past their controls to fade the scrim, and while that padding belonged to the element it swallowed clicks on the video. The gradients live in `::before` with `pointer-events: none` (`.osc::before` also needs `z-index: -1` plus `isolation: isolate`), and the elements are sized to their content. → [css](docs/rules/css.md)
10. **The window is transparent, so "nothing painted" means "desktop visible"** — mpv renders *behind* the webview, so any moment when neither party paints is a hole through the window. `videoReady`/`.player.backdrop` keeps a dark fill until mpv's VO comes up, and on macOS the NSWindow carries an opaque `#101016` `backgroundColor` for the frames before the webview's first composite — never paired with `setOpaque(true)`. → [window](docs/rules/window.md)
11. **Buttons/inputs get `font-family: inherit`** via a reset in +page.svelte — form controls don't inherit fonts by default and silently fall back to the system font.
12. **HMR resets JS mirrors** (videoW/videoH etc.) while mpv keeps state and won't re-fire unchanged property events — after editing +page.svelte, reproduce bugs with a full dev restart, not just HMR.
13. **tauri.conf.json / capabilities / icons changes need a dev restart** (and icons are tracked by build.rs `rerun-if-changed`); frontend-only changes hot-reload.
14. **Code after `updater.downloadAndInstall()` never runs on Windows** — the NSIS installer kills the process inside that call. Anything that must survive the update (e.g. the `frameplayer.resume` snapshot) has to be persisted *before* install starts (see `saveResumeSnapshot()`); `relaunch()` after it is effectively dead code on Windows.
15. **Never move the macOS traffic lights by hand — grow the title bar instead.** There is no positioning API, and setting the buttons' frames is a measured dead end. An empty `NSToolbar` + `NSWindowToolbarStyle::Unified` grows the title bar 28 → 66 pt and AppKit places the buttons itself, tracking rects included. It is removed on `WillEnterFullScreen` and restored on `DidExitFullScreen` (**not** `Will`), visibility comes from the frontend's recorded intent rather than the buttons' own `isHidden`, and the window is forced to `darkAqua`. → [window](docs/rules/window.md)
16. **A file descriptor is not a renewable resource on macOS, and running out of them does not fail like running out of something.** launchd gives a GUI application a soft `RLIMIT_NOFILE` of **256**, and exhausting it poisons `AudioComponentInstanceNew` and `VTIsHardwareDecodeSupported` **for the life of the process** — which reads as a video that never plays, with nothing in any log to connect it to a limit. `raise_fd_limit()` is the first statement of `run()`, and `torrent_storage.rs` opens a torrent's files lazily under a process-wide budget. → [torrents-core](docs/rules/torrents-core.md)

## Conventions & flows

Each line is the rule; the chapter it links to is the whole of it.

- **mpv option surface**: defaults are set in `initialOptions` in +page.svelte; user overrides come from `%APPDATA%/<identifier>/mpv.conf` (command `user_mpv_conf`, template in lib.rs, `wid` filtered out). → [ui-surfaces](docs/rules/ui-surfaces.md)
- **Returning to the start screen is a destination, not a blank to wait out.** `showEmpty` is debounced by 300 ms because a playlist transition blanks `filename` for a moment and the picker must not flash through it. → [ui-surfaces](docs/rules/ui-surfaces.md)
- **The end of a file is a moment the player owns, and that is a deliberate inversion.** `keep-open=always` (not mpv's `yes`, which stops only on the LAST playlist entry and advances silently everywhere else) parks mpv on the final … → [mpv-playback](docs/rules/mpv-playback.md)
- **A file's name in the queue comes from three places, and only the third costs anything.** mpv fills `playlist/N/title` from `media-title`, but **only for entries it has actually opened** — so everything ahead of the playhead was … → [mpv-playback](docs/rules/mpv-playback.md)
- **A queue is built from one opened file, never from several.** Dropping five files is an explicit selection and stays exactly what was given; opening a single episode is the case where the rest of the folder is wanted. → [mpv-playback](docs/rules/mpv-playback.md)
- **Whether the sidecar shares mpv's FFmpeg depends on the platform, and the prefix on a log line tells you who is talking.** On macOS `src-tauri/ffmpeg-macos/lib/*.dylib` are **symlinks into `src-tauri/lib/`** (verified with … → [thumbnails](docs/rules/thumbnails.md)
- **Background decoding competes with playback — budget it explicitly.** The thumbnail storyboard decodes in *software*, so it degrades the player identically whether mpv's hwdec is on or off; that symmetry is the tell. → [thumbnails](docs/rules/thumbnails.md)
- **Seek flags are a performance contract, not a detail**: `hr-seek=yes` makes any seek without an explicit flag *exact*, and that cost is a property of the **file** — so it is probed once per file (`issueSeek` times the first one, `fileSlowSeek`/`wantExact()` decide from then on, `resetSeekProbe()` on `path`), never switched mid-gesture. → [mpv-playback](docs/rules/mpv-playback.md)
- **A preview is a decoded frame, and a decoded frame has a colour space — which the thumbnail service used to ignore entirely.** `scaling::Context::get` was given the two pixel formats and nothing else, so swscale fell back to its … → [thumbnails](docs/rules/thumbnails.md)
- **The preview must show the frame you land on, and both halves of that were wrong.** The hover thumbnail comes from a grid whose step is `clamp(duration/240, 0.25, 10)` — 10 s for anything longer than 40 minutes. → [thumbnails](docs/rules/thumbnails.md)
- **Frame export and external tracks go through mpv, not around it.** `screenshot-to-file` keeps exactly what the VO decoded, HDR tone mapping included — which is precisely what the StepEngine canvas path got wrong, so do not … → [mpv-playback](docs/rules/mpv-playback.md)
- **A subtitle found by the file's hash and one found by its title are different results, and the panel says which.** The hash (size + every 64-bit word of the first and last 64 KiB) identifies a *release*, so what comes back is … → [tracks-and-subtitles](docs/rules/tracks-and-subtitles.md)
- **Closed captions are a subtitle track that isn't in the container.** `eia_608`/`eia_708` captions ride inside the video stream (SEI), so `ffprobe -select_streams s` shows nothing while mpv lists `--sid=1 (eia_608)` — do not … → [tracks-and-subtitles](docs/rules/tracks-and-subtitles.md)
- **The subtitles get out of the control bar's way, and `sub-pos` is the only lever measured to work.** While the chrome is up the last line of dialogue sits behind the seekbar — exactly while it is being read, since what raised … → [tracks-and-subtitles](docs/rules/tracks-and-subtitles.md)
- **Every list property is read through sub-properties, and they do not all behave alike.** `track-list`, `chapter-list`, `playlist` and `audio-device-list` are all implemented on mpv's side by the same helper, which exposes … → [mpv-playback](docs/rules/mpv-playback.md)
- **"Skip intro" matches the whole chapter title, never a substring.** `skipKind()` offers a skip button for the first `SKIP_WINDOW` seconds of a chapter that names itself an opening, a recap, a preview, a trailer, the credits or … → [mpv-playback](docs/rules/mpv-playback.md)
- **The media info panel is a live readout, not a snapshot, and half of it is legitimately missing.** Every field comes from a plain `getProperty` — measured on a 4K HEVC file, `video-codec` gives the readable name ("H.265 / … → [mpv-playback](docs/rules/mpv-playback.md)
- **Picture geometry is per file, and reset only if we touched it.** `video-rotate`, `video-aspect-override` and `panscan` are global mpv options: a phone clip turned upright stays turned for the next episode, so they are reset on … → [mpv-playback](docs/rules/mpv-playback.md)
- **The A–B loop is mpv's to run; ours only to mark.** Both points set means mpv loops the segment by itself — measured, playback stays inside the range and wraps without anything watching the position. → [mpv-playback](docs/rules/mpv-playback.md)
- **A control's actions belong to the control, not to the list around it.** The delay stepper's reset used to be a `.menu-item` under the −/+ pill, which is correct for a list row and wrong beside a control: a menu row is … → [ui-surfaces](docs/rules/ui-surfaces.md)
- **The settings dialog has two control shapes, and the choice between them is about the value, not the layout.** A pill (`.segmented`) picks one of several *named* values; a slider covers a continuous range. → [ui-surfaces](docs/rules/ui-surfaces.md)
- **Volume leveling is a labeled `af` filter, and its state is ours.** `af add @fpnorm:lavfi=[dynaudnorm=…]` / `af remove @fpnorm` — the label is what lets it be removed again without touching an `af` line from the user's own … → [mpv-playback](docs/rules/mpv-playback.md)
- **`hwdec-current` is shown in the settings footer** — a silent fallback to software decoding is otherwise invisible and is the first thing to check when seeking feels slow.
- **Everything the player needs at runtime is bundled; yt-dlp is the single exception.** `src-tauri/lib` carries libmpv, the wrapper and every transitive dependency (~65 MB and ~50 dylibs on macOS: FFmpeg, libass, libplacebo, … → [build-and-release](docs/rules/build-and-release.md)
- **A slow operation must say so before it starts, not after it finishes.** Opening a URL and encoding a screenshot both used to pass in silence; `showOsd`'s `sticky` is what raises a popup that cannot fade before the work ends, and anything raising one owes it a replacement on every exit path. → [ui-surfaces](docs/rules/ui-surfaces.md)
- **An `end-file` error is not a failed attempt.** mpv's ytdl hook raises one on the way to succeeding — it opens the URL, gives up on it and loads what yt-dlp resolved instead — and playlist transitions do the same. Reporting on the event threw an error dialog over videos that were about to play, which is worse than saying nothing. The verdict now waits `LOAD_FAIL_GRACE` and `onFileLoaded` cancels it: if anything opened, whatever failed on the way was a step and not an outcome.
- **No storyboard means no space for one.** With thumbnails gated for streams the hover popup was still reserving its 180px frame and filling it gray, which reads as a preview that broke rather than one that was never coming. → [thumbnails](docs/rules/thumbnails.md)
- **A torrent is a network source that steers itself, and both halves of that are the design.** A magnet becomes a list of files, each served from our own loopback HTTP server at `/t/<infohash>/<index>/<name>`; mpv opens that URL … → [torrents-core](docs/rules/torrents-core.md)
- **Windows kills the stock DHT, and the tracker was never a backup — two librqbit bugs whose combination left Windows with no peer source at all.** The symptom read as a network problem (magnet never resolves; a `.torrent` lists … → [torrents-network](docs/rules/torrents-network.md)
- **The HTTP announce was dead on every platform, and the DHT hid it.** Two more vendored fixes, found together and each fatal on its own. → [torrents-network](docs/rules/torrents-network.md)
- **A network can refuse a torrent by where it is going or by what it is, and the player can answer only the first.** That distinction is what decides everything below, and it was reported as one bug: a viewer whose VPN forbids … → [torrents-network](docs/rules/torrents-network.md)
- **Past the VPN is a choice of interface, not of route.** `SessionPrefs::route` (`auto` — the default, the system's choice — / `direct` / one named interface) becomes `bind_device_name`, which scopes every socket librqbit opens; Windows needs the vendored `librqbit-dualstack-sockets` (`IP_UNICAST_IF`). `direct` is re-resolved per session, a route that cannot be honoured refuses the torrent rather than falling back, and "is the route a tunnel" is asked of the system (`net_route.rs`), never guessed. → [torrents-network](docs/rules/torrents-network.md)
- **Five preferences are baked into the session and one comparison decides whether it can be reused.** Seeding, the port mapping, the proxy, the encryption mode and the route are all fixed when `Session::new_with_opts` runs, so changing any of them tears the … → [torrents-network](docs/rules/torrents-network.md)
- **The bump from librqbit 8.1.1 to 9.0.1 is what made those two possible, and it moved five things worth knowing.** `SessionOptions` grew sub-structs — `dht`, `listen`, `connect` — so the DHT persistence flag, the port range and … → [torrents-network](docs/rules/torrents-network.md)
- **A torrent arrives three ways, and a `.torrent` file is not a file the player opens.** A magnet is pasted, a `.torrent` URL is pasted, and a `.torrent` **file** is dropped or picked — and only the first two fit a box that takes … → [torrents-core](docs/rules/torrents-core.md)
- **A freshly added torrent is not ready to be touched, and the order of the three steps is load-bearing**: `add_torrent` returns while the torrent is still `Initializing` (checking and allocating files on disk, which scales with … → [torrents-core](docs/rules/torrents-core.md)
- **Seeding is off by default, and the switch has to be able to stop it *now*.** In Germany and several other jurisdictions the exposure from uploading is categorically worse than from downloading, so a player must not opt anyone … → [torrents-core](docs/rules/torrents-core.md)
- **Peer count has exactly one large lever, and it is not the one that suggests itself.** One private tracker's swarm was probed one address at a time, and the failures separate cleanly: of ~30 peers the tracker handed out, **20–22 … → [torrents-network](docs/rules/torrents-network.md)
- **A finished episode leaves no trace, so the torrent store keeps its own record of them.** `frameplayer.positions` *deletes* an entry past `FINISHED_FRACTION` — which is what takes a film out of "continue watching" — so a watched … → [torrents-core](docs/rules/torrents-core.md)
- **Once the viewer can choose where torrents are written, "the torrent directory" is three questions, and answering them with one path is how a video player deletes somebody's films.** `Dirs` keeps them apart. → [torrents-core](docs/rules/torrents-core.md)
- **The torrent's folder carries its info hash, and disk is the source of truth for storage.** `ManagedTorrentOptions.output_folder` is `pub(crate)`, so a folder named after the torrent alone could never be *read back* and the … → [torrents-core](docs/rules/torrents-core.md)
- **A deleted torrent used to come back, one per restart, as an empty copy of itself.** `torrent_forget` removed the directory and our record, but took the torrent out of librqbit's own store only when it was in `inner.torrents` — … → [torrents-core](docs/rules/torrents-core.md)
- **Deleting a torrent must not be able to fail quietly, and three separate things made it able to.** Reported as "«удалить всё» пишет «освобождено 0 МБ», раздача остаётся, у неё по-прежнему 14 ГБ", and the diagnosis cost a day … → [torrents-core](docs/rules/torrents-core.md)
- **Opening from the history must give back the queue as well as the file.** A local file gets it for free (`loadFiles` builds the folder queue through `filesOpened`), but a torrent episode did not: `resolveTorrentFile` handed back … → [torrents-playback](docs/rules/torrents-playback.md)
- **A torrent stalling mid-playback used to be completely silent.** `opening` is raised in `beforeLoad` and cleared on `file-loaded`, so the loading overlay — the one thing that reports peers and rate — is gone by the time playback … → [torrents-playback](docs/rules/torrents-playback.md)
- **A torrent cannot gain an episode, so replacing one is a first-class action.** The info hash covers the file list and every piece hash — verified by building two torrents from the same folder and adding a file, which changes it … → [torrents-playback](docs/rules/torrents-playback.md)
- **An RSS feed is a link to a *release*, and that is what makes discovery possible without becoming a background client.** Many trackers publish a feed per release whose next item is the re-upload with the next episode, so a feed … → [torrents-playback](docs/rules/torrents-playback.md)
- **The buffer map needed no fork of librqbit, and finding that out was the whole job.** `with_chunk_tracker` is `pub(crate)`, which reads as "patch required" — but `Api::api_dump_haves` is public and hands out the same bitmap, as … → [torrents-playback](docs/rules/torrents-playback.md)
- **Previews before a file is complete are per-position, not per-file.** `thumb_get(exact)` opens its own decode session, so single frames need no storyboard — but the background grid pass walks a file start to end and would decode … → [torrents-playback](docs/rules/torrents-playback.md)
- **Three things a torrent gains once its file is on disk, and one it cannot.** A torrent routinely **ships its own subtitles** (Sintel carries ten) — `sub-auto` finds nothing, since there is no local file to look beside, so they … → [torrents-playback](docs/rules/torrents-playback.md)
- **Casting (Google Cast) is a URL handoff, and while it runs the window is a remote control.** The TV fetches the file from a LAN HTTP server in cast.rs and decodes it itself; mpv stays **paused with the file loaded**, so tracks, … → [casting](docs/rules/casting.md)
- **Discovery is multicast, and a router may not carry multicast between the machine and the television — so every search is also a unicast sweep.** Wired/5 GHz vs 2.4 GHz is the case it is for (a precaution — not yet measured against such a router); `lan_sweep.rs` asks remembered addresses, then each LAN host, directly. → [casting](docs/rules/casting.md)
- **DLNA is the second way to a television, and the one that asks the least.** A UPnP MediaRenderer *is* the TV's own player, so a release it already decodes plays untouched — measured on the real set: a 6.3 GB 4K HEVC Main-10 … → [casting](docs/rules/casting.md)
- **Everything DLNA knows about a television has to be read off that television, because the next one is a different make.** Every rule above was measured against one set, and the failure mode of measuring against one device is … → [casting](docs/rules/casting.md)
- **The tolerance of the string-scanning XML lives in one function, and it is the price of not taking a parser.** `tag`/`chunks` compare a **local** name against everything up to the first whitespace, so an attribute … → [casting](docs/rules/casting.md)
- **Watching together is one shared timeline and nothing else, and the shape of the wire is the whole design.** What travels is a **snapshot of the state after a change** — `{content, paused, position, speed, at, rev, by}` — never … → [sync](docs/rules/sync.md)
- **The catalog is two services and one rule about which answers what.** A metadata service says *what* to watch — posters, localised titles, descriptions, how many seasons a series has — and a Torznab-compatible indexer says … → [catalog](docs/rules/catalog.md)
- **A network source is not a file, and a surprising amount of the player assumes otherwise.** `isNetworkSource()` gates the three that matter: the thumbnail storyboard (it decodes the *whole* file in the background — over a stream … → [sources-and-privacy](docs/rules/sources-and-privacy.md)
- **File associations on macOS need UTIs, not extensions**: `bundle.fileAssociations[].contentTypes` → `LSItemContentTypes`. Without it only mp4/mov/avi worked, because modern LaunchServices matches by UTI and `CFBundleTypeExtensions` is a legacy fallback. `public.movie` is the supertype that covers formats declared by other apps (mkv); the explicit UTIs next to it are belt-and-braces. Tauri's association schema is `additionalProperties: false` — no comment keys.
- **Opening files ("Open with", associations)**: two delivery paths that must both work — `argv`/single-instance on Windows, an Apple Event (`RunEvent::Opened`) on macOS — and both funnel into `deliver_files()`, a buffer the frontend drains with `take_pending_files` on the event *and* on window focus. → [sources-and-privacy](docs/rules/sources-and-privacy.md)
- **Anything mpv keeps across a file change is a leak waiting to be noticed.** `video-rotate`, `video-aspect-override`, `panscan`, the A–B loop points and — measured — **`sub-delay`, `audio-delay` and `sub-speed`** all survive a `loadfile`, so … → [mpv-playback](docs/rules/mpv-playback.md)
- **What a source *is* is not how the player reaches it.** Everything remembered about a video — position, chosen tracks, title — is filed under `sourceId()` (source.ts), not under the path. → [sources-and-privacy](docs/rules/sources-and-privacy.md)
- **A video's name is asked for only when it is not already free, and only from the site serving it.** mpv supplies `media-title` for anything that plays (yt-dlp resolves the page), so that is recorded into one cache keyed by … → [sources-and-privacy](docs/rules/sources-and-privacy.md)
- **The start screen shows the title the player showed, not the file name.** A path is written for sorting, not for reading, and for a link it is a video id with a tracking query attached — so `frameplayer.positions` carries the … → [sources-and-privacy](docs/rules/sources-and-privacy.md)
- **Start screen recents**: `RECENT_LIMIT` cards, candidates drawn from a pool 3× that size — a dead file (renamed, deleted, unplugged drive) is replaced by the next candidate rather than leaving a gap, and is *not* purged from the … → [ui-surfaces](docs/rules/ui-surfaces.md)
- **The start screen must be complete in its first frame.** `loadRecent` and `refreshTorrents` used to be fired un-awaited before `show()`, so the window appeared holding the panel and its two buttons and then grew a rail and a … → [ui-surfaces](docs/rules/ui-surfaces.md)
- **Start-screen layout must not depend on its own content.** `.overlay` is a `grid` with `place-items: center`, which sizes the column from content — so the recents grid was measured from the cards, and the cards changed as … → [ui-surfaces](docs/rules/ui-surfaces.md)
- **A remembered track is a description, not an id, and its scope is the folder.** Track ids are positions inside one file — the Russian dub that is #2 in episode 1 is routinely #3 in episode 2 — so a choice is stored as `{lang, … → [tracks-and-subtitles](docs/rules/tracks-and-subtitles.md)
- **Restoring on resume has two traps.** The chosen tracks live in `frameplayer.tracks`, separate from `frameplayer.positions` because those entries are *deleted* past 97 % and a dub choice should outlive finishing an episode; only … → [tracks-and-subtitles](docs/rules/tracks-and-subtitles.md)
- **Watch history & privacy**: a file enters the history only once playback passes `min(15 s, 5 % of duration)` and leaves it again past 97 % — the relative half of that floor is not optional, a flat 15 s made anything shorter than … → [sources-and-privacy](docs/rules/sources-and-privacy.md)
- **The mini player is a window mode, and every other size mode has to know about it.** With `wid` embedding there is no true picture-in-picture to reach for — the video is a child view of our own window — so it is a small … → [window](docs/rules/window.md)
- **Floating over another app's fullscreen is decided by the window's class, not by its level — and every plausible guess about it is wrong.** "Always on top" only lifts a window above the others *on its own space*, and a … → [window](docs/rules/window.md)
- **Placing a floating box is three rules, and they belong in one place.** Flip to the other side of the anchor when the preferred one has no room, shift along the other axis to stay inside, cap the size when the box is simply … → [ui-surfaces](docs/rules/ui-surfaces.md)
- **The OSC panels are anchored to the bar, not to a number somebody added up.** `.menu`'s `bottom` is `calc(100% + 12px)` — `.osc`'s top edge plus the same gap the timestamps already keep above the seekbar … → [ui-surfaces](docs/rules/ui-surfaces.md)
- **Each side of the window has one edge, 20px in, measured to the glyph.** The seekbar, the end glyphs of the control row (hence `.controls { margin-inline: -11px }`), the logo, the track menus, the skip button, the readouts and the OSD all end on it, because the traffic lights and the Windows close glyph already do and cannot move. → [ui-surfaces](docs/rules/ui-surfaces.md)
- **The control bar's right-hand cluster folds into one button, and what decides it is arithmetic rather than a breakpoint.** How many buttons that cluster carries is a property of the file, so the width where it pushes play off centre is too — `Controls` measures the row and folds when … → [ui-surfaces](docs/rules/ui-surfaces.md)
- **The player's wheel gestures stop at the edge of any floating surface.** Vertical is volume and horizontal is a scrub, which is right over the video and wrong over a list — scrolling the queue was changing the volume. The rule lives once in `onWheel` (`WHEEL_SURFACES`, matched with `closest`), not as a `stopPropagation` per surface: that list is invisible until the one surface missing from it is found by hand, which is how the queue, the chapter list and the context menu all ended up missing. Ctrl+wheel still calls `preventDefault` everywhere, surface or not — without it the webview zooms the whole UI and there is no way back.
- **The HTML resize edges are Windows-only, and on macOS they were actively harmful.** `startResizeDragging` reaches `drag_resize_window`, which on macOS is a stub returning `NotSupported` — and Tauri discards the error (`let _ = … → [window](docs/rules/window.md)
- **Window prefs & the macOS menu bar**: settings live in `localStorage` (`frameplayer.window`: remember geometry / fit to video / always-on-top), restored **before** `show()` — restoring a visible window reads as a jump. → [window](docs/rules/window.md)
- **A link promises navigation; an action gets a button.** `.settings-link` is for "show mpv.conf in Finder" and nothing else. Destructive actions are `.btn-danger`, and secondary actions next to a `.primary` — "open a link", "install yt-dlp" — are `.btn-outline`: they open a dialog or download a binary, which is not navigating anywhere.
- **Hiding the cursor must trail the native button hide, never race it — and a timer is not a way to trail anything.** `set_buttons_visible` asks AppKit for a title-bar relayout, which resets the window's cursor rectangles and puts the arrow back; the flat margin that used to cover the IPC, the main-thread hop and the relayout is now a … → [window](docs/rules/window.md)
- **Hiding the cursor on idle has exactly one exception, and it is a corner, not a band.** `.player.nocursor` applies in windowed mode too — there is no fullscreen condition. The corner is a latch on the last mousemove, and only the native side can … → [window](docs/rules/window.md)
- **The chrome's hover flags are edge-triggered, so each one needs a way back.** `oscHover`, `barHover` and `chipHover` hold the bars up under a resting pointer, and one lost `mouseleave` pins the interface on screen for the rest of the session — which is why `pokeUi` … → [window](docs/rules/window.md)
- **Coming back to the window counts as a mouse move.** A space switch or an activation makes AppKit put the arrow back without sending a mousemove, and nothing re-applies `cursor: none` until the mouse is touched — so `wakeEffect` … → [window](docs/rules/window.md)
- **A `$derived` re-evaluates the moment it is read again, not at the end of the tick.** So an event handler must read the derived object it is acting on *once*, before it touches anything that derived depends on. → [frontend](docs/rules/frontend.md)
- **State lives in `.svelte.ts` modules, not in a store library.** Runes work outside components, so a module exporting a class instance with `$state` fields *is* the store — with per-field reactivity a subscription-based store … → [frontend](docs/rules/frontend.md)
- **Splitting further.** The page went from 11 727 lines to ~1 600 across twenty-four components and twenty-two `.svelte.ts` modules, and four silent traps came out of it: a CSS rule whose markup moved, scoped `@keyframes`, a state written as an ancestor, and the cascade order changing with the file. → [frontend](docs/rules/frontend.md)
- **A hotkey is a row in a table, and three things move with it.** `ACTIONS` in [keys.svelte.ts](src/lib/keys.svelte.ts) is the only place a default binding is written; `onKeydown` normalizes the event to a chord … → [frontend](docs/rules/frontend.md)
- **Localisation**: user-facing strings live only in `src/lib/i18n.svelte.ts` and reach the UI through `t('key')` / `t('key', { param })`; the macOS menu is built in Rust and carries its own EN/RU tables, and platform wording gets separate keys rather than a branch inside one string. → [frontend](docs/rules/frontend.md)
- **A subtitle made for another frame rate is stretched, never delayed, and the stretch is offered, never applied.** `sub-speed` = subtitle fps / video fps (`sub-speed.ts`); a search result that knows its fps asks, the track menu covers the rest by hand. → [tracks-and-subtitles](docs/rules/tracks-and-subtitles.md)
- **The language of the interface and the language of the film are different questions, and conflating them was a bug with a name.** Every language choice in the player used to offer exactly two — `alang`/`slang` in the settings, … → [tracks-and-subtitles](docs/rules/tracks-and-subtitles.md)
- **"No subtitles in a language I am already hearing" is mpv's option, not ours, and its rule is not the obvious one.** `subs-with-matching-audio` — choices `no`/`forced`/`yes`, default **`yes`** on mpv 0.41 (read out of … → [tracks-and-subtitles](docs/rules/tracks-and-subtitles.md)
- **Focusing an input scrolls it into view, and that silently undoes whatever scrolling you just arranged.** The subtitle panel's sign-in form unfolds at the bottom of a box that already scrolls (`.settings` is `max-height: 100%; … → [css](docs/rules/css.md)
- **Every box is its border box, and every size written down is an outer size.** The reset (`*, *::before, *::after`) lives in app.css. → [css](docs/rules/css.md)
- **A flex item does not shrink below its content unless told to.** `min-width: auto` is the default, so `overflow: hidden; text-overflow: ellipsis` on a label inside a flex row never gets the chance to fire: the row grows instead, … → [css](docs/rules/css.md)
- **Reusing a class means beating it, and equal weight is decided by source order — this has now cost two bugs.** The queue rows lost `display: flex` to `.menu-item`'s `display: block` that way; the torrent list's delete button … → [css](docs/rules/css.md)
- **Animating `opacity` on a small glyph makes it twitch, and no amount of measuring geometry will find it.** The torrent list's delete cross was reported "jumping and coming back" on hover. → [css](docs/rules/css.md)
- **The hover preview keeps the video's own aspect, not 16:9.** mpv reports `dwidth`/`dheight` — the size with the aspect already applied — the moment a file opens, so the box can be shaped correctly before any frame exists. → [ui-surfaces](docs/rules/ui-surfaces.md)
- **A scrolling box says it has more below it, and the shape of that is forced rather than chosen.** `ScrollFade` is a bottom gradient with a chevron, rendered as the **last child of the container that scrolls** — one line per list … → [ui-surfaces](docs/rules/ui-surfaces.md)
- **Tooltips**: put `data-tip="..."` (+ matching `aria-label`) on any element — a delegated handler renders the custom tooltip. → [ui-surfaces](docs/rules/ui-surfaces.md)
- **OSD**: `showOsd(text, { sub, progress, sticky })` — a label, an optional bar and an optional smaller second line, up for 1.2 s. No icons, and it sits at `z-index: 80`, above the dialog backdrop, which is safe only because it takes no clicks. → [ui-surfaces](docs/rules/ui-surfaces.md)
- **Tests cover the pure end of the codebase, and the choice of what to cover is the point.** `vitest.config.ts` is separate from the app's Vite config because `sveltekit()` brings routing and the `$app/*` virtuals with it; the … → [frontend](docs/rules/frontend.md)
- **CI runs the gates on every push** (`.github/workflows/checks.yml`), which nothing did before: `release.yml` is gated on a version bump, so an ordinary commit was checked by nobody. Node only — the Rust half needs libmpv and the FFmpeg SDK, which the release job downloads or builds from source, so **`cargo check` and `clippy` are not covered by CI** and stay a local step. There is deliberately no build-output style diff in CI: it existed as a one-off while the page was being split into components and answered a question — "did this refactor change a rendered style" — that stops being asked once the split is done.
- **Validation before finishing any task**: `npm run gates` (0 errors) and, if Rust/config changed, `cargo check` (isolated target dir if dev is running). If anything under `services/` or `shared/sync-protocol.txt` changed, `go vet frameplayer/... && go test -race frameplayer/...` from the repository root — the Node gates do not reach the Go services, and the protocol contract is checked from both sides. Run sidecar smoke tests when touching step_engine/thumb_service.
- **The application is GPL-3.0-or-later, and that is the conclusion the LGPL work was for.** Keeping every bundled library strictly LGPL bought exactly one thing: the freedom to make this player closed or paid. → [build-and-release](docs/rules/build-and-release.md)
- **Licensing is a build output, not a document somebody remembers to update.** The player links libmpv, FFmpeg, libplacebo, GLib, FriBidi, Graphite2 and the gettext runtime — all LGPL-2.1 — which a proprietary application may do … → [build-and-release](docs/rules/build-and-release.md)
- **Releases**: `npm run set-version 0.21.0` (or `patch`/`minor`/`major`), then push to main. → [build-and-release](docs/rules/build-and-release.md)
- **Icon**: master is `src-tauri/icons/icon-master.svg` (viewfinder glyph, indigo #6366f1, ~3% inner margin, no tile); regenerate via resvg + `npm run tauri icon`. Keep the title-bar inline SVG in +page.svelte visually in sync (same glyph, rounded triangle).
- **Accent colors**: indigo `#6366f1` (fills) / `#818cf8` (hover/active/focus); white text on accent surfaces. → [css](docs/rules/css.md)
- Do not commit `src-tauri/{lib,ffmpeg,tools}` binaries. **The repository is public**, `docs/` included: nothing committed may name a host, an account, a machine, a person or an address on somebody's network. Device behavior goes in by model and by what it did. The one file kept out is `HANDOFF.md` — the notes carrying exactly those details while work moves between machines.
