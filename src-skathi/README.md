# Version 2: the interface on Skathi

Version 2 is version 1's interface (`../src`) drawn by
[Skathi](https://github.com/argon-chat/skathi) instead of a web view, with mpv rendering
through its render API into the page instead of into a native window behind it. The
interface's code is shared: what version 2 needs of it is either a no-op in a browser or
switched on only by version 2's Tauri stand-in (`windowTransition` in
`../src/lib/window-transition.ts`), so version 1 behaves as before. It is a beta: it builds
and plays on macOS and Windows (on Windows the overlay route needs a libmpv with our patch,
see below), and much of version 1 (torrents, casting past the device list, watching
together, the catalog, the issue report) is still a stub. The reasoning behind it is in
`docs/ROADMAP-v2.md`, `docs/native-frames.md` and `docs/v2-beta-release.md` on the
[`docs/native-frames` branch](https://github.com/risenxxx/frame-player/tree/docs/native-frames). Problems seen in builds of version 2, and their
causes: [KNOWN-ISSUES.md](KNOWN-ISSUES.md).

This folder is everything version 2 adds. `../src-tauri` is version 1's and is not built
here; four of its modules (`color.rs`, `crop.rs`, `power.rs`, `lan_sweep.rs`) are compiled
into this crate as they are.

| Path | What |
|---|---|
| `app/` | The Skathi app root, its own bun project. `vite.config.ts` points `$lib` at `../src/lib` and dedupes Svelte; `src/main.ts` mounts `../src/routes`; `src/sk-port/` stands in for Tauri |
| `app/src/sk-port/tauri-stub.ts` | Tauri's IPC through `@tauri-apps/api/mocks`: `plugin:libmpv`, dialogs, paths, the window and the player's small commands go to the `native:mpv` module or Skathi's `appWindow`; everything else answers a neutral value. `window.__skPortCalls` counts every command, which is the list of what is still a stub |
| `app/src/sk-port/{fonts,focus-within}.ts`, `Overrides.svelte` | Where Skathi's page differs from a browser's: Rubik's `unicode-range` faces loaded at start, `:focus-within` as a class, and the rules that make Skathi draw what WebKit and WebView2 draw from `src/` |
| `src/main.rs` | The crate root: shared modules, then the platform's |
| `src/{commands,thumbs,subtitle_fonts}.rs` | Version 1's commands, thumbnail service and subtitle fonts with Tauri taken out |
| `src/{cast,dlna}/`, `src/cast_module.rs`, `src/logs.rs`, `shim-tauri/` | `native:cast`: version 1's cast discovery (`cast/discovery.rs`, `dlna/{discovery,description,xml,control}.rs`, `lan_sweep.rs`) compiled as it is; `shim-tauri` is a crate named `tauri` whose `async_runtime` is one tokio runtime. Only the device lists: nothing past them is ported |
| `src/macos/` | The `native:mpv` module (`app.rs`), libmpv and its render routes (`mpv.rs`), the Vulkan producer (`vk.rs`) and its Metal un-premultiply pass (`unpremul.m`), the `<sk-layer>` route (`child.rs`), fullscreen, quitting, Now Playing, AppKit glue |
| `src/windows/` | The same module, libmpv with the D3D11 producer and the swap-chain layer routes, Win32 glue |
| `patches/` | The mpv patches the default routes need (see its README) |
| `scripts/` | `fetch-skathi.mjs` (Skathi's JS packages at the pinned commit), `stage.mjs` (version 1's static files and resources), `build-macos.sh`, `run-macos.sh`, and the local packaging helpers `bundle-macos.sh`, `dmg-macos.sh` |

## Skathi

One pin, `rev` on the `skathi` line of `Cargo.toml`. The Rust crate comes from it as a git
dependency; `scripts/fetch-skathi.mjs` packs `@skathi-native/cli` and `/runtime` from
cargo's own checkout of the same commit into `app/.skathi/` and builds the CLI's CSS
checker there. To move the pin: change `rev` (and `skathi-taffy` under `[patch.crates-io]`
to Skathi's `vendor/taffy` submodule commit), run the script with `--force --update-lock`,
then `bun install` in `app/` and commit the new `bun.lock`.

`app/sk.config.json` sets `"textRendering": "optimized"` (DirectWrite on Windows), the
closer match to WebView2's text; `Overrides.svelte` says why, and
`SK_TEXT_RENDERING=skathi-mode` tries Skathi's own mode without a rebuild.

## Build and run (macOS)

Needs Rust from `rust-toolchain.toml` (with the wasm32 target), bun and Node from the
repository's `.prototools`, Xcode's command-line tools, and meson, ninja, cmake and
pkg-config for the library build:

```sh
scripts/build-macos-libs.sh --v2                # once: src-skathi/lib + src-skathi/ffmpeg-macos
FFMPEG_DIR=$PWD/src-skathi/ffmpeg-macos src-skathi/scripts/build-macos.sh   # packages, payload, exe
MPV_LIB_DIR=$PWD/src-skathi/lib src-skathi/scripts/run-macos.sh NAME clip.mp4
```

`--v2` builds mpv (PR #18845 + `patches/`), libplacebo, glslang, the Vulkan headers and
loader, FFmpeg (version 1's configuration), libass and the rest of mpv's dependencies from
pinned, checksummed sources, and takes MoltenVK from KhronosGroup's release; nothing is
linked from Homebrew, every library is `@rpath/<name>` with `@loader_path` as its only run
path, and the licence gate and a minimum-macOS check run at the end. `vk.rs` loads the
app's own Vulkan loader and MoltenVK: `src-skathi/lib/libvulkan.1.dylib` with
`src-skathi/lib/MoltenVK_icd.json` in a development build,
`Contents/Frameworks/libvulkan.1.dylib` with `Contents/Resources/vulkan/icd.d/MoltenVK_icd.json`
in an app bundle; `MP_VK_LIB` and `VK_DRIVER_FILES` still override. Version 1's FFmpeg SDK
(`src-tauri/ffmpeg-macos`) also works for `FFMPEG_DIR`.

`run-macos.sh` starts the app in the background, muted (`FPUI_MUTE=1`), without taking
focus, with its data in `src-skathi/runs/NAME/data`. Anything that starts the app for a
test should set `FPUI_MUTE=1` the same way; the app itself plays sound. The default route
needs the libmpv of `patches/` (Vulkan render API); `FPUI_ROUTE=gl` runs on version 1's
`src-tauri/lib`.

Checks:

```sh
cd src-skathi
FFMPEG_DIR=$PWD/ffmpeg-macos cargo clippy --all-targets -- -D warnings
FFMPEG_DIR=$PWD/ffmpeg-macos DYLD_FALLBACK_LIBRARY_PATH=$PWD/lib cargo test
cd .. && bun run test        # includes app/src/sk-port/*.test.ts
```

`cargo build --release --features selftest` adds `frameplayer --thumb-selftest OUT_DIR FILE…`,
which times the thumbnail commands on real files with no window; no package has it.

Devtools: `cargo build --release --no-default-features --features devtools` with
`SK_DEVTOOLS_BUNDLE` pointing at a devtools bundle built from the same Skathi commit
(`apps/devtools` in Skathi's repository); the payload then needs `publishDevtools: true`.

## Build and run (Windows)

1. `bun run fetch-libs` at the repository root (version 1's FFmpeg SDK, libclang and
   `libmpv-2.dll`), then `node src-skathi/scripts/fetch-skathi.mjs`. The script packs
   Skathi's packages itself (it writes the ustar archives in JavaScript), so no particular
   `tar` is needed.
2. `bun install --frozen-lockfile` and `bun run payload` in `src-skathi/app/` (its
   `tsconfig.json` stands alone; nothing at the root has to be built first).
3. `cargo build --release` in `src-skathi/` with `FFMPEG_DIR` and `LIBCLANG_PATH` set to
   version 1's `src-tauri/ffmpeg` and `src-tauri/tools`, and clang-cl on `PATH` for Skathi's
   C++ parts. The release exe is a GUI-subsystem program with version 1's icon
   (`src/windows/app.rc`).
4. Run it. A build reads the payload from the tree it was built in
   (`app/dist/release/.sk/app.skpk`); a copy of the exe anywhere else needs `app.skpk`
   beside it (which also makes it count as an installed copy) or `FPUI_PAYLOAD`. libmpv is
   `libmpv-2.dll` beside the exe, or the file `MPV_DLL` names:
   - **Stock libmpv:** version 1's `src-tauri/lib/libmpv-2.dll` from step 1 (with the
     `vulkan-1.dll` beside it) has mpv's D3D11 render API and runs the default route; it
     lacks our overlay target, so the host draws subtitles and the OSD into the picture (it
     says so on stderr), and its renderer is `gpu`, not `gpu-next`.
   - **Patched libmpv:** mpv PR #18845 at `a98335f5` with `patches/mpv-d3d11-overlay.diff`,
     built with mpv's own Windows CI recipe and `-Dgpl=false -Dffmpeg:gpl=disabled`. It
     links the MSVC runtime dynamically, so `msvcp140.dll`, `vcruntime140.dll` and
     `vcruntime140_1.dll` go beside it. There is no script in this repository for that build
     yet (`THIRD-PARTY-NOTICES.md` here, "Windows", has the recipe).
5. For an installed copy's layout: a folder with the exe as `fpui.exe`, `libmpv-2.dll` (and
   what it needs), version 1's FFmpeg DLLs, `app.skpk` and `resources/`
   (`scripts/stage.mjs` fills `src-skathi/resources/`).

## Packaging

The beta installs and updates through [Velopack](https://velopack.io) (`src/{macos,windows}/velo.rs`;
`VelopackApp::build().run()` is the first thing `main` does, so the install, update and
uninstall hooks run and exit before anything else). The crate pins `velopack` to the `vpk`
version that packs it. There is no CI workflow for version 2; packaging is done locally. Every package carries
`src-skathi/THIRD-PARTY-NOTICES.md` (`scripts/stage.mjs` puts it in `resources/`), version
2's own notices, rendered with version 1's by `bun run notices` and held to its inputs by
`bun run gates`:

- **macOS:** `scripts/bundle-macos.sh OUT VERSION` lays out `OUT/Frame Player Beta.app` from
  a release build (`src-skathi/lib` in `Frameworks`, the resources, `app.skpk`, the MoltenVK
  ICD file) and fails if any Mach-O links outside the bundle and the system. `vpk pack`
  then signs it with the hardened runtime and `frameplayer-beta.entitlements`, notarizes it
  and builds the installer; `scripts/dmg-macos.sh FEED OUT.dmg IDENTITY PROFILE` makes the
  drag-to-Applications disk image from the same feed. Signing identities and the notary
  profile are always passed in, never stored here.
- **Windows:** `vpk pack` over the folder of step 5 above, with `-r win-x64`.

When an update is ready (`FPUI_UPDATE_FEED`), the app hands it to Velopack's updater and
**closes itself** the ordinary way, so that mpv shuts down before the files are swapped;
the updater then applies it and starts the new version.

## Where version 2 keeps things

| | |
|---|---|
| Data (Skathi's storage, `config/mpv.conf`, `cache/`, `subtitle-fonts/`, `screenshots/`) | `~/Library/Application Support/app.frameplayer.beta`, `%APPDATA%\app.frameplayer.beta`; `FPUI_DATA_DIR` overrides. An installed copy also logs there: `velopack.log` and `frameplayer.log` (stderr, when nothing redirected it) |
| Resources (`lua/`, `shaders/`, the notices) | `Contents/Resources` of an app bundle (macOS), `resources/` beside the exe (Windows), else `src-skathi/resources` (`scripts/stage.mjs`); `FPUI_RESOURCES` overrides |
| Payload | `Contents/Resources/app.skpk` of an app bundle (macOS), `app.skpk` beside the exe (Windows), else `app/dist/release/.sk/app.skpk`; `FPUI_PAYLOAD` overrides |
| libmpv | macOS: `MPV_LIB_DIR`, else `Contents/Frameworks`, else beside the exe. Windows: `MPV_DLL`, else `libmpv-2.dll` beside the exe |

Nothing here reads or writes version 1's data (`app.frameplayer`).

## Developer switches

Environment variables, read at start. The routes and some of their details are still being
chosen, so the alternatives stay switchable; no package sets any of these.

| Variable | Platform | |
|---|---|---|
| `FPUI_MUTE=1` | both | silence for test runs: `ao=null`, `mute=yes` (Windows also `volume=0` and refuses the interface's volume commands). Unset, sound is on |
| `FPUI_ROUTE` | both | macOS: `child` (default: mpv's own layer under the page, placed by `<sk-layer name="video">`), `split` (picture in `<sk-surface name="video">`, subtitles and OSD in `overlay`), `element`, `video`, `gl` (version 1's libmpv). Windows: `sklayer` (default: the swap chain in Skathi's composition under `<sk-layer>`), `split`, `video`, `element`, `layer` |
| `FPUI_RING` | both | picture ring size |
| `FPUI_HIDDEN=1` | both | the window starts hidden. Skathi shows a hidden window after ~10 s when the app has no tray icon, so that it can still be reached |
| `FPUI_NO_THUMBS=1` | both | no storyboard |
| `FPUI_MPV_EXTRA`, `FPUI_MPV_LOG=1` | both | `"k=v;k=v"` appended to mpv's options; print them |
| `FPUI_FS_ANIM` | both | the fullscreen transition. macOS: `custom` (default) the live window grows to its fullscreen frame and back, the picture playing throughout; `1` AppKit's snapshot animation; `0` Skathi's fade through black. Windows (`sklayer`): on by default, the video's visual animated by DirectComposition; a number sets the length in ms; `0` the fade through black |
| `FPUI_FS_MOVE=window` | macOS | the custom way in steps the window's frame instead of animating the video layer |
| `FPUI_UPDATE_FEED`, `FPUI_UPDATE_DELAY`, `FPUI_UPDATE_APPLY=0` | both | an installed copy checks this Velopack feed (URL or folder) once, `FPUI_UPDATE_DELAY` s after the start (default 5), downloads an update, then closes the app so that Velopack applies it and restarts it (`=0`: download only). Unset, nothing is checked |
| `FPUI_LEAN` | macOS | the memory levers (`FPUI_OVL_RING`, `FPUI_OVL_DROP`, `FPUI_NOJIT`, `FPUI_RING`, `FPUI_METALLIB`); on by default with the `lean` feature |
| `FPUI_OVL_OUT`, `FPUI_OVL_BAND` | macOS, `split` | the overlay written out as an 8-bit copy, and the overlay as a bottom band following what mpv drew |
| `FPUI_FRAME_SOURCE` | macOS | `auto`, `link`, `next`: the window's Metal frame source |
| `FPUI_RESIZE_REDRAW=0` | macOS | do not redraw a paused picture at a new element size |
| `FPUI_CHILD_DRAWABLES`, `FPUI_CHILD_EVICT`, `FPUI_CHILD_COPY=1`, `FPUI_BACKDROP` | macOS | the `child` route's layer: drawable count, eviction, a copy into the drawable instead of rendering into it, the window's backdrop |
| `FPUI_STATS=1`, `FPUI_MTL_LOG` | macOS | render and Metal memory counters on stderr |
| `FPUI_SYNC`, `FPUI_RENDERER`, `FPUI_IDLE_RING`, `FPUI_OVL_DROP`, `FPUI_OVL_BAND`, `FPUI_EXACT_FEEDBACK`, `FPUI_LAYER_SDR`, `FPUI_LAYER_PEAK`, `FPUI_BACKSTOP=0` | Windows | pacing, renderer, ring, overlay and layer details |
| `FPUI_SDR_TRC` | Windows, `sklayer` | the 8-bit layer's `target-trc`. Unset: the interface's own value, mpv's `auto` when it sets none, as version 1 |
| `FPUI_RESIZING=0` | Windows | no window subclass: no `resizing` events (the bars stay up during a live resize) and no `FPUI_FS_ANIM` landing |
| `SK_WINDOW_FOCUS=never` | both | Skathi's: the window does not take focus when it opens (`run-macos.sh` sets it) |
