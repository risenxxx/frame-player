# Linux

How Frame Player draws a video on Linux, why it is different from the other two
platforms, and what was measured. The rules that break the player when broken
are in [rules/mpv-playback.md](rules/mpv-playback.md) (the Linux line); the
build, the bundle and the container harness are under "Linux" in
[distribution.md](distribution.md).

## Why not `--wid`

On Windows and macOS mpv is handed a native view (`wid`) and draws into it, and
the transparent web view sits on top ([architecture.md](architecture.md)). Linux
offers neither half of that:

- **Wayland has no foreign-window embedding at all.** There is no handle to give
  mpv; the plugin's own `get_wid` answers "not supported" and mpv opens a window
  of its own.
- **On X11 the order is wrong.** mpv would create a child window inside ours, and
  an X11 child window is drawn *over* its parent's other content — over the web
  view, not under it. The interface would vanish behind the picture.

So on Linux the picture is not mpv's window but a texture we draw: libmpv's
**render API** (`vo=libmpv`, `mpv_render_context_render`) paints into a
`GtkGLArea`, and the `WebKitWebView` sits over it in a `GtkOverlay`. GTK
composites the two layers itself, inside one surface, on both X11 and Wayland —
no compositor, no subsurface, no second toplevel. This is the "render-API
presenter" [architecture.md](architecture.md) calls the endgame upgrade, done on
the one platform where the other model does not exist; Windows and macOS keep
`wid`.

## The GTK tree, and the one constraint on it

Tauri (tao + wry) builds `GtkWindow > GtkBox (tao's default vbox) > WebKitWebView`.
The plugin rebuilds it, once per window, into

```
GtkWindow > GtkOverlay > [ GtkGLArea (main child, bottom),
                           WebKitWebView (overlay child, top, takes the input) ]
```

**The web view's grandparent must stay the `GtkWindow`.** tauri-runtime-wry
attaches a resize handler to every Linux web view (`undecorated_resizing.rs`)
whose `button-press-event` does
`webview.parent().and_then(|w| w.parent())` → `downcast::<gtk::Window>().unwrap()`.
The obvious placement — the overlay inside the vbox — passes every test until
the first click, which aborts the process ("panic in a function that cannot
unwind"). So the overlay *replaces* the vbox as the window's child and the vbox
leaves the tree. A GTK menubar packed into it (Tauri's menu on Linux) would
therefore never show; Frame Player has none.

The tree is rebuilt from `setup`, right after the window is created and before
its page loads (`prepare_window` in the plugin, called from lib.rs) — the order
the prototype proved; `init` rebuilds it itself if that call is ever skipped.
Taking the web view out of the vbox unsets it as the window's focus widget, and
nothing gives that back until a click: measured, Space did nothing until the
first click on the seekbar. So the web view grabs focus again afterwards.

The `GtkGLArea` is windowless and never focusable, so hover, clicks, keys and
drag-and-drop reach the web view exactly as before. It is opaque
(`has_alpha=false`: GTK backs it with a renderbuffer and blits), drawn only when
mpv has a frame (`auto_render=false`, a redraw queued from libmpv's update
callback), and black while no core is attached.

## WebKit has to be compositing

WebKitGTK keeps the page's transparent pixels transparent only in accelerated
compositing mode. Outside it (`DrawingAreaProxyCoordinatedGraphics`), it paints
an **opaque black backdrop** wherever the page is transparent. On a desktop with
a compositor WebKit is in that mode anyway; on a bare X11 screen it is not, and
the result is the worst kind of failure — a working interface over a black
window, with mpv's frames correct all along underneath. Measured under Xvfb with
no compositor in the prototype: hiding the web view showed the video, and mpv's
FBO read back the right colours.

So `linux_webkit_env` (lib.rs) sets `WEBKIT_FORCE_COMPOSITING_MODE=1` before the
builder — the web process inherits the environment when it is spawned — unless
the user set it. Two variables people set to work around WebKitGTK driver bugs,
`WEBKIT_DISABLE_COMPOSITING_MODE=1` and `WEBKIT_DISABLE_DMABUF_RENDERER=1`
(WebKit 2.52: any value but `0` leaves the transport empty and falls back to the
non-composited path), produce the same black; they are reported on stderr, not
undone. `WEBKIT_DMABUF_RENDERER_FORCE_SHM=1` keeps compositing with shared-memory
buffers and is the workaround to suggest instead.

## Order of events

Every one of these was found by breaking it in the prototype:

1. **Signals before the tree.** The area's `realize`, `render` and `unrealize`
   handlers are connected before it is added: when the window is already mapped,
   `show_all()` realizes the area synchronously and a handler connected
   afterwards never runs.
2. **A render context before the first `loadfile`.** `vo=libmpv` refuses to open
   without one (`No render context set`, then "Error opening/initializing the
   selected video_out"), and the frontend loads the first file as soon as `init`
   resolves. So `init` does not resolve until the context exists: it creates the
   core (options, observers, `mpv_initialize`), hops to the GTK thread through
   `with_webview`, builds the overlay if this window has none yet, realizes the area
   if it is not (the main window is created hidden and shown by the frontend
   concurrently with `init`; realizing an unmapped widget creates its GL context
   without showing anything), creates the render context there, and only then
   starts the event thread and returns. No GL is an `init` failure that names the
   reason, which the start-failure screen shows.
3. **`LC_NUMERIC=C` before `mpv_create`.** `gtk_init` sets the locale from the
   environment and mpv refuses to create a handle under a decimal comma. The
   plugin's setup sets it, and `Core::create` again right before the call.
4. **Teardown: the render context, then the core.** The context is freed on the
   GTK thread with the area's GL context current — in `destroy` through
   `run_on_main_thread` (inline when already on it), or in the area's `unrealize`
   handler when the window goes first (`unrealize` is `RUN_LAST`, so the user
   handler runs while GTK's context still exists). Only then
   `mpv_terminate_destroy`, then the event thread is joined. `RunEvent::Exit`
   destroys whatever core is left, so the process never exits with mpv's threads
   running. A core whose render context could not be freed (the GTK thread did
   not answer in 10 s) is leaked rather than destroyed.
5. **Re-initialisation reuses the overlay.** `session.ts` may call `init` twice
   for one window — the viewer's mpv.conf is dropped and mpv started again
   without it. The second core takes over the same area; the overlay is never
   nested.

## Rendering

In the area's `render` handler: the FBO GTK bound (`GL_DRAW_FRAMEBUFFER_BINDING`),
its size in device pixels (allocation × scale factor; with `GDK_SCALE=2` the
allocation stays logical and the FBO doubles), `FLIP_Y=1` (GTK treats the area's
FBO as y-up), and `mpv_render_context_report_swap` right after the render — GTK
composites once the handler returns, which is the closest honest swap point.

`ADVANCED_CONTROL=1` with **`BLOCK_FOR_TARGET_TIME=0`** (the prototype's
measurement). With the default block,
the render waits inside the GTK thread for the frame's target time; on a starved
main loop (llvmpipe at 1920×1200 device pixels, ~14 renders/s) that wait made
every render come out black and stay black after the window shrank again. With
the block off it never happened; `ADVANCED_CONTROL=0` is the fallback if frame
timing ever misbehaves on real hardware.

**On a software renderer the intermediate framebuffers are 8-bit.** mpv renders
through `rgba16f` framebuffers by default. On llvmpipe that came out black for
the whole run in **9 of 26 starts** — read back from the area's own FBO, so
mpv's output and not the page; render rate, mpv's properties (`vo-configured`,
`osd-dimensions`, zoom, pan, margins, equalizer), the GL state before the
render (scissor, blend, viewport, colour mask, bound framebuffers) and the
current GLX context were all identical to a good run's, and neither turning
advanced control off, nor a window that never resized, nor an empty shader
cache changed the rate. With `fbo-format=rgba8`: **0 of 16**. So when
`GL_RENDERER` names llvmpipe or softpipe — what a machine without a GPU driver
gets: a virtual machine, a broken driver install — `fbo-format=rgba8` is set
before the render context is created, and the player says so on stderr. A GPU
keeps mpv's default; whether a real driver ever does the same is unknown, and
the first thing to try if a Linux machine reports a black picture under a
working interface.

The `get_proc_address` libmpv needs comes from the loader that owns the current
GdkGLContext — GTK 3's X11 backend is GLX-only (`glXGetProcAddressARB` from
`libGL.so.1`), its Wayland backend EGL (`eglGetProcAddress` from
`libEGL.so.1`); libepoxy, GTK's own dispatch, exports no lookup function.

**The X11 display is passed to mpv, the Wayland one is not.**
`MPV_RENDER_PARAM_X11_DISPLAY` lets VA-API/VDPAU interop work over GLX. Given
`MPV_RENDER_PARAM_WL_DISPLAY`, mpv's VA-API interop calls `vaInitialize` on the
compositor connection inside `mpv_render_context_create`, and Ubuntu 24.04's
libva-wayland crashed the process there (SIGSEGV, backtrace
`libva-wayland → vaInitialize → mpv_render_context_create`) under a compositor
that offers no `wl_drm` — a headless Weston in the harness, but not only there.
`hwdec=no` did not avoid it (mpv loads the interop when the context is
created). Without the display, Wayland decodes in hardware through the copy
path (`vaapi-copy` opens its own DRM device) or in software, and starting up no
longer depends on the compositor.

## Threads

- **The GTK main thread** owns every widget and GL call: the overlay, the render
  context, rendering, freeing.
- **libmpv's threads** call the update callback, which does nothing but post a
  `queue_render` to the main context through a weak, thread-safe reference — a
  callback for an area already gone does nothing.
- **The event thread** (`mpv-events`, one per core) waits on the `event-client`
  and emits each event to the window directly, in order. Emitting only queues the
  message for the web view, so it never waits on the GTK thread.
- **The plugin's commands** run on Tauri's worker threads, serialised per window
  by the instances mutex as on the other platforms. `init` waits for the GTK
  thread *without* holding that mutex, so a synchronous command that runs on the
  main thread cannot deadlock against it.
- mpv's video output waits for a render (up to 200 ms, then it logs "not being
  called or stuck" and moves on) before it continues. A synchronous Tauri command
  — those run on the main thread — that calls mpv can therefore stall the frame it
  is waiting for. Nothing in the app does that on a hot path: the plugin's
  commands are asynchronous, and `zoom_pan` (synchronous) is only the fallback
  for a `zoompan.lua` that did not load.

## The wrapper contract, and the defects not copied

On Windows and macOS the plugin drives `libmpv-wrapper`, a C-ABI library, and
the frontend (`tauri-plugin-libmpv-api`, unchanged) was written against what it
does. On Linux `client.rs` does the same against `libmpv.so.2` directly, and the
frontend sees no difference:

- two clients — `main` for every call, `event-client` (that exact name, the one a
  `script-message-to` would target) for observing and waiting;
- options through `mpv_set_option_string` in the order given — booleans as
  `yes`/`no`, numbers as written, `null`/arrays/objects skipped;
- observed properties registered with ids `1..=N` before `mpv_initialize`;
- events as `{"event": "<kebab-case>", <snake_case fields>}`, a property change
  as `{event, name, data, id}` with `null` for an unavailable value in any
  format, `end-file` reasons `eof|stop|quit|error|redirect`;
- commands through the string-array `mpv_command`, every argument a string (a
  boolean becomes `true`/`false`, as there — mpv refuses that for a flag),
  synchronous;
- `set_property`: a string as a string, a boolean as FLAG, **every number as
  DOUBLE** (an integer-typed property refuses it, as there);
- `get_property` by format, an unknown format name meaning `string`, a success
  without data (a non-finite double) reported as "FFI response contained no data";
- the error texts, prefix for prefix ("mpv command failed: Failed to execute
  command '…': …", "Failed to set property: Failed to set property '…': …", …).

Three defects of the wrapper are deliberately not reproduced:

- **`get_property(…, "node")`** passes the address of a real 16-byte `mpv_node`
  and frees it with `mpv_free_node_contents`. The wrapper passed an 8-byte
  pointer where mpv writes 16 bytes — the stack corruption the app's rules work
  around. The rule stays (the other platforms still have the wrapper), so nothing
  in the app relies on the fix.
- **A client message is read for `num_args` arguments.** The wrapper walked the
  array to a NULL that is not there, which is why a broadcast `script-message`
  crashed it. The rule against broadcasts stays, for the same reason.
- **A failure after `mpv_create` neither hangs nor loses its reason.** The
  wrapper terminated the core with its event client still alive, and mpv's
  shutdown waits for every client — `init` never returned and every later call
  on the window blocked behind it. Here the event client is destroyed first, and
  the error names the refused option (`Failed to create mpv instance: Failed to
  set option '…': …`), which is what `player.confError` shows. The mpv.conf
  retry in `session.ts` can only work because of this.

Also different, on purpose: events are emitted in order from the event thread
(the plugin spawned a task per event), and the event thread is joined on destroy.

## Options that are not the same

The frontend passes the same initial options everywhere except `vo`: on Linux it
says `libmpv`, and the plugin enforces it whatever mpv.conf says, because no
other output draws into the area. `gpu-api`, `gpu-context` and the like do not
apply to the render API and are ignored. `hwdec=auto-safe` stays; mpv's VA-API
*interop* needs EGL, which GTK 3's X11 backend does not use, and on Wayland the
display is withheld (above), so in practice hardware decoding on Linux is the
copy path (`vaapi-copy`, `nvdec-copy`, `vdpau-copy`) — slower than zero-copy,
still far cheaper than software. `media-controls` is not passed on Linux: mpv
has no Linux backend for it and the mpv 0.37 Ubuntu 24.04 ships does not know
the option at all — an unknown option of ours refuses initialisation, which is
how the first run of the player in the container ended ("Could not start mpv").
Audio output is mpv's `auto` (PipeWire, PulseAudio or ALSA, whichever answers).

mpv's `force-window=yes` tries to open the video output inside
`mpv_initialize`, before the render context exists, and logs `No render
context set` / "Error opening/initializing the VO window" at fatal level about
half the time. It is harmless: the output opens with the first file.

## What was measured, and where

In Docker (OrbStack on an Apple-silicon Mac): Ubuntu 24.04 arm64, GTK 3.24.41,
WebKitGTK 2.52.6, libmpv 2.2 (mpv 0.37), Mesa 25.2 llvmpipe, no GPU, no sound
server. `scripts/linux/check.sh run-x11` / `run-wayland` build the real player
(debug, frontend embedded), generate a 60 s `testsrc2` video with a tone and a
keyframe a second, play it and photograph it (`scripts/linux/run-app.sh`).

X11 (Xvfb with GLX, no window manager, no compositor), every run after the
fixes above — 16 in a row:

- the picture plays under the HTML interface: the title bar, the seekbar, the
  buttons and the gradients over a moving video; two shots 1.5 s apart differ
  by 13–28 k pixels in the picture's middle;
- Space pauses (two shots 1.5 s apart: 0 pixels differ) and resumes;
- `L` while paused moves the picture 10 s; a click on the seekbar seeks there;
- hovering the seekbar shows the thumbnail (the FFmpeg sidecar, unchanged);
- Ctrl+Q exits with status 0, in the order `render context freed → mpv
  destroyed`.

Wayland (headless Weston, GL renderer on llvmpipe): the picture plays and
fills the window; the interface is idle-hidden because nothing can move a
pointer in a headless Weston, so neither the controls nor a quit could be
exercised there — the run ends with SIGTERM (status 143 at the time; the
player now handles it, below).

`cargo clippy -D warnings` and the tests pass on Linux (the crate, and the
plugin's own three tests for the JSON conversions and the `mpv_node` layout,
which the crate's run does not reach).

## Measured on a desktop

Ubuntu 26.04 (GNOME 50, Wayland — GNOME no longer has an X11 session; mpv
0.41, WebKitGTK 2.52.6, GTK 3.24.52, Mesa 26.0), an RTX 5080 on **nouveau**: GL
is zink over NVK, a 3840×2160 60 Hz screen at **150 %** (Mutter's fractional
scaling, so GTK 3 draws at 2× into a 5120×2880 buffer in fullscreen and the
compositor scales it down). `FP_RENDER_STATS=1` prints the area's renders per
second, their cost on the GTK thread and the FBO's size every two seconds.

- **The picture** plays under the interface from the first start: render
  context over EGL, `rgba16f` intact (no black frames), no tile grid after
  resizing — that was llvmpipe's.
- **Fullscreen crashed the process, every time**, until the shutter window
  became Windows-only: Mutter disconnected it with
  `wp_linux_drm_syncobj_surface_v1: Explicit Sync only supported on dmabuf
  buffers` on the hidden `veil` web view's surface (`WAYLAND_DEBUG=client`).
- **The picture stuttered whenever the controls were up**: 13 renders a second
  of a 30 fps video with the interface on screen, 30 without it. mpv's own
  render cost 5 ms either way; `perf` put 92 % of the GTK thread under
  `gdk_cairo_draw_from_gl` called by WebKit, ending in NVK's
  `nil_copy_linear_to_tiled` — GTK uploading the window's whole cairo surface
  for a blend nothing needed (see the rule in
  [rules/mpv-playback.md](rules/mpv-playback.md)). After the overlay draws the
  web view itself: 30 renders a second, 6.4 ms each, controls pinned.
  `WEBKIT_DMABUF_RENDERER_FORCE_SHM=1` was worse (9.5 a second).
- **Shadows**: the title bar's `drop-shadow` filters came out as dark wedges
  and the logo was clipped with WebKit's GPU rasterisation, and were right on
  the CPU — hence the nouveau switch.
- **Hardware decoding: none.** On nouveau mpv tries vulkan, nvdec, vaapi and
  vdpau (and their `-copy` forms) and every one fails (`vainfo`: the nouveau VA
  driver does not initialise on this GPU; NVK offers no video decode to
  FFmpeg; no CUDA), so `hwdec-current` is `no` and decoding is software. With
  the proprietary driver this would be nvdec — not measured: the session ran
  from a live image, where it cannot be installed.
- **4K, software-decoded, window the size of the screen**: H.264 4K60 plays at
  60 renders a second (10 ms each). HEVC 10-bit 4K60 rendered at 30 a second,
  20 ms each, 297 frames dropped in 12 s: 87 % of the GTK thread was NVK's
  `nil_copy_linear_to_tiled` under `mpv_render_context_render`, the upload of
  each decoded frame. `opengl-pbo=yes` brought it to 60 at 1.3 ms;
  `vd-lavc-dr=yes` to 60 at 0.3 ms with nothing dropped, which the plugin now
  sets on NVK.
- **Audio**: `ao=pipewire`.
- **Signals**: SIGTERM, SIGINT and SIGHUP used to kill the process (143) with
  mpv never torn down. `linux_signals` (lib.rs) answers them on GTK's main loop
  with the same exit as Ctrl+Q: status 0 in ~100 ms, render context freed,
  then mpv destroyed.
- **The window by hand** (GNOME 50): moving from the title bar and the
  picture, double click to maximize, the window buttons, hotkeys from the
  first frame, the mini player, dropping a file from Files, Ctrl+Q — all
  work. "Match video aspect ratio" did not hold the shape at all, the bars
  stayed up through a resize, every other drag did nothing and the first click
  after a drag was lost; fixed as the window rule describes. Not yet done:
  rounded corners (the window is square; on Windows 11 and macOS the system
  rounds it, and here the GL layer under the page is opaque), and on entering
  fullscreen the window's old frame is seen moving to the top-left corner for a
  moment at the screen's edges.
- **X11 through XWayland** (`GDK_BACKEND=x11`; GNOME 50 has no X11 session):
  the render context over GLX, the picture and the interface right, 1080p at
  30 renders a second with the controls up, signals clean. The overlay's own
  drawing of the web view made the interface disappear entirely there, so it is
  Wayland-only. 4K HEVC 10-bit 60 fps tops out at 46–50 renders a second with
  frames dropped, the overlay drawn either way — a limit of the X11 path on
  this driver, not investigated further.
- **Single instance**: a second start with a file hands it to the running
  player over D-Bus and exits 0; the running one plays it.

## What only a real desktop can answer

Everything above ran on a software renderer, without a compositor on X11 and
without input on Wayland. On real hardware, in rough order of risk:

- **A GPU driver**: the render path, the frame rate at 4K, `rgba16f` without
  the software fallback, and resizing — the prototype saw a tile grid after
  growing the window under llvmpipe/Xvfb that it attributed to llvmpipe.
- **Hardware decoding**: which `*-copy` decoder `auto-safe` picks on Intel, AMD
  and NVIDIA, and what it costs; the settings footer shows `hwdec-current`.
- **A real Wayland session** (GNOME, KDE): input, the window's own resize edges
  and dragging (Frame Player draws its own chrome: `decorations: false`), the
  minimum size and "match video aspect ratio" with a compositor in charge of
  geometry, fractional scaling, and whether withholding `wl_display` from mpv
  is still needed there.
- **HiDPI on X11** (`GDK_SCALE=2`): verified in the prototype, not with the
  player.
- **Fullscreen and the mini player**: written for Windows and macOS; the GTK
  window is the same widget, but nothing was exercised.
- **Audio** through PipeWire/PulseAudio (the container had none) and the
  per-device offset, which has no Linux backend.
- **Closing from the window manager** (the close button of a decorated
  session, `wmctrl -c`, logging out): that path goes through the plugin's
  `CloseRequested` handler rather than the player's own ✕/Ctrl+Q.
