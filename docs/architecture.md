# Architecture

Frame Player is a Tauri 2 application whose video is decoded by **libmpv in the
same process** and rendered into a native child view *behind* a transparent
webview. The entire interface is HTML composited on top of the picture.

Everything difficult about the player follows from that one sentence, so it is
worth being precise about what it means.

## The embedding model

mpv is not a library that hands you frames. It is a player that owns an output
surface, and it can be told to draw into a window that already exists — the
`--wid` option, which takes a native window handle (an `HWND` on Windows, an
`NSView` on macOS).

So the window has three layers:

```
   webview (transparent)      ← the whole interface: bars, menus, seekbar, overlays
   mpv's child view (wid)     ← the picture, drawn by mpv's own GPU output
   the window's own surface   ← an opaque fill, visible only where neither painted
```

The consequences are constant companions:

**The video is not in the DOM.** It cannot be styled, transformed, clipped by a
CSS rule or captured by a canvas. Anything that looks like it affects the
picture — zoom, pan, rotation, aspect, screenshots — is an mpv property or an
mpv command, never a webview operation.

**"Nothing painted" means "the desktop shows through."** The window is
deliberately transparent; a moment when neither mpv nor the webview has painted
is a hole. Hence an opaque backdrop that holds until mpv's output is up, and an
opaque window background color underneath both — erring long costs nothing,
erring short shows the desktop.

**Two compositors have to agree about geometry.** Fullscreen transitions,
window resizing and the mini player each involve the webview relayouting and
mpv's view being resized, and they do not happen in the same frame. The
sequences that mask this (a black shutter window during a fullscreen change,
waiting a presented frame before showing it) are not decoration.

### Why this and not the alternatives

**External mpv driven over JSON IPC.** An extra process, extra milliseconds on
every command, and a second thing to ship and keep alive. Rejected.

**A render-API presenter** — our own child window plus an OpenGL/Vulkan context,
`mpv_render_context` and a texture ring. This is the endgame upgrade: it would
give frame-exact control, sub-5 ms stepping and perfect color agreement with
whatever we draw. It costs the hardest kind of native work on both platforms
(compositing beneath the webview, DPI, fullscreen, HDR), and the current
architecture does not preclude moving to it later.

**A `<video>` element.** Format coverage, hardware decoding and HDR would all
become the webview's problem, which is the reason people install a real player
instead of using a browser.

## The constraint list

These are not style preferences. Each one was found by breaking something.

**Never read an mpv property in `node` format, and never observe one in it.**
The wrapper's node deserialiser corrupts the stack. Lists are read through their
sub-properties instead (`track-list/count`, `track-list/N/title`), which every
list property in mpv supports.

**Never broadcast a `script-message`.** Only targeted `script-message-to`; the
broadcast path crashes in the wrapper's client-message serializer.

**State mirrors go stale.** mpv's event queue can overflow and drop
property-change events. Toggles therefore use `cycle`, never
`set(prop, !mirror)`, and every mirror is re-read on a timer and on
`queue-overflow`.

**Several properties are never visually atomic.** Two `set_property` calls let
the video output redraw between them. Where a pair must land in one frame —
zoom together with pan — it goes through a Lua script running on mpv's own
thread.

**Seek flags are a performance contract.** With `hr-seek=yes` every seek without
an explicit flag is exact, and an exact seek costs decoding from the preceding
keyframe. The cost is a property of the *file*, not of the distance jumped:

| file | `absolute+exact` | `absolute+keyframes` |
|---|---|---|
| HEVC 4K 25 fps, 2 s GOP | 25 ms | 25 ms |
| H.264 4K 60 fps, 2 s GOP | 36 ms | 37 ms |
| VP9 4K 60 fps, 5 s GOP | 1219 ms | 50 ms |

On an ordinary file keyframe seeking buys nothing, while *switching modes
mid-gesture* is visible as a jerk. So the player probes the first exact seek on
each file and picks one mode for it: fast file → exact everywhere; slow file →
keyframe previews with exactness only where a frame is actually being asked
for. Hardware decoding halves keyframe seeks and doubles exact ones, so "turn
hwdec off" is not a way out.

**Everything the player needs at runtime is bundled.** libmpv, its wrapper and
the whole dependency closure ship with the application; nothing is taken from
the user's system. The single exception is `yt-dlp`, which is optional by
design, looked up at runtime, and updates itself.

**Nothing may block player startup.** Whatever waits during initialization holds
up the window, and until the player is up a click on a file does nothing — which
reads as a dead application rather than a slow one. Discovery, torrent sessions,
external-binary probes and network calls are all on demand.

## Latency: where it went

The player is built around gestures that are expected to be instantaneous, and
each one has a specific reason it is:

**Seeking** — the mode probe above, plus exactly one seek in flight at a time.
A fixed-interval pump builds a queue that keeps playing out after the fingers
stop; one-in-flight does not.

**Hover previews** — a background storyboard decodes the file into a disk cache
after it opens, so hovering is a cache read. Decoding is budgeted: a capped
thread count, a duty cycle, and a background quality-of-service class, because
the storyboard competes with playback and an unbudgeted one makes seeking feel
broken. The class matters in both directions — the lowest one pins the work to
efficiency cores and the storyboard then arrives after the viewer has already
reached for it.

**Frame stepping** — a sidecar decoder that pre-warms a GOP ring on pause, so
the first step is instant. Currently disabled behind a flag: mpv's own stepping
turned out to be good enough, and the sidecar's canvas path could not reproduce
mpv's HDR tone mapping, so the frame it showed did not match the frame the
player was showing. Kept working and smoke-tested.

**Screenshots** go through mpv, which keeps exactly what the video output
produced, HDR tone mapping included. The compression level is lowered
temporarily for the throwaway copy that reaches the clipboard: encoding a 4K PNG
at the default level costs 1.5 s on mpv's core thread, which stutters playback
as well as making the user wait.

## Where the code lives

| Path | What is there |
|---|---|
| `src/routes/+page.svelte` | Markup, CSS, and everything gesture-shaped: seekbar, wheel, hotkeys, menus, overlays |
| `src/lib/player.svelte.ts` | mpv state mirrors, initialization, commands, tracks, file loading |
| `src/lib/*.svelte.ts` | State modules: history, playlist, torrents, casting, thumbnails, zoom, window prefs |
| `src-tauri/src/lib.rs` | Plugin registration, window lifecycle, the small commands |
| `src-tauri/src/thumb_service.rs` | Seekbar thumbnails and posters: decoding, scoring, disk cache |
| `src-tauri/src/torrent.rs` | The torrent session and the loopback HTTP server |
| `src-tauri/src/cast.rs` · `dlna.rs` | Casting: the CASTV2 client, the LAN file server, the UPnP transport |
| `src-tauri/src/step_engine.rs` | The frame-stepping sidecar (disabled, kept working) |

State lives in `.svelte.ts` modules rather than a store library: Svelte runes
work outside components, so a module exporting a class with `$state` fields *is*
the store, with per-field reactivity. Two rules keep that honest — never export a
`let` that gets reassigned (the importer would get a snapshot), and keep the
dependency direction one-way, with the page as the place where modules meet.
