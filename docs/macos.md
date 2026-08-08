# macOS

The player's embedding model — mpv rendering into a view behind a transparent
webview — works on Windows out of the box and is **architecturally impossible on
macOS with stock mpv**. This document is why, what the patch does, and what else
the platform makes different.

## Stock mpv cannot embed on macOS

The `--wid` option, which tells mpv to render into an existing native window, is
read in exactly four places in mpv's source, and none of them is the macOS
backend:

```
options/options.h            int64_t WinID;
options/options.c            {"wid", OPT_INT64(WinID), .flags = UPDATE_VO}
video/out/x11_common.c        ← X11
video/out/w32_common.c        ← Windows
video/out/android_common.c    ← Android
video/out/vo_mediacodec_embed.c
```

The entire macOS backend — `video/out/mac/*.swift` and `mac_common.swift`,
~2300 lines — never references `WinID`. mpv accepts the option, stores it, and
silently ignores it. No warning, no error: it simply opens a window of its own.

Everything *else* about the stack already works there. The plugin builds, the
wrapper has an official macOS build, the transparent WKWebView works, and the
whole interface renders and stays live against mpv. Only the pixels land in the
wrong window.

## The patch

`patches/mpv-<version>-macos-wid-embedding.patch` adds the macOS case, reusing
the `NSView` mpv already renders into — it just installs it somewhere else
instead of into a window of its own. Four files, roughly +160/−15, built by
`scripts/build-libmpv-macos.sh`.

| File | Change |
|---|---|
| `video/out/mac/common.swift` | Resolve `--wid` to an `NSView`; add mpv's view as the bottom-most subview of the host and track it; notification-based resize/DPI/screen observers, since there is no window delegate to attach to; embedded fallbacks for screen, display link, backing scale and window size queries; skip `initApp()` so the host keeps its activation policy, Dock icon and menu bar; skip window geometry |
| `video/out/mac_common.swift` | Visibility and backing-property changes fall back to the host window; a re-initialization guard |
| `video/out/vulkan/context_mac.m` | Sizing no longer bails when there is no window of its own, and comes from the view |
| `DOCS/man/options.rst` | Document the macOS `--wid` case |

Three things it had to get right, each of which was a real bug first:

**The Vulkan context was the actual blocker.** Its resize path returned early
when mpv had no window of its own and took the swapchain size from the window's
frame. Patching only the Swift side produces an embedded view whose surface
never resizes. The fix takes the size from the view in **backing pixels**, which
is also what makes Retina correct.

**Re-initialization was gated on "no window yet".** Embedded, that condition is
true forever, so every reconfiguration — each new file, each resolution change —
would re-run initialization and stack another view. Replaced with an explicit
flag.

**Window geometry was applied on reconfigure**, pushing a screen-derived size
into mpv's dimensions so one frame rendered at the wrong scale before the
view-derived resize corrected it. Skipped entirely when embedded: the host owns
geometry, which also makes `--geometry`, `--autofit` and `--fullscreen`
correctly inert.

What the patch preserves: `gpu-next`, Metal, VideoToolbox hardware decoding and
HDR tone mapping all behave as they do in mpv proper. What it costs: building
libmpv from source for macOS releases, and re-applying the patch when mpv is
updated.

## Library loading

The wrapper opens `libmpv.dylib` by bare leaf name, so libmpv has to sit next to
it: the dynamic loader probes that directory through the wrapper's own
`@loader_path` search path. `DYLD_LIBRARY_PATH` is not a way around it —
`npm` execs through a system-protected shell, which strips every `DYLD_*`
variable.

The bundled set (libmpv, the wrapper, FFmpeg, libass, libplacebo, MoltenVK, Lua
and the rest of the closure — around 50 libraries) is made self-contained by
`scripts/bundle-macos-libs.sh`: it walks the dependency graph, copies everything
into one flat directory, rewrites absolute paths to relative ones and gives each
library a search path of its own directory. The same script relinks the ffmpeg
command-line binary the casting prepare step spawns, which is a small shim over
those same libraries.

One rule follows from where things end up: inside the bundle the executable is
in `Contents/MacOS` and the libraries are in `Contents/Resources/lib`, while in
a development build they are side by side. Anything that looks for a bundled
file has to know both places — "next to the executable" is not one location on
both platforms.

## Window chrome

**Do not move the traffic lights by hand — grow the title bar instead.** There
is no API for positioning them, so the obvious approach is to set their frames
and re-set them whenever the system puts them back. That was built in full and
is a dead end, for three measured reasons:

- The system resets them on every title-bar relayout, and the causes cannot be
  enumerated (setting the window title alone does it every time).
- **Position is only half of moving a button.** The glyphs come from tracking
  areas laid out from the system's own model of where the button is, never from
  the buttons' frames — so moved buttons keep the hover region of unmoved ones.
- **That cannot be repaired.** Replacing the tracking area with a correctly
  positioned copy of itself fixes the geometry and kills hover completely,
  because the system keys on the identity of the area it created.

What works instead: an empty toolbar with the unified style grows the title bar,
and the system itself places the buttons correctly, with matching tracking
areas, stable across resizes and title changes. The one thing it costs is
fullscreen — a window with a toolbar shows it there permanently as an opaque
band — so the toolbar is removed when entering fullscreen and restored when
leaving. Restoration has to wait for the *did*-exit notification: set earlier it
is ignored, and the gap is visible as the buttons sitting at the system position
and then jumping, so the buttons are hidden across the transition and unhidden
once the toolbar is back.

Two smaller ones. The window is forced to the dark appearance, because the frame
view draws itself to match the appearance rather than the background color and
under a light system theme puts a bright highlight along the top edge. And the
window gets an opaque background color, so that the moment before the webview's
first composite shows a dark fill rather than the desktop — but not
`setOpaque(true)`, which costs the rounded corners and the shadow.

## Floating over another application's fullscreen

"Always on top" only lifts a window above others *on its own space*, and a
fullscreen application is a space of its own — so the mini player vanished
exactly when a small window on top is wanted. Everything plausible about this is
wrong, and it was settled with a probe application that put labeled windows in
each configuration:

- **The window level is irrelevant.** An ordinary window does not reach the
  space at any level, including levels above everything the system itself draws.
- **The collection behavior is necessary but not sufficient.**
- What decides it is being an `NSPanel` **with the non-activating style bit**,
  and it then works at the ordinary floating level. Either half alone fails.

Since the window belongs to the windowing library and cannot be asked for as a
panel, it is promoted in place by changing its class and demoted on exit. That
is only sound because the panel class adds no storage of its own — verified by
comparing instance sizes — so only the method table changes, and the size is
re-checked at runtime.

Three consequences: while promoted, the library's own window overrides are gone;
nothing may ask the window to change its focusability, because the library looks
that up by name and the panel class does not have it; and panels hide themselves
when the application deactivates, which has to be turned off. Restoring the
previous state must be verbatim rather than recomputed — the flag that makes a
window fullscreen-capable shares an exclusivity group with the one this feature
needs, and forgetting to put it back leaves fullscreen silently doing nothing.

What this does not buy is corner tucking and pinch resize; those need a private
system controller, which would additionally mean reparenting mpv's view into a
panel the player does not own.

## Permissions

Since macOS 15, a process without **Local Network** permission has its multicast
silently dropped — no error, no prompt in a non-interactive context. The system
resolver is exempt, so a command-line probe reports an empty network while the
system finds devices fine. Anything discovery-shaped therefore has to be
measured from inside the application, and a bundled build is the only place the
permission can be granted at all.

Two things follow for anything that browses the network:

**The prompt has to explain itself.** `NSLocalNetworkUsageDescription` and
`NSBonjourServices` live in `src-tauri/Info.plist`, which the bundler merges into
the application's own (verified in a built bundle). Without the description the
system asks for network access with nothing to say about why — at the moment
whose answer decides whether the feature can ever work.

**The prompt is answered after the search has started.** So are the prompts of
any third-party firewall, and a socket refused in the meantime stays refused —
re-sending on it changes nothing. Discovery therefore has to be *rebuilt*, not
retried, and the interface must not report failure while a prompt is still on
screen waiting for an answer. Reported from a real installation: the first run
found nothing and showed no prompt, the second showed one, and by the time it
was granted the panel had already said "no devices found" — the only visible way
forward being to close it and open it again.

## Testing

A test binary in a custom target directory cannot find the bundled libraries
(the loader resolves relative to the binary, which is not where the build script
copied them), so test runs need the library directory added to the fallback
search path. The repository's Cargo configuration points at Windows SDK paths,
so macOS builds override the FFmpeg and libclang locations; the npm scripts do
this for the normal build and the README states the form for a bare `cargo`
invocation.
