---
paths:
  - "scripts/**"
  - ".github/**"
  - "src-tauri/tauri.conf.json"
  - "src-tauri/build.rs"
  - "src-tauri/Cargo.toml"
  - "src-tauri/installer-hooks.nsh"
  - "src-tauri/entitlements.plist"
  - "src-tauri/icons/**"
  - "src-tauri/src/update.rs"
  - "src-tauri/src/portable.rs"
  - "src-tauri/src/webview2.rs"
  - "src-tauri/src/libmpv_load.rs"
  - "src/lib/portable.svelte.ts"
  - "src/lib/updater.svelte.ts"
  - "src/lib/release-notes.ts"
  - "src/lib/components/LicensesDialog.svelte"
  - "licenses/**"
  - "THIRD-PARTY-NOTICES.md"
  - "patches/**"
  - ".changeset/**"
  - "changelog/**"
---

# build-and-release — the short form

The short form of [`docs/rules/build-and-release.md`](../../docs/rules/build-and-release.md): the modules this area is made of,
its rules one paragraph each, and the commands that exercise it. Claude Code loads this file
by itself when a file it covers is opened with Read/Edit/Write — not when one is read through
Bash, which is why `CLAUDE.md` says to read it before changing the area.

Each line is the rule; the chapter it links to is the whole of it.

## Modules

| Path | What lives there |
|---|---|
| `src-tauri/src/portable.rs` + `src/lib/portable.svelte.ts` | A copy that keeps everything beside its own executable: the `.portable` marker that decides it, the `.data/{config,data,cache,webview}` layout, and `portable::{config,cache,data}_dir` — which every base directory in the player goes through, so there is one answer to "where does this go". The frontend half owns the one choice that comes with the mode, the shell registration. See the line in [rules/build-and-release](../../docs/rules/build-and-release.md) |
| `src-tauri/installer-hooks.nsh` | What Frame Player adds to Tauri's generated NSIS script (`bundle.windows.nsis.installerHooks`): the welcome-page checkbox that makes an installation portable, and the uninstall half that takes `.data` with it when the viewer asks. Reachable only because the hook file is included at the template's line 35 — before its pages — since the four `NSIS_HOOK_*` macros themselves sit inside sections. See the line in [rules/build-and-release](../../docs/rules/build-and-release.md) |
| `src-tauri/src/libmpv_load.rs` | Loading libmpv on Windows before the plugin does: the system's `vulkan-1.dll` or, failing that, the bundled one (upstream's libmpv imports it and Windows does not ship it), then `libmpv-2.dll` itself, keeping the system's error and the missing imports for the start-failure screen (`libmpv_load_error`) instead of the plugin's "Failed to create mpv instance". See the bundling line in [rules/build-and-release](../../docs/rules/build-and-release.md) |
| `src-tauri/src/webview2.rs` | The engine the interface is drawn with, when nothing installed it: ask (`tauri::webview_version()`), fix silently (Microsoft's bootstrapper, no elevation), and failing that say so in the one interface a process with no window has. Runs before the builder and for both kinds of copy |
| `src-tauri/src/update.rs` + `scripts/pack-windows-update.ps1` | The Windows update applied in place: the second manifest key (`windows-x86_64-zip`, `windows-aarch64-zip` on ARM64), the payload the script builds out of `bundle.resources`, and the swap — `*.fp-old`, the rollback, the file-set rule that sends a release which changes it to the installer instead, and the sweep the relaunched copy does once its predecessor has exited. `scripts/update-test.ps1` runs the whole of it against a staged copy of a real installation, with a throwaway key and a loopback server. See the line in [rules/build-and-release](../../docs/rules/build-and-release.md) |
| `licenses/` + `THIRD-PARTY-NOTICES.md` | The attribution that ships inside the app: `manifest.json` (one entry per third-party project, with the license of the artifact **as built**), `text/` (each project's own license, verbatim), `spdx/` (canonical texts for the ids the Rust and npm graphs declare). `scripts/gen-notices.mjs` renders the notices; `bun run notices:check` is a gate |
| `scripts/fetch-libs.ps1` | Downloads all binary SDKs for one architecture (`-Arch x64` or `-Arch arm64`, x64 by default; also used by CI with an actions/cache keyed on the architecture and this file's hash). Its two `releases/latest` lookups **must be authenticated in CI**: the anonymous GitHub API allowance is 60 an hour *per IP*, a hosted runner shares its address, and a release therefore fails on traffic that is not ours — `GITHUB_TOKEN` (automatic in Actions, optional locally) takes it to 1000. The header goes on the API calls only, never on an asset download: those redirect to a storage host that rejects a GitHub `Authorization` header, and PowerShell forwards headers across redirects |
| `scripts/macos-{build,sign,notarize}.sh` | The macOS signing chain, in the order they run: `sign` puts the Developer ID on the native libraries (and, in CI, imports the certificate into a throwaway keychain), `build` decides whether the hardened runtime goes on at all, `notarize` submits and staples an `.app` or a `.dmg` and repacks the updater archive around the stapled bundle. Each one is a no-op or an ad-hoc fallback without `APPLE_SIGNING_IDENTITY`, so a checkout with no Apple account still builds |
| `.changeset/`, `changelog/` + `scripts/changelog.mjs` | Release notes. `.changeset/*.md` are the changes not released yet, one per thing a viewer will notice; `changelog/<version>.md` is each published release, written from them by `set-version`, and the one source of the GitHub release body, the site's `/updates` page, its feed, the home page's release cards and `latest.json`'s `notes` (the update button's tooltip, via `src/lib/release-notes.ts`) |
| `.github/workflows/release.yml` | Version-bump-gated release: build → sign → notarise → R2 upload (installer + latest.json) → GitHub Release |

## Critical gotchas

(Numbered as in `CLAUDE.md`, which keeps the headline of each.)

14. **A Windows update has two paths, and only one of them comes back.** The one that ships is in place: `update_prepare` downloads the installation as a zip and stages what changed, `update_commit` renames the old files to `*.fp-old`, moves the new ones in and relaunches — so code after it does run, and the relaunched copy waits for `--after-update=<pid>` before sweeping, because a renamed image cannot be deleted until its process exits and because the single-instance guard would otherwise make the player vanish. The fallback is the NSIS installer, and **code after `downloadAndInstall()` never runs there** — it kills the process inside that call, so anything that must survive (the `frameplayer.resume` snapshot) is persisted *before* install starts and `relaunch()` after it is dead code. An in-place update **may not change the file set**: the uninstaller's delete list is fixed at install time, so a release that adds or drops a path takes the installer. → [build-and-release](../../docs/rules/build-and-release.md)

## Rules

- **Everything the player needs at runtime is bundled; yt-dlp is the single exception.** `src-tauri/lib` carries libmpv, the wrapper and every transitive dependency (~65 MB and ~50 dylibs on macOS: FFmpeg, libass, libplacebo, … → [build-and-release](../../docs/rules/build-and-release.md)
- **The application is GPL-3.0-or-later, and that is the conclusion the LGPL work was for.** Keeping every bundled library strictly LGPL bought exactly one thing: the freedom to make this player closed or paid. → [build-and-release](../../docs/rules/build-and-release.md)
- **Licensing is a build output, not a document somebody remembers to update.** The player links libmpv, FFmpeg, libplacebo, GLib, FriBidi, Graphite2 and the gettext runtime — all LGPL-2.1 — which a proprietary application may do … → [build-and-release](../../docs/rules/build-and-release.md)
- **The mpv patches are a set, and the set is part of what the macOS libraries are.** Every `patches/mpv-<version>-*.patch` is applied, in sorted order, and hashed into the name of the published set; the unpacked tree keeps a stamp so a patch added later is never skipped; each one is described in `licenses/manifest.json` or the notices refuse to render; a backport's own header says when to delete it. → [build-and-release](../../docs/rules/build-and-release.md)
- **Windows ships for two architectures, each built on its own kind of machine, and an installed copy never changes architecture by itself.** ARM64 comes from the same upstreams' ARM64 assets plus LunarG's `warm` Vulkan Runtime (the ARM64 libmpv imports `vulkan-1.dll` too); `fetch-libs.ps1 -Arch` picks them and, with `pack-windows-update.ps1 -Arch`, checks every PE file's machine type; the SDK cache key carries the architecture; `release.yml` builds it on `windows-11-arm` with an explicit `--target` and publishes `_arm64-setup.exe` / `_arm64.zip` / `_arm64-portable.zip` under `windows-aarch64` / `windows-aarch64-zip`. `windows-x86_64(-zip)` stay as they were — and an x64 copy on an ARM64 machine keeps reading them until somebody installs the ARM64 build over it. → [build-and-release](../../docs/rules/build-and-release.md)
- **A portable copy keeps everything beside its own executable, and one file decides that it is one.** `.portable` next to the binary — written by unpacking the portable archive or by the installer's own checkbox (`installer-hooks.nsh`); state under `.data/`, both dot-prefixed so the update's file-set rule ignores them. **Where the state lives and how the copy was delivered are two questions**: `portable::installed()` answers the second, and it is what decides the update's fallback (`Plan`'s third mode `download` only for a copy nobody installed) and who owns the shell registration; the window built in Rust so the webview's data store can be an absolute path (an ordinary installation passes nothing and keeps the directory it always had); the shell registration written by the player, which keeps and restores what it displaces; and `webview2::ensure()` for the one dependency the archive cannot carry. → [build-and-release](../../docs/rules/build-and-release.md)
- **A Windows update replaces the files in place, and may not change which files there are.** Two manifest keys — `windows-x86_64` stays the installer for every older client, `windows-x86_64-zip` is the installation as a zip; the swap renames each file to `*.fp-old` and moves the new one in, the relaunched copy waits for `--after-update=<pid>` before sweeping, and a release that adds or drops a path takes the installer, because only the installer rewrites the uninstaller's delete list. → [build-and-release](../../docs/rules/build-and-release.md)
- **Icon**: master is `src-tauri/icons/icon-master.svg` (viewfinder glyph, indigo #6366f1, ~3% inner margin, no tile); regenerate via resvg + `bun run tauri icon`. Keep the title-bar inline SVG in +page.svelte visually in sync (same glyph, rounded triangle).

## Commands

The macOS bundle, and the one decision its build makes for you:

```bash
bun run tauri:macos:build     # → scripts/macos-build.sh. With no APPLE_SIGNING_IDENTITY in the
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
scripts/macos-sign.sh && bun run tauri:macos:build
scripts/macos-notarize.sh "src-tauri/target/release/bundle/macos/Frame Player.app"
```

The installer's own checkbox, without a twelve-minute bundle each time: a
throwaway script with nothing but MUI's own pages compiles in a second, and
`PrintWindow` captures the dialog even when it is not in front. Four things were
visible only this way — a comma in the label silently shifting every
`System::Call` parameter after it, `WS_EX_LAYOUTRTL` not yet defined at the
hook's include point, which page the generic `_SHOW` define actually reaches,
and a control that exists and reports itself visible while a static paints over
it:

```bash
# probe.nsi: MUI2 + the hook's two functions + MUI_PAGE_DIRECTORY + an empty section
"$LOCALAPPDATA/tauri/NSIS/makensis.exe" probe.nsi && ./probe.exe
```

A portable copy, from the archive the release publishes — unpack it, run it, and
watch where it puts things (`FP_NO_SINGLE_INSTANCE` is what lets it run while an
installed copy is open, since the guard is keyed on the identifier;
`FP_SHELL_HANDLER=on|off` drives the registration the settings switch owns, so
the undo path can be tested without a hand on the mouse). **Back the two
registry trees up first** — a portable copy repoints them at itself:

```bash
reg export "HKCU\Software\Classes\frameplayer" protocol.reg
reg export "HKCU\Software\Classes\Video" progid.reg
bun run tauri build --debug --no-bundle
powershell -ExecutionPolicy Bypass -File scripts/pack-windows-update.ps1 -Portable -OutDir /tmp/p
```

The Windows update applied in place, end to end, with no release and no access
to the signing key: a throwaway key, a loopback server and a staged copy of a
real installation (`-Auto <seconds>` drives it without a hand on the mouse,
`-WithRegistry` exercises the uninstall entry and puts it back). See "Testing
the Windows update" in `docs/distribution.md`:

```bash
bun run tauri build --debug --no-bundle
powershell -ExecutionPolicy Bypass -File scripts/update-test.ps1 -WithRegistry
# and, on its own, the payload's layout against a real installation:
powershell -ExecutionPolicy Bypass -File scripts/pack-windows-update.ps1 -CompareWith "$LOCALAPPDATA/Frame Player"
```
