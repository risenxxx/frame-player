# Known issues of version 2

The open and known problems of version 2 (the interface on Skathi, video from libmpv), for contributors. Each
entry says what is seen, on which platform, where it stands and why, as far as that is known. Entries marked
"worked around" are kept so that the code that works around them has an explanation; claims backed by a
measurement say **measured**, the rest are from source or **inferred**.

Contents:

- **Open:** [Casting connects to nothing](#casting-connects-to-nothing) ·
  [Third-party notices on Windows](#third-party-notices-on-windows) ·
  [Shadows lighter than in version 1](#shadows-lighter-than-in-version-1) ·
  [Line boxes at fractional scales and the Chinese ellipsis (Windows)](#line-boxes-at-fractional-scales-and-the-chinese-ellipsis-windows) ·
  [The "+" on the keys tab sits low](#the--on-the-keys-tab-sits-low) ·
  [Fullscreen leftovers (Windows)](#fullscreen-leftovers-windows) ·
  [Fullscreen leftovers (macOS)](#fullscreen-leftovers-macos) ·
  [Updating a copy that cannot be written (macOS)](#updating-a-copy-that-cannot-be-written-macos)
- **Fix proposed upstream:** [Live resize: a strip and a one-frame shift at the edge (Windows)](#live-resize-a-strip-and-a-one-frame-shift-at-the-edge-windows)
- **Worked around:** [Live resize: frames out of order (Windows)](#live-resize-frames-out-of-order-windows) ·
  [Dropping a file](#dropping-a-file) ·
  [Reproducible Skathi package tarballs](#reproducible-skathi-package-tarballs) ·
  [The wheel notch is 48 px](#the-wheel-notch-is-48-px) ·
  [Resizing from inside the edge (Windows)](#resizing-from-inside-the-edge-windows) ·
  [CSS gaps in the settings sheet](#css-gaps-in-the-settings-sheet) ·
  [Text rendering on Windows](#text-rendering-on-windows) ·
  [Traffic lights (macOS)](#traffic-lights-macos) ·
  [No live-resize signal](#no-live-resize-signal) ·
  [Our own fullscreen transition](#our-own-fullscreen-transition) ·
  [Colour output](#colour-output) ·
  [Quitting does not stop playback at once (macOS)](#quitting-does-not-stop-playback-at-once-macos)
- **macOS's own:** [A flash of the desktop when leaving fullscreen (macOS)](#a-flash-of-the-desktop-when-leaving-fullscreen-macos)

## Casting connects to nothing

**Platform:** both. **Status:** open.

Discovery is ported: Google Cast and DLNA devices on the LAN are listed (measured: the device present was found
within 3 s). Connecting, the LAN media server and the cast session are not ported; picking a device is refused
with a message (`src/cast_module.rs`). On Windows a newly run exe can get **Block** firewall rules for the Public
profile; discovery still works with them, but a cast server, once ported, needs inbound connections and would be
blocked (inferred).

## Third-party notices on Windows

**Platform:** Windows. **Status:** open, not checked.

The Windows build ships a libmpv built from [mpv-player/mpv#18845](https://github.com/mpv-player/mpv/pull/18845)
plus `patches/mpv-d3d11-overlay.diff`, with its dependencies (libplacebo among them) linked statically, and the
MSVC runtime DLLs (`msvcp140.dll`, `vcruntime140.dll`, `vcruntime140_1.dll`) beside it. `resources/THIRD-PARTY-NOTICES.md`
was written for version 1's libmpv and has not been compared against this set. Before a public release either the
notices generator covers the Windows library set, or the runtime is installed through Velopack's
`--framework vcredist143-x64` instead of shipped beside the exe.

## Shadows lighter than in version 1

**Platform:** both (measured on Windows against Edge). **Status:** open; no upstream issue filed yet.

`box-shadow: 0 4px 16px` is lighter next to the box and fades out further than in Chromium (measured: 224 against
204 at the box, 34 px of fade against 23): that is Skathi's blur. Skathi also draws only the first of several
`drop-shadow()` filters (documented), so `--ui-shadow-drop` loses its second shadow; the logo's is compensated in
`app/src/sk-port/Overrides.svelte` (89 % of Chromium's, measured). The close button's glyph shadow keeps about
20 % of Chromium's, not explained.

## Line boxes at fractional scales and the Chinese ellipsis (Windows)

**Platform:** Windows. **Status:** open; no upstream issue filed yet.

At 150 % scaling, `line-height: normal` comes out a third of a CSS pixel off Chromium's whole pixel (measured on a
minimal page: Rubik 12.5px 15.33 against 15, Segoe UI 16px 20.67 against 21), so rows of the settings sheet drift
by up to 7 px by its fourth group. Separately, the `Han Punctuation` face in `app.css` is `local()` only and Skathi
does not consult `local()` (documented), so the Chinese "…" and quotes come from Rubik and "设置…" is 5 px narrower
than in Chromium.

## The "+" on the keys tab sits low

**Platform:** Windows (measured). **Status:** open.

The "+" glyph is centred horizontally now, but sits 1.5 device px below the box's centre where Chromium puts it
0.5 above. Flex centring did not move it; cause not found.

## Fullscreen leftovers (Windows)

**Platform:** Windows. **Status:** open.

- **The exit's landing:** 0–3 desktop frames (up to 12 ms at 240 Hz) when the window is restored, in which the
  picture is displaced by the window's origin and clipped, or the window is see-through. DWM composes the window at
  its new origin before the visual's landing commit, which is issued at the right moment (logged). Not fixed.
- **Entering from the mini player:** the swap chain is still the mini player's size, so the picture is upscaled
  (soft) until the chain is resized at the end (inferred from logs, not compared by eye).
- **The exit's last 40 ms** can repeat one 240 Hz frame (an 8 ms gap) where the old full-screen black used to
  cover the desktop (measured; the cause is inferred to be DWM composing the desktop behind the window).
- Not checked: HDR output, a second display, the `split` route (the animated transition is `sklayer` only).

## Fullscreen leftovers (macOS)

**Platform:** macOS. **Status:** open.

- A 1–2 frame jump of the picture at the very start of every enter, before our animation begins.
- macOS slides the fullscreen Space in from the right while the window grows, its black background beside the
  window for about 300 ms.
- Occasionally a transition stalls (about 1 s, more often on 4K SDR entering and under display load): sampled, the
  main thread waits inside AppKit for the window server (`NSCGSTransactionSynchronize` in `setStyleMask:` and
  `doAfterEnterFullScreen`): the window server is busy with the Space switch, and the app cannot shorten it.
- The shared "interface stays away after a fullscreen change" behaviour (see
  [Our own fullscreen transition](#our-own-fullscreen-transition)) has not yet been run on a Mac.
- Not checked: a second display, HDR on a display without EDR, a real keypress (transitions were driven in
  process).

## Updating a copy that cannot be written (macOS)

**Platform:** macOS. **Status:** known limitation.

Copies dragged from the disk image to `/Applications` or `~/Applications` update without a password and relaunch,
keeping the user's ownership (measured). A copy run from the mounted image or translocated by Gatekeeper refuses
the update cleanly ("not writable") rather than asking for an administrator's password. The first update after any
install is a full download, because nothing leaves a base package in Velopack's cache; later updates are deltas.
Not checked: the first-launch dialog, a real Finder drag, and a standard (non-admin) user, who cannot write
`/Applications` and may end up with a copy owned by someone else that never updates. A drag to the Trash leaves the
data folder (`~/Library/Application Support/app.frameplayer.beta`), Velopack's cache
(`~/Library/Caches/velopack/FramePlayerBeta`), the preferences plist and a system cache folder.

## Live resize: a strip and a one-frame shift at the edge (Windows)

**Platform:** Windows. **Status:** mitigated in our code; fix proposed upstream in
[argon-chat/skathi-gpui#2](https://github.com/argon-chat/skathi-gpui/pull/2) (open). The Skathi pin moves when it is
merged.

While the window is resized, DWM shows the frame at the new size one desktop frame before the content arrives, so a
strip of whatever is behind the transparent window shows at the moving edge; dragging the left or top edge also
shifts the whole page and picture by a step for that frame. The same happens with no video at all (measured,
240 Hz desktop capture: 88 of 796 frames). Cause: Skathi's `gpui_windows` resizes buffers in `WM_SIZE` and draws at
the next vsync-driven `WM_PAINT`; presenting in `WM_SIZE` is already too late, and nothing on the host side
(`RedrawWindow` + `DwmFlush` from our subclass) helps. The upstream change draws in `WM_WINDOWPOSCHANGING`
(measured: 88 → 2 strip frames, 88 → 2 shifted). Until then a black "backstop" visual sits behind everything
(`make_backstop` in `src/windows/mpv.rs`; `FPUI_BACKSTOP=0` leaves it out), so the strip is black instead of the
desktop (measured: 0 strip frames with it at the current pin). The one-frame shift remains.

## Live resize: frames out of order (Windows)

**Platform:** Windows. **Status:** fixed in our code (`src/windows/mpv.rs`).

The picture used to step back a size during a live resize. On the `sklayer` route the video layer must not call
`IDCompositionDevice::Commit` itself: Skathi commits after presenting the frame that carries the placement, and an
earlier commit put the video's new box on screen around an older page. After `ResizeBuffers` the old stretch is
kept until a frame of the new size has been presented, and a forced turn is not paced. If a one-frame lag of the
picture behind the page edge is still noticeable, the next step would be a DirectComposition virtual surface,
whose content changes only at `Commit`.

## Dropping a file

**Platform:** both (run on macOS through Skathi devtools' drop; not run on Windows or with a real Finder drag).
**Status:** worked around in `app/src/sk-port/file-drop.ts`.

The interface listens for Tauri's drag-drop events; Skathi delivers a native drop as DOM drag events instead and
refuses it unless a page accepts `dragover`. The bridge accepts drags carrying files, reads paths with
`appFiles.getPathForFile` and re-emits Tauri's four events (`file-drop.test.ts` covers the mapping). Worth
checking on Windows: a subtitle dropped onto a playing video, a folder of two videos, paths with spaces and
non-Latin letters, and a network share.

## Reproducible Skathi package tarballs

**Platform:** both. **Status:** worked around in `scripts/fetch-skathi.mjs`; confirming that a Windows pack matches
the committed lock is still to do.

`bun install --frozen-lockfile` failed the integrity check of the two Skathi tarballs on another machine. Two
causes: macOS bsdtar archives extended attributes (such as `com.apple.provenance`) as pax headers no other system
produces, and zlib writes a different OS byte in the gzip header on each system. The packer now runs `tar` without
xattrs, ACLs, flags or Mac metadata, sets every mode to 0644/0755, refuses any pax header left in the archive, and
writes one fixed gzip OS byte. With bun's cache warm a mismatching lock can pass, so check with an empty
`BUN_INSTALL_CACHE_DIR`.

## The wheel notch is 48 px

**Platform:** Windows (measured). **Status:** worked around in `app/src/sk-port/tauri-stub.ts`; no upstream issue
filed yet.

Skathi reports a wheel notch as 3 lines × 16 px = 48 px; Chromium reports 100, and the player's wheel rules are
written for that, so a notch moved the volume by 2–3 instead of 5. The stub rescales whole-16-px deltas by 100/48.

## Resizing from inside the edge (Windows)

**Platform:** Windows. **Status:** worked around.

Skathi's `startResizeDragging` is not implemented (it logs a warning), and gpui's own resize band lies inside the
window only at the top. The stub calls the host's `startResize(direction)`, which **posts** `WM_SYSCOMMAND` with
`SC_SIZE | WMSZ_*` so the sizing loop runs outside Skathi's frame draw. On macOS the page draws no resize strips and
AppKit's own inside band applies.

## CSS gaps in the settings sheet

**Platform:** both (measured against Edge). **Status:** worked around in `app/src/sk-port/`.

- Skathi has no `button { text-align: center }` default; `Overrides.svelte` restates it.
- Rubik's Cyrillic `unicode-range` subset was left unloaded inside the sheet, so Russian drew in a fallback; a
  minimal page loads it, so the cause was not found. `fonts.ts` loads Rubik's faces after mount.
- Skathi matches `:focus-within` but does not paint its rule, and keeps the `:focus-visible` outline that
  `input:focus { outline: none }` should remove. `focus-within.ts` marks the focused element's ancestors with a
  class that `Overrides.svelte` styles. No upstream issues filed yet.

## Text rendering on Windows

**Platform:** Windows. **Status:** worked around.

Against Chromium drawing the same interface, Skathi's default text is slightly light (ink 0.97 of Chromium's, the
control panel 0.91; measured). `font-dilation: luminance` under `html[data-sk-windows]` brings it to 1.03.
Subpixel antialiasing adds ClearType fringes Chromium does not draw, and Skathi's own rasterizer is heavier
(1.14), so neither is used. See also
[Line boxes at fractional scales](#line-boxes-at-fractional-scales-and-the-chinese-ellipsis-windows).

## Traffic lights (macOS)

**Platform:** macOS. **Status:** worked around.

The window buttons sat 7 pt further left than in version 1 and were nearly black in an inactive window. Skathi's
`appWindow.theme` does not set the window's appearance on macOS (no upstream issue filed yet), so the host sets
`NSApp.appearance` to dark aqua. Skathi places the buttons itself on every layout, so — unlike version 1, which
grows its title bar — the stub moves them with Skathi's `setTrafficLightPosition` (19, 19). Not checked after a
display or appearance change.

## No live-resize signal

**Platform:** both. **Status:** worked around (`src/macos/live_resize.rs`; the window subclass on Windows).

Version 1 hides the bars and floating surfaces while the pointer resizes the window. Skathi reports no live resize,
so the page kept them up and dropped frames (measured at 1080p60: frame p95 17–25 ms with the interface, 14–16 ms
without). The host watches the platform's live-resize notifications and sends version 1's `resizing` event.

## Our own fullscreen transition

**Platform:** both. **Status:** worked around; the code in `src/macos/fullscreen.{m,rs}` and `src/windows/mpv.rs`
exists for this.

Skathi's `setFullscreen` with its `fade` transition goes through black for a few hundred milliseconds, and on macOS
the green button bypasses the page entirely. Both hosts therefore run their own transition (`FPUI_FS_ANIM=0`
brings the fade back):

- **macOS.** Every way in (keys, double-click, menu, green button via `toggleFullScreen:`) takes one path. The
  window takes its end frame at once and Core Animation animates the video layer, because stepping the window from
  the main thread missed 35–56 % of display frames (measured). The target is the frame AppKit gives a fullscreen
  window (below the notch), not the screen's. The way out takes the mask off at once and shrinks the window through
  AppKit's animated resize, the held fullscreen picture filling it, as bare mpv does. The page is hidden during the
  transition, and AppKit takes the first responder from a hidden view for good, so it is handed back explicitly;
  otherwise F and Esc reach the window and beep. The host keeps the **wanted** state (`fsWant`): a request during a
  transition waits and the latest one is applied afterwards, because AppKit refuses toggles mid-transition. The
  exit animation is started from the run loop, not a main-queue block, which ran up to a second late behind gpui's
  work.
- **Windows.** The window is made full screen once and the video's DirectComposition visual is animated. The old
  swap chain is mapped onto the new box with one scale (`PicLayout::rect` ports mpv's `video/out/aspect.c`), so the
  picture keeps its aspect (measured: at most 0.24 % error). The backstop is clipped to the animated box so the
  rest of the window stays see-through.
- **Both.** The bars go away for a fullscreen change and do not come back on their own, only on what normally
  raises them (`windowTransition`, `chrome.hushed` in the shared interface; off in version 1).

What remains is under [Fullscreen leftovers (Windows)](#fullscreen-leftovers-windows) and
[(macOS)](#fullscreen-leftovers-macos).

## Colour output

**Platform:** both. **Status:** worked around in the hosts' surface setup.

- **macOS.** SDR files go to 8-bit sRGB surfaces with the interface's own `target-trc`; HDR files go as PQ/BT.2020
  into a 10-bit layer tagged BT.2100 PQ with HDR10 EDR metadata, as bare mpv's `target-colorspace-hint` does. A
  linear route into 8-bit drawables made everything far too dark (gamma applied twice, HDR without EDR). Measured
  after the change: SDR equal to version 1 and bare mpv, HDR equal to QuickTime. Not checked: Dolby Vision
  reshaping, HLG, a display without EDR.
- **Windows.** On an HDR display the `sklayer` route writes PQ into a 10-bit HDR10 chain (as bright as Chromium and
  Media Player, measured); on an SDR display an 8-bit chain keeps the interface's `target-trc` (forcing `srgb` lifted
  the shadows). SDR content on an HDR desktop follows mpv's BT.1886 (a 2.4 power), 8–12 % darker in the mid-tones
  than the system's players (measured); kept, as version 1 does the same (inferred). Not checked: a real SDR display.

## Quitting does not stop playback at once (macOS)

**Platform:** macOS. **Status:** worked around in `src/macos/quit.{m,rs}`; reported upstream as
[argon-chat/skathi#112](https://github.com/argon-chat/skathi/issues/112).

gpui answers `applicationShouldTerminate:` with TerminateLater and never replies, so the process ended only at
Skathi's 4 s teardown deadline while mpv played on hidden. The host pauses mpv and hides the video on the first sign
of a quit, replies to AppKit after gpui's queued quit, and posts a wake-up event every 100 ms until the process ends
(a Dock quit otherwise sometimes slept on). Measured: paused at +1 ms, gone at +0.25 s. A logout, restart or
shutdown keeps gpui's answer; not checked.

## A flash of the desktop when leaving fullscreen (macOS)

**Platform:** macOS. **Status:** the system's own: a browser's native fullscreen exit shows it too; nothing to fix in the app.

At the very end of leaving fullscreen the desktop wallpaper can flash dark for a frame or two. A browser's native
fullscreen exit does the same; other windows do not flash, and screen capture cannot see it, so it happens in the
display pipeline as the Space switches back.
