# CLAUDE.md — Frame Player

Windows and macOS video player: **Tauri 2 + Svelte 5 (SvelteKit static) + in-process libmpv** via `tauri-plugin-libmpv` (wid embedding: mpv renders into a child HWND / NSView *behind* the transparent webview; the entire UI is HTML on top). On macOS that needs a patched libmpv — stock mpv does not implement `--wid` there at all; see `patches/` and `scripts/build-macos-libs.sh`.

The UI is **localised (ru/en/zh)** — every user-facing string goes through `t()`, see "Localisation" below. Code comments and docs are **English**.

## Where the documentation is

This file is the index of the rule book and the part of it that holds anywhere:
how the code works now, and which rules must not be broken whatever is being
changed. Each area's short form — its modules, its rules one paragraph each and
the commands that exercise it — is **`.claude/rules/<chapter>.md`**. The
**whole** of a rule — its identifiers, its measurements and the failure it was
written against — lives in **`docs/rules/`**, and every short-form line links to
it. The reasoning *behind* the rules — dead ends, options tried and rejected —
lives in **`docs/`**. All of it is part of the repository.

**Before changing code in an area, read its short form** (`cat
.claude/rules/<chapter>.md`; the last column of "Layout" says which), and the
chapter when the short form is not enough to change the code under it. Claude
Code loads a short form by itself when a file it covers is opened with Read,
Edit or Write — a file read through Bash loads nothing. A question that changes
no code can skip it.
<!-- Why the explicit read: over the forty sessions measured on 2026-10-27 the
agent read files through Bash 3 871 times and with Read 253 times, so a rule
that waited to be loaded by its path would mostly not have been.
docs/agent-context.md has the numbers. -->

| `docs/rules/…` | The rules for |
|---|---|
| `mpv-playback.md` | Driving libmpv: state mirrors, the seek contract, the playlist and the end of a file, chapters, picture geometry, the A–B loop, audio |
| `thumbnails.md` | The seekbar storyboard and every frame decoded outside playback: the budget, the colour space, which frame a hover must show |
| `tracks-and-subtitles.md` | Choosing a track and remembering the choice, subtitle search and placement, closed captions, content languages |
| `torrents-core.md` | The session, adding a torrent, where the data lives, what may be deleted — and the macOS descriptor budget under it |
| `torrents-network.md` | The swarm: DHT, trackers, encryption, the proxy, port forwarding, and the preferences that rebuild the session |
| `torrents-playback.md` | Playing one: the queue, the buffer map, the readouts, subtitles inside a release, replacing a torrent with its re-upload |
| `casting.md` | Google Cast, DLNA and AirPlay: the ladders, the LAN server, the remote-control rules, what one television taught us |
| `sync.md` | Watching together: the wire, the reconciler, readiness, what a room may know |
| `catalog.md` | The metadata proxy, the indexer, and what leaves this machine when somebody searches |
| `sources-and-privacy.md` | What a source *is* against how it is reached, files arriving from the system, the watch history and the nine privacy enforcement points |
| `window.md` | The transparent window, the macOS title bar, fullscreen, the mini player, geometry, the idle and cursor rules |
| `ui-surfaces.md` | The settings sheet, the context menu, the start screen, tooltips, the OSD, and the arithmetic that places anything floating |
| `css.md` | The box model, the cascade between borrowed classes, and the rendering traps measured in both engines |
| `frontend.md` | Where state lives and which way it may depend, the split, hotkeys, localisation, what is worth a test |
| `build-and-release.md` | What ships inside the app, the LGPL/GPL obligations, signing on both platforms, the release workflow |

Each chapter's short form has the same name under `.claude/rules/`; `site.md`
there is the one without a chapter.

| `docs/…` | What it holds |
|---|---|
| `architecture.md` | Stack decisions, the embedding model and the constraint list — mandatory reading before touching mpv interop, seeking, fullscreen or zoom |
| `ROADMAP.md` | Shipped, planned and considered features; a comment saying "ROADMAP 21" means a numbered item there |
| `macos.md` | Why stock `--wid` cannot work on macOS, what the patch does, the CoreAudio crash the second patch is a backport for (and the one that was measured and not shipped), and the platform's own traps |
| `sdr-color.md` | SDR against QuickTime, measured: the grey-scale and real-frame numbers, why metadata is not the cause, and which option combinations did and did not reproduce it |
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

A new rule's paragraph goes into its chapter's short form, not here: this file
takes a rule only when it holds whatever is being changed.
<!-- This file was 104 KB and sent with every request when it was split, which
is why the threshold is "holds anywhere" rather than "important". -->

## Commands

Node and bun are pinned in `.prototools` and installed by [proto](https://moonrepo.dev/proto) on first use; CI reads the same file (`moonrepo/setup-toolchain`), so a version bump is one line there. bun is the package manager and script runner, **Node stays the runtime** — the scripts under `scripts/`, Vite, svelte-check and vitest all run on it. `bun test` is bun's own test runner, not ours: the tests are `bun run test`.

```bash
bun install
bun run fetch-libs        # one-time: downloads libmpv, FFmpeg SDK, libclang into src-tauri/{lib,ffmpeg,tools}
bun run tauri dev         # dev run (vite on :1420 + cargo)
bun run check             # svelte-check — run after ANY frontend change (expect 0 errors; a few known a11y warnings are OK)
bun run gates             # check + check-runes + check-imports + css-orphans + notices + changelog + readme + test, in one go
bun run test              # vitest, the pure end of the codebase (what is worth a test: `.claude/rules/frontend.md`)
bun run check-runes       # a `$effect` at a .svelte.ts top level (throws on import), a `$state` its own file never writes, a declaration whose code left
bun run check-imports     # import cycles: no error, no warning, a module-evaluation order nobody chose
bun run css-orphans       # a CSS rule whose markup moved into another component
bun run readme:check      # the three READMEs: same headings, every #anchor and file link resolves
bun run tauri build       # NSIS installer (no updater signature locally unless TAURI_SIGNING_PRIVATE_KEY is set)
bun run set-version       # report the version in all four files; exits 1 if they disagree
bun run set-version 0.21.0   # …or `patch` / `minor` / `major`. See "Releases" below
bun run set-version release  # …or the bump the pending changesets call for; writes changelog/<version>.md from them
bun run changelog:preview    # what the next release's notes would say
```

Rust validation while the user's dev instance may be running (target/ is locked by the running exe):

```bash
cd src-tauri && CARGO_TARGET_DIR=/some/tmp/target-check cargo check -q
# macOS: .cargo/config.toml points FFMPEG_DIR/LIBCLANG_PATH at the Windows SDKs, which a fresh target dir needs
cd src-tauri && FFMPEG_DIR=$PWD/ffmpeg-macos LIBCLANG_PATH=/Library/Developer/CommandLineTools/usr/lib \
  CARGO_TARGET_DIR=/some/tmp/target-check cargo check -q
```

`-q` drops the few hundred `Checking` lines and keeps every warning and error.

The Go services (`services/`) are their own tree and their own test run — Node's
gates do not touch them. The workspace at the repository root is what lets one
command cover both; note the pattern is `frameplayer/...` and never `./...`,
because the root is not itself a module:

```bash
go vet frameplayer/... && go test -race frameplayer/...
go run ./services/relay                                  # :8080
go run ./services/relay/cmd/probe -play -drive           # a second peer, so one machine is enough
```

On macOS a test binary in a custom target dir cannot find the bundled dylibs
(`@executable_path/lib` resolves next to `deps/`, where build.rs did not copy
them), so any `cargo test` there also needs `DYLD_FALLBACK_LIBRARY_PATH=$PWD/lib`.

Each area's smoke tests and probes — the sidecar, a torrent against a real
swarm, casting, the installer, the Windows update, the macOS signing chain —
are under "Commands" in its `.claude/rules/` file.

## Layout

| Path | What lives there | Short form |
|---|---|---|
| `src/routes/+page.svelte` | What is left after the split: hotkeys, the fullscreen veil, idle and cursor, the wheel dispatch, the mpv → UI bridge | frontend, window, mpv-playback |
| `src/lib/components/` | The UI, one file per surface; a rule and the markup it styles live in the same file | ui-surfaces, css |
| `src/lib/{player,playback,seek,frame-step,crop,picture-adjust,upscale,now-playing,audio-output}…` | mpv's mirrors and commands, playback whichever machine decodes, everything that moves the playhead, the picture | mpv-playback |
| `src/lib/thumbs.svelte.ts`, `src-tauri/src/{thumb_service,color,step_engine}.rs` | Hover thumbnails, the FFmpeg sidecar, colour | thumbnails |
| `src/lib/{tracks,resume,subs}.svelte.ts`, `languages.ts`, `src-tauri/src/{opensubtitles,external_dirs}.rs` | Tracks, the remembered choice, subtitle search | tracks-and-subtitles |
| `src/lib/{open,torrent}.svelte.ts`, `feed.ts`, `src-tauri/src/{torrent,torrent_storage,feed}.rs` | Opening a source, torrent streaming | torrents-core, torrents-playback |
| `src-tauri/src/{net_route,upnp}.rs`, `src-tauri/vendor/` | The swarm's network, vendored librqbit | torrents-network |
| `src-tauri/src/{cast,dlna,lan_sweep,airplay,airplay_hls}.rs`, `src/lib/cast.svelte.ts` | Google Cast, DLNA, AirPlay | casting |
| `src/lib/sync/`, `services/`, `shared/` | Watching together, the Go services, the contracts two languages keep | sync |
| `src-tauri/src/catalog.rs`, `src/lib/catalog.svelte.ts`, `services/tmdb/` | The catalog | catalog |
| `src/lib/{history,report}.svelte.ts`, `source.ts`, `redact.ts`, `src-tauri/src/{logs,install_count,shell_handler}.rs` | What a source is, the watch history, the issue report, privacy | sources-and-privacy |
| `src/lib/{chrome,window-prefs,curtain,endscreen}.svelte.ts`, `src-tauri/src/{lib,window_*,macos_*}.rs`, `src/routes/veil/` | The window shell, geometry, fullscreen, the app's lifecycle | window |
| `src/lib/{keys,i18n,input}.svelte.ts`, `wheel.ts`, `latest.ts`, `*.test.ts` | Hotkeys, localisation, input, the tests | frontend |
| `src/app.css` + `src/routes/+layout.svelte` | Document-level styles | css |
| `scripts/`, `.github/workflows/`, `licenses/`, `patches/`, `.changeset/`, `changelog/`, `src-tauri/src/{update,portable,webview2,libmpv_load}.rs` | Build, signing, release, the notices, the Windows update, portable copies | build-and-release |
| `site/` | The landing page, its own bun project | site |

## Critical gotchas (violating these causes crashes or subtle breakage)

1. **libmpv-wrapper bugs (upstream, unfixed)** — see architecture.md for details:
   - Never `getProperty(..., 'node')` and never observe properties in `node` format → stack corruption crash. Use string/flag/int64/double; read track-list via `track-list/N/...` sub-properties — and observe `track-list/count` (int64) to know *when* to re-read it, because tracks keep appearing after `file-loaded` (external subs via `sub-auto`, lazily parsed containers): reading once on that event left the subtitles button missing until something else happened to refresh the list. `loadTracks()` is a long chain of sequential reads with several possible triggers, so it needs a sequence guard or a slow earlier run finishes last and overwrites the fresh list.
   - Never send broadcast `script-message` → AV crash in the wrapper's client-message serializer. Only targeted `script-message-to <script> ...`.
2. **ffmpeg-the-third seeking**: always `ictx.seek(ts, ..=ts)` (inclusive). `..ts` silently fails with EPERM.
3. **mpv state mirrors go stale** (event queue overflow drops property-change events). Toggles must use `command('cycle', [prop])`, never `setProperty(prop, !mirror)`. Mirrors are resynced every 1 s and on `queue-overflow` in `resyncState()`, and **which** mirrors is `RESYNC`, a `Record<ObservedName, boolean>` — a newly observed property is a compile error until somebody says whether losing its event is survivable. Both paths into the mirrors go through one `applyProperty`; the `property` hook stays outside it. → [mpv-playback](docs/rules/mpv-playback.md)
4. **Multiple mpv properties are never visually atomic** from outside — even back-to-back Rust `set_property` calls let the VO redraw in between. If several properties must land in one frame (zoom+pan), go through `zoompan.lua`-style script handlers.
5. **Seekbar**: the original rule was "click = one `absolute+exact`; drag = `absolute+keyframes` only, *including the final one*", because a trailing exact seek after keyframe previews caused a frame-blink. That has since been deliberately narrowed — see "Seek flags are a performance contract" in `.claude/rules/mpv-playback.md` for what replaced it and why. If the blink ever returns, `onSeekUp` is the first place to look.
6. **Fullscreen transitions** → `.claude/rules/window.md`
7. **Hidden `veil` window keeps the app alive** → `.claude/rules/window.md`
8. **Hotkeys use `e.code`** (physical keys) so they work in any keyboard layout. Keep it that way — the editor stores codes for the same reason. They live in `ACTIONS` in [keys.svelte.ts](src/lib/keys.svelte.ts), not in a `switch`: `onKeydown` resolves a chord to an action and calls `runAction`, and every place that *shows* a key reads `hint()` from the same table. Two defaults worth knowing: `L` is seek-forward (the J/K/L trio) and **loop is `Shift+L`** — mpv's own binding; the A–B loop that would have taken mpv's plain `l` therefore lives on `A` (`Shift+A` clears it). `noRepeat` and `quiet` are fields of the *action*, not lists of key codes, because neither is a property of a key once the key can be moved.
9. **A bar's gradient tail must not be part of its hit area.** → `.claude/rules/css.md`
10. **The window is transparent, so "nothing painted" means "desktop visible"** → `.claude/rules/window.md`
11. **Buttons/inputs get `font-family: inherit`** via a reset in +page.svelte — form controls don't inherit fonts by default and silently fall back to the system font.
12. **HMR resets JS mirrors** (videoW/videoH etc.) while mpv keeps state and won't re-fire unchanged property events — after editing +page.svelte, reproduce bugs with a full dev restart, not just HMR.
13. **tauri.conf.json / capabilities / icons changes need a dev restart** (and icons are tracked by build.rs `rerun-if-changed`); frontend-only changes hot-reload. **The main window is not in that file** — `app.windows` is deliberately empty and `create_main_window` in lib.rs carries the title, the size, the minimum, the transparency and the rest, because a portable copy's webview data directory has to be an absolute path and the configuration only takes a relative one. Changing the window means changing that function, and `MIN_WINDOW_W/H` in window-prefs duplicates its minimum.
14. **A Windows update has two paths, and only one of them comes back.** → `.claude/rules/build-and-release.md`
15. **Never move the macOS traffic lights by hand — grow the title bar instead.** → `.claude/rules/window.md`
16. **A file descriptor is not a renewable resource on macOS, and running out of them does not fail like running out of something.** → `.claude/rules/torrents-core.md`

## Working in this repository

- **One task per session.** Every request re-sends the whole conversation, so history that has nothing to do with the task only costs — and a long context answers worse, not just dearer. When a new request is unrelated to the work so far in this session, say so and suggest `/clear` (or a new session) before starting on it; when it continues the work, just continue.
- **Work whose output is not needed — only its answer — goes to a subagent.** A search across the tree, reading a long log or a whole chapter to answer one question, a probe or smoke test with a noisy output, a review that should not be anchored on your own reading: hand it to an `Explore` or general-purpose agent with a brief that says what is already known and what to bring back, and keep its conclusion. A lookup of two or three calls, and reading the code you are about to change, stay here.
- **Checks are run in their quiet form**: `bun run gates`, `cargo check -q`, `go test` without `-v`; a long log is cut with `tail` or `grep`, not read whole.
<!-- Measured 2026-10-27: one session in five passed 400 K tokens of context, and
the two longest (945 K and 955 K) each ran several unrelated tasks. -->

## Conventions that hold anywhere

Each line is the rule; the chapter it links to is the whole of it.

- **Localisation**: user-facing strings live only in `src/lib/i18n.svelte.ts` and reach the UI through `t('key')` / `t('key', { param })`; a new key needs all three languages, the Chinese written against [docs/zh-glossary.md](docs/zh-glossary.md); the macOS menu is built in Rust and carries its own EN/RU/ZH tables, platform wording gets separate keys rather than a branch inside one string, and nothing a viewer reads is glued together in code — a list goes through `joinList`, a label and a value through one key with a placeholder. The landing page's mocks are held to these strings in all three languages (`site/src/i18n/app.ts`), so renaming a label the site draws fails the site's build until its mock follows. → [frontend](docs/rules/frontend.md)
- **CI runs the gates on every push** (`.github/workflows/checks.yml`), which nothing did before: `release.yml` is gated on a version bump, so an ordinary commit was checked by nobody. The Rust half is its own workflow (`.github/workflows/rust.yml`), started only by a change under `src-tauri/` (or to the scripts that fetch its SDKs): `cargo clippy --all-targets -- -D warnings` and `cargo test` on Windows **and** macOS, with libmpv and FFmpeg fetched the way the release fetches them. A lint is fixed or turned off on purpose in `[lints.clippy]` of `Cargo.toml`, never left as a warning — a workflow that is always yellow is one nobody reads. The Windows half of the crate cannot be compiled from a Mac, so that job is the only place it is checked before a release. There is deliberately no build-output style diff in CI: it existed as a one-off while the page was being split into components and answered a question — "did this refactor change a rendered style" — that stops being asked once the split is done.
- **The README is three files, and they are one document.** `README.md`, `README.ru.md` and `README.zh-CN.md`, linked to one another from the centred header; a change to one is a change to all three, in the same commit. GitHub builds an anchor from the heading text, so an in-page link copied across from the English leads nowhere in the translation — `bun run readme:check` (a gate) holds the three to the same heading levels in the same order and checks every `#anchor` and relative link. The Chinese follows [docs/zh-glossary.md](docs/zh-glossary.md); the Russian follows the `ru` dictionary's own words for the interface.
- **Validation before finishing any task**: `bun run gates` (0 errors) and, if Rust/config changed, `cargo +<RUST_TOOLCHAIN> clippy --all-targets -- -D warnings`, with the version `rust.yml` pins (isolated target dir if dev is running) — CI fails on any warning, and an older clippy does not know the lints CI runs, so a clean `cargo check` is not enough. If anything under `services/` or `shared/sync-protocol.txt` changed, `go vet frameplayer/... && go test -race frameplayer/...` from the repository root — the Node gates do not reach the Go services, and the protocol contract is checked from both sides. Run sidecar smoke tests when touching step_engine/thumb_service.
- **Every change a viewer will notice gets a changeset, written in the same piece of work as the change.** A file in `.changeset/` (format in `.changeset/README.md`): `"frameplayer": minor` for something new, `patch` for an improvement or a fix; a headline line, then one paragraph. Written for people who use the player — English, what changed for them, no names from the code, no hosts/accounts/machines/people. **Before adding one, read the pending ones**: refining or fixing something that has not been released yet means editing *its* changeset, never adding a "Fixed" line for a bug no user ever had; and keep the empty-frontmatter summary changeset (one sentence for the home page's card) true to the whole pending set. Refactors, docs, tests, CI and the site get none. `bun run changelog:preview` shows the result. → [build-and-release](docs/rules/build-and-release.md)
- **Releases**: `bun run set-version release` (or a version / `patch`/`minor`/`major`), then push to main; it turns the pending changesets into `changelog/<version>.md`. → [build-and-release](docs/rules/build-and-release.md)
- Do not commit `src-tauri/{lib,ffmpeg,tools}` binaries. **The repository is public**, `docs/` included: nothing committed may name a host, an account, a machine, a person or an address on somebody's network. Device behavior goes in by model and by what it did. The one file kept out is `HANDOFF.md` — the notes carrying exactly those details while work moves between machines.
