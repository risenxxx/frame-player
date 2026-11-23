# Handing mpv's frames to a native UI framework

What was measured and agreed in November 2026 while evaluating a move from
today's shell (a transparent WebView with mpv drawing into a child window behind
it, see [architecture.md](architecture.md)) to [Skathi](https://github.com/argon-chat/skathi),
a native UI framework that draws the interface itself and takes video as a GPU
texture through its `<sk-surface>` element. The player is not built yet; two
throwaway prototypes are, one per platform (see "The prototypes on Skathi,
measured"). The point of this document is that the implementation starts from
these findings instead of re-measuring them.

Every number below comes from a probe that was run, unless it is marked as
something else: a re-read of an earlier run's logs, a figure from source, a
diff count, a simulation, or a measurement by Skathi's maintainer. The prototypes' sections
give the conditions per stage or batch; where a summary pools runs, the
section it summarises has the per-run values. The probes themselves were
throwaway programs and are not in the repository; "Reproducing the probes" at
the end says enough to rebuild the earlier probes and the Vulkan offscreen
proof. The prototypes are described by their sections, not step by step.
Two machines were used: an Apple M3 Max laptop (built-in XDR display, 120 Hz)
and a Windows desktop (build 28000) with an RTX 5080 (driver 610.62) and a Gigabyte MO27U2
monitor (3840×2160, 240 Hz, Windows HDR on, SDR white level 240 nits, reported
peak 1000 nits).

## Decided for version 2

Settled in November 2026, after the Skathi 0.4 prototypes on both platforms
and the Skathi fixes that followed them. The prototypes' measurements follow
this section. The sections from "The shape of the design" to "Other findings
along the way" were written from the earlier probes; where they disagree with
the prototypes, the prototypes are the newer evidence, and the passages known
to disagree are marked superseded.

- **The renderer is gpu-next on both platforms, through the render API.**
  The way there is mpv PR #18845 ("libmpv: use the gpu-next renderer by
  default in the render API", opened 2026-11-17 by an mpv maintainer, not
  merged at the time of writing). It adds `MPV_RENDER_PARAM_RENDERER` and a
  gpu-next render backend that builds libplacebo on the same API contexts the
  old renderer uses (`libmpv_gpu_context_create` → `gpu_ctx_create`). Until it
  is merged we carry it as a patch; a change to its API before the merge means
  updating the patch.
- **Windows: gpu-next on D3D11.** mpv renders with its D3D11 render API on our
  own device into shared `R16G16B16A16_FLOAT` textures (one fence per texture,
  CPU wait for the slot), now with the gpu-next renderer selected. On the
  `<sk-surface>` route Skathi stays on its D3D12 renderer and opens the
  textures there; on the native-layer route mpv renders into a swap chain of
  ours instead (see "Not decided" below). This needs a Windows
  libmpv build of our own (the prebuilt one cannot carry the PR); the PR's CI
  artifacts are for testing only.
- **macOS: a Vulkan render API, by a patch of ours.** On top of PR #18845, a
  `vulkan` backend for the render API: the client supplies a `VkImage`
  imported from an IOSurface (`VkImportMetalIOSurfaceInfoEXT`) and a timeline
  semaphore exported as an `MTLSharedEvent` (on the `<sk-surface>` route
  Skathi waits on it; on the native-layer route our own present does); libplacebo
  renders through MoltenVK, i.e. Metal underneath. The backend is our patch
  on a PR #18845 checkout (`aade20e8c`), now 9 files, +1014 −29. It passes
  an offscreen regression harness and runs in the Skathi prototype at the
  GL path's cadence and colour, without a CPU wait per frame, and under the
  ported interface (below).
  Route A, the OpenGL render API, works with today's 0.41 libmpv, which renders
  it with the old `gpu` renderer (0.41 ignores the renderer parameter);
  gpu-next on it needs PR #18845. That is the interim step only: OpenGL 4.1 has
  no compute shaders, so part of gpu-next is unavailable there.
- **Deferred:** libplacebo backends of our own for Metal or D3D12. A custom
  backend would leave the current Skathi copy path in place unless that
  interface also changes; its own cost is unknown, since none was built. It
  would be tested on far less hardware than libplacebo's Vulkan and D3D11
  backends; its real gain would be platform features (MetalFX, DirectSR).
  To be reconsidered after version 2 is stable.
- **Not decided: how the picture reaches the screen.** Everything above
  assumes the `<sk-surface>` route (mpv renders into textures Skathi copies
  and composes). A native video layer under a transparent page, mpv
  rendering straight into a swap chain or `CAMetalLayer` of ours, has since
  run as a prototype on both platforms on Skathi 0.7 without Skathi changes
  and removes the copies, the ring and the overlay pass (see "A native video
  layer under the page"). Since 2026-11-22 Skathi supports it as the
  `<sk-layer>` element and the `Layer` trait (#44), and the port runs on it
  on both systems. The render-API decisions above hold on either route.
  As the measurements stand (2026-11-23; "On Skathi's own layer" and "A
  lighter render set in the port"), without deciding it:
  - **For `<sk-layer>`**: lower memory and GPU time wherever the picture is
    large (macOS 2160p PQ screen-sized 1718 against 1935 MiB, app GPU time
    12.2 against 17.9 %; Windows 2160p HDR10 1228 against 1684 MiB of GPU
    memory, 6.26 against 7.67 % GPU time); the window stays 8-bit under
    HDR on both systems; mpv scales to the window itself, so its scalers
    and the upscaler act (on `<sk-surface>` mpv draws at about the video's
    size and neither does); subtitles need no overlay pass. On macOS it
    moved in the page's frame in every captured frame of a native live
    resize, and it follows the mini player's panel.
  - **For `<sk-surface>`**: it is the route the whole port was built and
    measured on; nothing of ours sits outside Skathi's tree; on macOS at
    1080p and in the mini player the two routes are within 1–3 % in total
    memory (on Windows the layer route holds 45 MiB less GPU memory at
    1080p and 94 less in the mini player). On
    Windows the layer was one desktop refresh behind the page while the
    window grew (measured on `04e24dee`, before Skathi changed the commit
    order for it; not re-measured).
  - **Open for both**: the AOT build of the layer route, the context menu
    over the picture, a real SDR desktop on Windows, the HDR target (below,
    "Still open").
- **macOS: the window takes its drawables through `NextDrawable`.**
  `Builder::metal_frame_source(MetalFrameSource::NextDrawable)` (Skathi
  `329afd60`, #49) keeps three screen-sized window drawables where the
  default `Auto` and `DisplayLink` keep five on a visible window: −55 MiB
  at 2160p SDR, −50 (`<sk-layer>`) and −104 (`<sk-surface>`, whose window
  is half float) at 2160p PQ, with the same median cadence, GPU and CPU
  time (exceptions under "The window's frame source"), and
  no stretched page frame in a live resize, where `DisplayLink` showed
  52–55 per run (measured on `ce6aaca2`, 3 launches a cell; "The window's
  frame source"). The maintainer's stated costs of a pinned
  `NextDrawable`, a frame that waits on the UI thread when every drawable
  is in use and present feedback without a target time from the link, did
  not show in these runs; pacing under load was not tested.
- **Recommended, not yet decided: a lighter mpv render set as the port's
  default.** `profile=fast`, `dither=fruit`, `linear-downscaling=yes`,
  `correct-downscaling=yes`, `hwdec-threads=1`, with the port's
  `hdr-compute-peak=no` kept (called P1 below). Measured on both systems
  and both routes: memory falls in almost every cell (macOS up to −206 MiB
  at 2160p PQ, Windows up to −112 MiB of GPU memory with HDR10). The
  exceptions: on macOS the `<sk-layer>` route at 1080p, +28 MiB
  screen-sized and +6 in fullscreen, from our layer's drawable pool (the
  cap of two since then was re-measured screen-sized only, not in
  fullscreen); on Windows +1 MiB of GPU memory in the small window on
  `<sk-surface>`. GPU time falls; the frame rate held wherever it could be
  read (not in the Windows small window); the HDR peak and banding in the
  test captures are as under P0 within small differences (below).
  **The caveat is the Windows mini player**: the port's own `dscale` is
  hermite, and `profile=fast` makes it bilinear, which leaves faint ghost
  rings on a 4K zone plate (SSIM 0.862 against 0.934). Keep the port's
  `dscale`, or set one explicitly, instead of taking bilinear from the
  profile; a P1 with hermite `dscale` is **not measured**, and whether the
  rings show on real content is not checked. The
  real-content crops from macOS still wait for the owner's judgement. See
  "A lighter render set in the port".

The priority now is to ship version 2 as a beta beside version 1; the
release plan is [v2-beta-release.md](v2-beta-release.md). Improvements
found during this work that version 1 could also use are listed, for later,
in [v1-backports.md](v1-backports.md).

## The prototypes on Skathi, measured

Built and measured in November 2026, on Skathi 0.4.0 and then on its master
up to 0.5.0 (with `skathi-gpui` 0.5.2), as each fix landed. Each platform got
a standalone prototype: an `<sk-surface>` fed by today's libmpv through its
render API, with a small test page over it. Neither prototype is the player.
Their interface is a minimal page. The player's own interface was later
ported onto Skathi on both platforms (the "port"), measured against the
shipping player with every process counted and against native players, and
tried with the picture in a native layer under the page instead of an
`<sk-surface>`; those sections follow the two prototypes'. The prototypes and
the port are throwaway programs and are not in the repository.

The table compares two configurations, not Skathi alone:

- **Old path**: the shipping player's video path without its web view, in a
  native harness: mpv with `vo=gpu-next`, `hwdec=auto-safe` and
  `video-sync=audio` in a native child window or view, presenting through
  mpv's own swapchain.
- **New path**: the prototype: mpv's old `gpu` renderer through the render
  API, `video-sync=display-resample` with our pacing loop, presenting through
  Skathi, in its own harness.
- **Shipping player**: the whole 1.49.0 player, web view included.

Renderer, sync mode, presentation and harness all differ between the first
two, so a difference between their columns is not attributed to any one of
them. Builds and clips: Windows cadence on Skathi `74e63fad`, latency on
`74e63fad` and `c7a524d5`, cost, memory and audio on `c7a524d5`; macOS stage
MB on `c602b5a4` with 1080p clips (`ao=null` for cadence). Each OS measures
cadence differently (see its section).

### Old path against the new one, by parameter

| | Windows (RTX 5080, 240 Hz) | macOS (M3 Max, 120 Hz) |
|---|---|---|
| Cadence | first shown on the intended refresh: old 51–86 %; new 96–97.5 % in a window, 99.3–100 % screen-sized | held the ideal number of refreshes, per clip: 23.976 fps old 74.5–77.6 %, new 96.7–99.2 %; 60 fps old 47.8–99.6 % by run, new 98.8–99.9 %; 24 fps SDR old 98.4–99.7 %, new 97.4–99.3 %; HDR10 24 fps old 92.6–100 %, new 96.1–98.6 % |
| Exact seek, paused | overlapping cell medians (41–56 vs 43–66 ms) | run medians 95–98 vs 138–158 ms (+43…+61 ms by run order) |
| Frame step, paused | the same once both use `display-resample` (44–46 vs 46–49 ms); the old path's 96–99 ms is `video-sync=audio` | run medians 99–101 vs 83–95 ms; single commands overlap (86–106 vs 50–117 ms) |
| Back step, paused | overlapping cell medians (83–94 vs 88–95 ms) | run medians 161–198 vs 216–243 ms (+19…+83 ms by run order) |
| CPU, 24 fps SDR | 2.5 → 4.5 % of a core | 7.9–10.0 → 15.1–29.2 % (old `gpu` renderer, capture running) |
| GPU, 24 fps SDR | 0.4 → 1.0 % | 3.8–4.4 → 9.1–10.6 % (capture running; not comparable across OSes) |
| Memory | private bytes against the shipping player, sizes unmatched: in a window 76–182 MB lower at 1080p, −230…+45 MB at 2160p; screen-sized +164…+213 MB at 1080p, −54…+225 MB at 2160p | `phys_footprint`: 221–225 MB above the bare video path at 1080p, same session; against an unmatched earlier whole-player observation (~845 MB), about 90–140 MB lower |
| GPU memory against the shipping player | +81…+172 MB in a window, about +500 MB screen-sized | not separated |
| Audio | no drift over 60 s by mpv's counters; about +6 ms `audio-delay` inferred, not tested | audio clock minus captured picture +51.8…+53.0 ms (old path +9.1…+11.1); `audio-delay=+0.050` tested in this setup (+4.9…+5.8 ms left); at 24 fps audio held at mpv's −0.125 % correction clamp while the picture ran at 0.9983–0.9989× |
| gpu-next through PR #18845 | works on D3D11 (CI build); colour differs from `gpu` with the same options | initialises, decodes and renders on GL 4.1; a scaler shader error once per file |

On Windows the new path put more frames on their intended refresh in every
cell. On macOS the clear difference is 23.976 fps, and the old path's
run-to-run spread at 60 fps; at 24 fps SDR and HDR10 the ranges overlap. Paused
latency: on Windows the seek and back-step medians overlap, and the forward
step is the same once the old path also uses `display-resample`. On macOS the
new path's run medians are higher for seeks and back steps. Stage MM reported
44.9–48.0 ms median from present to `shown_at`, and most of that is not
Skathi's: a native app paced by `CAMetalDisplayLink`, as gpui paces windows on
macOS 14+, takes as long (see "Latency" under macOS).
The new path costs more CPU and GPU. Memory depends on the window size: on
Windows the prototype's private bytes rise by about 320–370 MB and its GPU
memory by 580–610 MB from the 1280×720 window to screen-sized. An empty Skathi
window has no such step; it comes with presented frames, and the GPU half of
it is taken apart under "Memory" for Windows. The private-bytes half is the
driver charging each unshared texture to the process; the working set stays
flat. A ring of 2 and a render at the video's size, both on our side, bring
a 1080p clip at screen size 100–146 MB below the shipping player in private
bytes and to 58–76 MB above it in GPU memory. The cost is picture sharpness
while subtitles show (see "Memory" for Windows). The recommended first step
renders at the element's size while a subtitle track or an mpv OSD message is
on, and subtitled playback then gives up this saving. The ported interface
keeps the ring of 2 and the video-sized picture, with subtitles in an overlay
of their own; how it compares with the shipping player, every process
counted, is under "The ported player against the shipping one".

### Windows: the prototype, measured

A standalone prototype plays video through mpv's D3D11 render API into shared
textures that Skathi shows in an `<sk-surface>`, with a small test page drawn
over it. Ranges are the minimum to the maximum of the runs of a cell; "n" is
the number of runs. Everything comes from one machine, in one main display
mode (3840×2160, 240 Hz, HDR on); 120 Hz and 60 Hz were run separately, once
per cell, on `1eacd896`. A window called 1280×720 is that size at 150 % scale,
so its picture is 1920×1080 pixels, on both the old and the new path; the
shipping player measured for memory had another size (see "Memory").
"Desktop Duplication" times below are the first captured desktop frame that
shows a picture. How the capture's coalesced updates, cursor updates and gaps
were handled is not recorded in this document; treat that part as unverified.

#### Setup and build

- Hardware: RTX 5080, one 3840×2160 display at 240 Hz, HDR on (SDR white 240
  nits, peak 1000 nits), 150 % scale, Windows 11.
- **The driver caps every swap chain at 224 presents a second.** The
  NVIDIA global profile on this machine has `Frame Rate Limiter` = 224.
  This was read from the profile, and the setting was left as it is.
  - **Bare control:** a D3D12 swap chain without Skathi, built like gpui's,
    presents 224.01 times a second (10 runs).
  - **What it affects:** anything that presents near 240 a second is capped,
    and one-present-per-refresh tests lose one frame in fifteen to it.
  - **What it does not affect:** 24 and 60 fps playback presents far below
    the cap.
- mpv: libmpv `v0.41.0-1303-ga39482e7a` with the `gpu` renderer, and `d3d11va`
  zero-copy decoding on all five test clips (24, 23.976 and 60 fps SDR, HDR10
  at 24 fps, and a 2160p HDR10 clip). The gpu-next subsection uses the
  CI build of mpv PR #18845.
- "Old path" below is libmpv with `vo=gpu-next`, `hwdec=auto-safe` and
  `video-sync=audio` in a child window, with no web view. That is the shipping
  player's video path, not the shipping player. The shipping player itself
  (1.49.0, portable) was measured for memory only.

| Skathi | Measured on it |
|---|---|
| 0.4.0 (`452e24d`) | producer, ring size, frame events, pacing, OSD and subtitles, lifecycle, first cost |
| `1eacd896` | the fixes of issues #1 to #15; 120 Hz and 60 Hz |
| `6cd67304`, `c602b5a4` | device loss; restore and occlusion |
| `74e63fad` | vblank clock, the documented recipe, display off and wake, the cause of late frames, the cadence table |
| `37419f11` | control build, with and without a trial patch |
| `c7a524d5` | `exact_frame_feedback`, latency, CPU, GPU, power, memory, audio, gpu-next |

- The build needed `clang-cl`. The Visual Studio C++ workload installed on the
  build machine did not include it; an unpacked LLVM on `PATH` was enough.
- On 0.4.0 the cargo feature `video` was required, or `<sk-surface>` never
  reported a size (#1). From `1eacd896` it is not required.
- On 0.4.0 a Svelte 5.58 page needed an empty `customElements` registry
  installed before `mount` (#5). The shim is not needed from `1eacd896` on.
- In the tested builds (default features, on both platforms), a git
  dependency on Skathi built only when the devtools bundle was built from a
  clone and named in `SK_DEVTOOLS_BUNDLE` (`sk-app`'s build script runs bun
  in cargo's checkout). Whether a build with the `no-devtools` feature skips
  that was not tried.
- The page payload must be built by the CLI and runtime of the same Skathi
  version as the crate; 0.5.0 refuses a 0.4 payload. Between commits the
  packages kept version 0.4.0, and bun served the previous commit's tarball
  from its cache until the cache entries were deleted.

#### The producer as built

- **Textures.** A ring of `R16G16B16A16_FLOAT` textures shared by NT handle,
  with no keyed mutex. They live on a D3D11 device of the producer's own, on
  the adapter of Skathi's D3D12 device. mpv keeps this device through a Skathi
  device loss.
- **Fences.** One shared `ID3D11Fence` per texture. Per frame the producer
  renders, signals `wait`, flushes, and calls
  `present_shared_fenced(texture, wait, wait + 1)`. One `FrameEvent` arrived
  per accepted present: 201 140 of 201 140 in 81 runs.
- **Ring size: 3.**
  - On 0.4.0 a ring of 4 or 5 locked every later present into `Busy` in 7 of
    9 fast starts (#12). On `1eacd896` the same 9 starts had 0 to 2184 `Busy`,
    none of them permanent.
  - A ring of 2 kept the cadence: 99.1 to 99.9 % of frames on the intended
    refresh, 18 runs (their clips, window size and build are not recorded in
    this document).
- **The pacing recipe that won.** Starts at 0.05 and at 0.50 of the refresh
  differed by less than the run spread.
  1. `video-sync=display-resample`, with `display-fps-override` set from the
     surface's refresh rate at start and on every display change.
  2. One render-loop turn per refresh, started at
     `0.5 − render_time / refresh_period` of the refresh, counted from
     `last_vblank`.
  3. Ask `MPV_RENDER_PARAM_NEXT_FRAME_INFO`. A repeat that is not a redraw is
     rendered with `MPV_RENDER_PARAM_SKIP_RENDERING` and is not presented. At
     most one present per refresh.
  4. Call `mpv_render_context_report_swap` once per render-loop turn, on the
     render thread: right after the present, or, when a repeat was skipped,
     right after the skipped render. It is the producer's pacing signal to
     mpv, not presentation feedback; the frame events are what reports
     presentation. The macOS prototype did the same.
- **`report_swap` from the frame callback cannot work.** mpv waits for each
  report, and in that setup it ran 60 to 180 renders a second on the 240 Hz
  display (7 runs). On macOS the same placement made mpv see a 30–39 Hz
  display (stage M2).
- **No extra thread waiting on the vblank is needed from `74e63fad`.** mpv's
  `estimated-display-fps` was 239.99 to 240.01, with 0 delayed frames in 53 of
  53 runs. Before that commit, 24 fps material needed such a thread.
- **Colour recipe.** `target-trc=linear target-prim=bt.709 hdr-compute-peak=no`,
  `target-peak = 203 × content_headroom` (846 here, 203 when the element is
  limited to SDR), set again on every surface or display change. Also
  `blend-subtitles=no`.
- **Colour result.**
  - SDR white reads 1.0000 and the SDR 50 % signal 0.1912. BT.1886 with a
    zero black is a 2.4 power: 0.5^2.4 is 0.1895, and the 8-bit code
    128/255 gives 0.1912; the clip's exact code value is not recorded here.
    The HDR10 1000-nit patch reads 4.1641, approximately the target peak
    846 / 203 = 4.167.
  - Three paused readbacks per patch were identical to the last digit.
  - On the composed desktop each value arrives multiplied by 240 / 80, within
    0.1 % (30 samples).

#### Cadence

The table gives the share of video frames first shown on their intended
refresh. They were read from outside by Desktop Duplication: 30 s per run,
n = 3 per cell, 48 runs, Skathi `74e63fad`, `exact_frame_feedback` off.

| Clip | Window | Old path | Old: schedule error, ms rms | New path | New: schedule error, ms rms |
|---|---|---|---|---|---|
| 24 fps SDR | 1280×720 | 74.7–84.3 % | 1.52–1.96 | 96.8–97.5 % | 0.57–0.64 |
| 23.976 fps SDR | 1280×720 | 72.4–76.9 % | 2.03–2.16 | 96.5–96.8 % | 0.64–0.67 |
| 60 fps SDR | 1280×720 | 52.3–72.0 % | 1.98–2.75 | 95.9–96.6 % | 0.68–0.75 |
| HDR10 24 fps | 1280×720 | 52.2–80.7 % | 1.62–2.62 | 97.1–97.5 % | 0.58–0.63 |
| 24 fps SDR | 3840×2160 | 75.7–85.8 % | 1.44–1.79 | 99.3–99.6 % | 0.23–0.27 |
| 23.976 fps SDR | 3840×2160 | 75.2–81.1 % | 1.76–2.03 | 99.6–99.7 % | 0.20–0.26 |
| 60 fps SDR | 3840×2160 | 50.9–66.1 % | 2.07–2.64 | 99.6–99.8 % | 0.16–0.24 |
| HDR10 24 fps | 3840×2160 | 77.4–83.6 % | 1.55–1.92 | 99.4–100.0 % | 0.09–0.28 |

- The new path retimes 23.976 fps to exactly 10 refreshes per frame. The old
  path played 10.0099 to 10.0101.
- The old path had 0 to 29 frames per run more than one refresh off; the new
  path had 0. No frame was skipped on either path, as far as the capture
  shows (see the capture caveat above).
- **The new path's late frames in the small window were associated with
  another program presenting every 40 ms** (a 25 Hz window). When that update
  fell due in the same refresh, 23.8 % (60 fps) and 21.9 % (24 fps) of frames
  were a refresh late; otherwise 0.62 % and 0.52 % (41 runs, 44 987 frames).
  That program also stops updating under a screen-sized window, so the
  screen-sized cells differ in both size and desktop activity. Some frames
  were late without a coincident update. The compositor mechanism is
  inferred from present times, not measured.
- The old path with `video-sync=display-resample` was not run in this
  comparison. An earlier single run of it, outside this batch, cost 47 % of
  one core with about 950 delayed frames a minute; it is historical and not
  a control for this table.

#### Latency

Time from the command to the first captured desktop frame with another
picture, paused. 10 commands of each kind per run, n = 3 runs (30 commands)
per command, cell and batch, four clip and window cells. The old path was
measured in two batches, and the new path on `74e63fad` and on `c7a524d5`;
the ranges cover both. That is 3 × 30 × 4 × 4 = 1440 commands, and none
produced a wrong frame.

| Command | Old path, cell medians | Old path, single commands | New path, cell medians | New path, single commands |
|---|---|---|---|---|
| Exact seek | 40.7–56.2 ms | 13.7–119.7 ms | 43.2–65.9 ms | 14.7–135.8 ms |
| Frame step | 96.2–99.2 ms | 51.0–103.5 ms | 45.7–48.9 ms | 11.7–76.6 ms |
| Back step | 82.8–93.9 ms | 77.2–99.2 ms | 87.7–95.1 ms | 82.3–101.5 ms |

- Seek and back-step medians overlap between the paths, within the spread.
- **The frame-step difference is the old path's sync mode.** In a control on
  Skathi 0.5.0, the old path with `video-sync=display-resample` steps in
  44–46 ms, and the new path in 46–49 ms with either sync mode (3 runs per
  cell, no wrong frames). The old path's 96–99 ms comes from
  `video-sync=audio`. It is not a gain of the Skathi path.
- With the option off, a presented frame was reported 1.43 refreshes after the
  `report_swap` call: 5.97 ms at the median, p95 10.2 ms (n = 3). That matches
  the `shown_at` column of the feedback table below (1.43–1.45), not its
  desktop column (0.70–0.72); Skathi #15 gives 1.43 refreshes, 5.96 ms on
  `74e63fad`. The build and timestamp pair behind 5.97 ms are not recorded
  here, so using it to derive an audio delay is a heuristic.

#### CPU, GPU, power

30 s per run with the clip looping and no desktop capture running, n = 3 per
cell, Skathi `c7a524d5`, option off. The batch is recorded as 48 runs; the six
cells shown are 36 of them. CPU is the whole process tree in
percent of one core, counted by cycles. Power is the graphics board's own
reading.

| Clip, window | CPU old | CPU new | GPU 3D old | GPU 3D new | Compositor GPU 3D old / new | Board W old | Board W new |
|---|---|---|---|---|---|---|---|
| 24 fps SDR, 1280×720 | 2.50–2.53 % | 4.51–4.58 % | 0.38–0.39 % | 1.04–1.05 % | 0.62–0.64 / 0.69–0.71 % | 48.8–49.0 | 49.4–49.8 |
| 60 fps SDR, 1280×720 | 5.09–5.28 % | 8.51–8.61 % | 0.95–0.96 % | 2.59–2.60 % | 1.59–1.64 / 1.76–1.79 % | 52.5–52.8 | 53.8–54.3 |
| HDR10 24 fps, 1280×720 | 3.56–3.71 % | 4.65–4.81 % | 0.64–0.65 % | 1.10 % | 0.62–0.64 / 0.70–0.71 % | 51.6–52.0 | 52.3–52.8 |
| 24 fps SDR, 3840×2160 | 2.67–2.73 % | 4.58–4.71 % | 0.99–1.00 % | 2.48–2.49 % | 0.58 / 1.34 % | 52.1–53.0 | 55.6–57.0 |
| 60 fps SDR, 3840×2160 | 5.52–5.55 % | 8.68–9.26 % | 2.50–2.51 % | 6.22–6.24 % | 1.45 / 3.35–3.36 % | 55.1–55.2 | 62.2–62.4 |
| HDR10 24 fps, 3840×2160 | 3.97–4.04 % | 4.69–4.73 % | 1.41–1.42 % | 2.51 % | 0.46 / 1.33–1.34 % | 50.7 | 53.8–54.1 |

- Video decode costs the same on both paths: 1.23 to 1.41 % at 24 fps, 3.06
  to 3.09 % at 60 fps.
- Like runs differ by up to 1.4 W, so a power difference under about 1.5 W is
  not a claim. That covers the small window at 24 fps.
- The old path here has no web view. The shipping player's CPU, GPU and power
  with its web view are not measured.

#### Memory

The new player has no web view: Skathi draws the whole interface. The
comparison is therefore the shipping player's whole process tree against the
prototype alone, with nothing added.

Values in MB, 30 s after the load, n = 3 per cell, prototype in its lean
configuration on `c7a524d5`. Each cell is private bytes / working set /
dedicated GPU memory.

| Clip | Size | Shipping player 1.49.0, 13 processes | Prototype, 1 process | Private: prototype − shipping | GPU: prototype − shipping |
|---|---|---|---|---|---|
| 1080p24 SDR | window | 629–631 / 837–849 / 287–290 | 551–553 / 279–282 / 384 | −80 to −76 | +94 to +97 |
| 1080p24 SDR | screen-sized | 704–734 / 861–904 / 479–480 | 912–917 / 274–276 / 996 | +178 to +213 | +516 to +517 |
| 1080p HDR10 | window | 650–735 / 846–937 / 397–400 | 553–558 / 279–280 / 481 | −182 to −92 | +81 to +84 |
| 1080p HDR10 | screen-sized | 719–751 / 866–896 / 588–593 | 915–921 / 273–274 / 1093 | +164 to +202 | +500 to +505 |
| 2160p HDR10 | window | 865–1135 / 945–1234 / 956–968 | 905–910 / 419–420 / 1128 | −230 to +45 | +160 to +172 |
| 2160p HDR10 | screen-sized | 1010–1285 / 975–1246 / 1225–1227 | 1231–1235 / 419–421 / 1709 | −54 to +225 | +482 to +484 |

- **The sizes do not match.**
  - The shipping player's window had a client area of 1704×959 physical
    pixels. The prototype's 1280×720 window at 150 % is a 1920×1080-pixel
    surface, 1.27 times as many pixels.
  - Screen-sized is a captioned 3818×2147 client area against a caption-less
    3840×2160 one.
  - No matched run was made.
- **"Screen-sized" is not the shipping player's fullscreen mode.** Its window
  was resized from outside, and its true fullscreen was not measured in this
  batch (it was later, in the Windows matrix under "The ported player
  against the shipping one").
- **The prototype's interface is a minimal test page.** The real interface's
  memory was not measured in this batch; see "The real interface on Skathi"
  below and the Windows matrix.
- **The shipping player's web view processes hold 388 to 447 MB private and
  649 to 664 MB working set, whatever plays.** Its first runs after a first
  start were up to 283 MB higher with the HDR clips. The cause was not looked
  for, and the ranges include those runs.
- **Going from the window to screen-sized, the prototype's memory rises
  with the window size, whatever plays.** The bounds derived from the table's
  ranges (not paired runs) are 359–366, 357–368 and 321–330 MB of private
  bytes and 612, 612 and 581 MB of GPU memory for the three clips. An earlier
  summary gave 330–360 MB private; the table's bounds are kept, since that
  figure is narrower than the table it summarises.
  - A test pattern with no mpv decoding shows a similar, smaller step:
    324–326 → 583–584 MB private (+257–260), 229 → 746 MB GPU (+517) (n = 3).
  - The old path changes by −50 to +100 MB private and +20 to +170 MB GPU;
    the runs behind that figure are not tabled here.
  - Most of the screen-sized difference against the shipping player is this
    step.
- **What makes the step** (Skathi 0.5.0):
  - **An empty Skathi window does not make it.** A minimal app, with or
    without an `<sk-surface>` nobody presents to, holds 177–178 MB private
    in both sizes (12 runs). Its GPU memory goes 49 → 120 MB: three 8-bit
    swap-chain buffers.
  - **With frames presented**, the test pattern and a ring of 3, the step is
    +260 MB private and +517 MB GPU (3 runs per size). The GPU step splits
    as follows. The total is measured. The split uses sizes from the source
    and counts from a build that logs texture creation, because no
    per-allocation tool was available. The rows sum to the total within
    1–2 MB in both sizes.

    | Allocation | GPU step |
    |---|---|
    | The swap chain going half-float (3 buffers) | +141 MB |
    | Our producer ring (3 textures) | +141 MB |
    | Skathi's two copy slots, each a half-float texture with an 8-bit twin | +141 MB |
    | Skathi's HDR scene target | +47 MB |
    | Skathi's atlas copy of the frame | +47 MB |

  - **The private-bytes step is a charge, not used memory.** A control
    program with no Skathi shows that this driver charges every unshared
    D3D11/D3D12 texture 1:1 to private bytes as untouched commit, and does
    not charge shared textures or swap-chain buffers (one run per kind).
    Skathi's 234 MB of ordinary textures plus a 24 MB zeroed CPU image give
    +258 MB, against +255–260 MB measured. The working set stays at 155 MB.
  - **With mpv playing**, the step is about 95 MB larger on both counters.
    That this is mpv's output-sized render targets is inferred.
  - **Possibly avoidable**, from the source and not tried, at 3840×2160:
    - the 8-bit twin of each half-float slot in an HDR window, 64 MB.
      On D3D12 on master (`886ebf94`, #33) the twin is created on a slot's
      first SDR read and kept until the slot retires. Re-measured on
      `8c912edc`: 62.0 MiB less GPU memory and 62 MB less private bytes
      in an HDR window with 4K frames (15–16 MiB with 1080p frames), ring
      of 3 or 2, element- or video-sized; no twin is created in HDR
      (allocation trace). After an SDR → HDR switch through the element's
      `dynamic-range-limit` one twin stays, +32.0 MiB, in 12 of 12;
    - the CPU placeholder image, 32 MB;
    - one of the two copies of the frame, 64–128 MB (the maintainer keeps
      the slot-to-atlas copy by design: redraws need the frame after the
      producer reuses its texture, #33);
    - a third swap-chain buffer, 64 MB;
    - on our side, a ring of 2 (below).
  - **Seen once each:**
    - a third copy slot sometimes appears, +96 MB;
    - a slot of the old size is kept after the window shrinks back.
  - The prototype's own `VRAM` log lines count the shared ring twice. The
    figures in this section come from an external probe.
- A ring of 2 instead of 3 saves one texture: 16 MB of GPU memory in the
  window and 64 MB screen-sized, nothing in private bytes (n = 3).

**How much our side can remove** (Skathi 0.5.0, 3 runs a cell, MB, read 30 s
after the load). Each cell gives private bytes / working set / GPU dedicated.
"Video-sized" means mpv renders at the video's display size, and Skathi scales
the frame to the element. The shipping player's column is the 1.49.0 run
above, not repeated.

| Clip | Size | Shipping player | Baseline (ring 3, output-sized) | Ring 2 | Video-sized | Both |
|---|---|---|---|---|---|---|
| 1080p SDR | window | 629–631 / 837–849 / 287–290 | 554–562 / 284–289 / 384 | 530–555 / 274–283 / 367 | 546–555 / 280–285 / 384 | 550–554 / 279–280 / 367 |
| | screen-sized | 704–734 / 861–904 / 479–480 | 910–912 / 273–277 / 996 | 911–920 / 274–281 / 932 | 594–602 / 281–283 / 571 | 593–604 / 280–284 / 555 |
| 1080p HDR10 | window | 650–735 / 846–937 / 397–400 | 555–561 / 280–281 / 481 | 557–563 / 280–284 / 464 | 558–561 / 280–281 / 481 | 557–561 / 281–284 / 464 |
| | screen-sized | 719–751 / 866–896 / 588–593 | 919–922 / 275–278 / 1093 | 918–922 / 273–275 / 1029 | 602–611 / 277–284 / 668 | 605–610 / 281–284 / 651 |
| 2160p HDR10 | window | 865–1135 / 945–1234 / 956–968 | 903–910 / 420–421 / 1128 | 906–909 / 419–420 / 1112 | 902–910 / 419–421 / 1128 | 907–912 / 420 / 1112 |
| | screen-sized | 1010–1285 / 975–1246 / 1225–1227 | 1214–1230 / 408–421 / 1709 | 1229–1232 / 420–421 / 1645 | 1228–1231 / 420–421 / 1709 | 1229–1233 / 420–421 / 1645 |

- **The video-sized render** saves 308–320 MB private and 425 MB GPU for a
  1080p clip at screen size. It changes nothing in the 1280×720 window, which
  is already 1920×1080 pixels, or for the 2160p clip.
- **Skathi's buffers follow the frame size.** Its copy slots, their 8-bit
  twins and its atlas copy all follow the frame, not the element. This is
  confirmed from the source and by a build that logs texture creation
  (9 runs).
- **Where the 425 MB goes** (inferred from sizes): ring 142, slots 142,
  atlas 47, mpv's own targets about 94.
- **Both changes, 1080p at screen size, against the shipping player:**
  - private bytes 100–146 MB lower;
  - working set about a third of the player's;
  - GPU memory 58–76 MB higher.

  Before the changes the prototype was 164–213 MB higher in private bytes
  and 500–517 MB higher in GPU memory.
- **What remains at screen size:** about 188 MB of GPU memory over the
  window's reading. That is Skathi's half-float swap chain and HDR scene
  target (inferred), which nothing on our side reaches.
- **CPU** stays at 4.3–4.6 % of a core in every cell. GPU 3D time at screen
  size falls from 2.48 % to 1.69 % with the video-sized render (24 runs).
- **`Busy`:** 0 in 178 runs, 42 of them with a ring of 2 at screen size in an
  HDR window.
- **Cadence:** 99.2–100 % in 71 of 72 runs, with no difference between the
  configurations. The one outlier ran while another program was redrawing the
  desktop.
- **The price is the picture.** In these runs Skathi scaled with one bilinear
  tap, the only scaler on that build; mpv's scaler is `lanczos`. On master,
  `image-rendering: auto` upscales with Catmull–Rom (below).
  - **Fine detail at 2×:** PSNR 27.8 dB and SSIM 0.972 against the
    output-sized render. mpv itself forced to bilinear gives 29.5 dB.
  - **Subtitles drawn by mpv:** rasterised at the video size and stretched,
    so they are visibly blurred (17.0 dB, SSIM 0.850 over the text). mpv's
    OSD is affected the same way.
  - **Upscaler shaders:** an optional upscaler shader would never engage
    (from the source).
  - **When nothing changes:** where frame and element are the same size, the
    two paths are bit-identical.
  - **Limits:** one paused frame per clip, generated content, 8-bit captures
    and one ratio.
- **Verdict:**
  - **A ring of 2 is free.**
  - **The video-sized render is acceptable only while nothing sharp is drawn
    into the frame.** A product has three ways forward:
    - render at the element size while subtitles or the OSD show;
    - draw the subtitles outside the frame;
    - a better scaler in Skathi, which helps detail but cannot restore
      subtitles already rasterised at the video's size.

    Skathi answered the last one in #34 (`684e7036`, master, for 0.6):
    `<sk-surface>` now honours `image-rendering`. With `auto` it uses
    Catmull–Rom on enlarged axes and clamps overshoot at edges, HDR
    highlights included; shrinking stays bilinear. Re-measured on
    `8c912edc`, same method and reference pictures, one run a variant:
    fine detail 31.4 dB / SSIM 0.991 with `auto` (27.8 / 0.972 bilinear,
    and 29.5 for mpv's own bilinear), the subtitle box 18.4 dB / 0.890
    (17.0 / 0.850): text rasterised at the video's size stays soft.
    `pixelated` gives 2×2 blocks; 1:1 stays bit-exact. In three HDR10
    frames up to 12.48 scRGB, no `auto` sample fell outside the range of
    its source neighbours, so no halo.

#### Audio through WASAPI

60 s per run, `ao=wasapi` at 48 000 Hz with mpv's `volume=0`, n = 3 per row,
1280×720 window, `c7a524d5`, option off.

| | mpv's A/V difference at 1 s | at 20 s | at 30 s | at 60 s | 10 s to 60 s, min to max |
|---|---|---|---|---|---|
| Old path | −7.7 to −6.3 ms | 0.00 | 0.00 | 0.00 | 0.00 to 0.01 ms |
| New path | +1.2 to +1.5 ms | +0.4 to +0.5 ms | −0.4 to −0.3 ms | −0.05 to −0.03 ms | −0.54 to +1.99 ms |

- The old path reads 0.00 by construction: `video-sync=audio` times the video
  to the audio clock.
- On the new path, `report_swap` was called once per refresh in 13 328 of
  13 328 intervals in each run (about 55.5 s at 240 Hz; the counting window
  inside the 60 s run is not recorded), and `audio-speed-correction` stayed
  between 0.99984 and 1.00003. By mpv's own counters, no drift appeared over
  the logged interval at 240 Hz.
- An inference, not measured: a fixed `audio-delay` of about +6 ms would line
  sound up with the picture with the option off, about +10 ms with it on,
  derived from the present → `shown_at` offset above. Untested. Sound against
  picture was not measured; that needs a microphone and a photodiode. A
  loopback capture would observe the audio endpoint's stream, not the sound
  from the speaker.

#### Frame events and exact feedback

With the default feedback, on `74e63fad`:
- 74.5 to 80.4 % of shown frames are reported `Estimated`.
- Every frame the compositor showed a refresh late (1124 of 1124 in 43 runs)
  was reported `Estimated`, and 98.6 % of them on the refresh it missed
  (Skathi #15).
- Cadence cannot be read from `shown_at` in this mode; it was read from
  Desktop Duplication.

The comparison below is `Builder::exact_frame_feedback(true)` on `c7a524d5`
(our patch, taken upstream as an opt-in), 1080p SDR, n = 3 per frame rate,
one 240 Hz display. The competitor here is a window of our own presenting
every 40 ms, standing in for the other program of the cadence table, which
no longer drew by then:

| | Late, 60 fps | Late, 24 fps | Late, 23.976 fps | `Exact` events | Present → desktop, refreshes | Present → `shown_at`, refreshes | Event after `shown_at`, p50 |
|---|---|---|---|---|---|---|---|
| Off (default) | 1.24–2.10 % | 0.77–3.40 % | 1.39–2.32 % | 19.9–27.6 % | 0.70–0.72 | 1.43–1.45 | 4.21 ms |
| On | 0 of 4856 | 0 of 1940 | 0 of 1940 | 100 % | 1.59–1.61 | 2.43–2.44 | 0.15–0.17 ms |

- **The option costs latency.** The measured present → desktop offset shifts
  by 0.89 of a refresh (3.7 ms at 240 Hz), and the reported `shown_at` by
  1.00 refresh. These are shifts of the run offsets, not a per-frame
  guarantee, and hold for this single-display setup: Skathi's maintainers saw
  no gain with two unsynchronised 165 Hz displays (Skathi #15). The option is
  process-wide. Its CPU, GPU and power cost is not measured.
- On a quiet desktop, with the option off, 0.31 to 1.05 % of frames were late
  in the small window. With it on, 2 frames of 15 533 were late, both reported
  `Exact` (n = 3 per cell).
- The changes that are on by default in `c7a524d5` gave no measurable gain
  over `37419f11`: 1.68 % late (114 of 6796) against 1.40 % (95 of 6798), 60
  and 24 fps pooled. Like runs differ by up to 2.6 points.

#### gpu-next through mpv PR #18845 on D3D11

The PR's CI build (mpv `v0.42.0-dev-gb635e5f89`, libplacebo v7.387.0) was
loaded into the prototype on `c7a524d5`, n = 3 per cell.

- **It initialises, decodes and renders in the prototype.** The render
  context is created in 63 to 65 ms (the `gpu` renderer: 62 to 67 ms),
  decoding stays `d3d11va`, and eight 8 s runs (a separate check from the
  n = 3 cells below) had 189 to 191 accepted presents, 0 `Busy` and 0 render
  errors. OSD and subtitles were not checked on this build.
- **Cadence is the same:** 99.3 to 99.9 % of frames on the intended refresh
  with `gpu-next`, 99.2 to 99.9 % with `gpu` (four clip and window cells, 30 s
  runs).
- **Cost is the same or slightly higher on GPU.**
  - HDR10 24 fps at 3840×2160: GPU 3D 2.86 to 2.88 % against 2.52 to 2.56 %.
  - 1080p24 SDR at 1280×720: 1.02 to 1.04 % against 1.05 to 1.06 %.
  - CPU is within 0.2 points, and power within the run spread.
- **Memory is lower.** In the 1280×720 window:
  - Private bytes: 533 to 536 MB against 553 to 555 MB (1080p24 SDR), and 891
    to 901 MB against 943 to 945 MB (2160p HDR10).
  - Dedicated GPU memory: 375 against 396 MB, and 1165 against 1224 MB.
- **Colour differs.** With the prototype's options, `gpu-next` does not give
  the `gpu` renderer's values. The table has linear values read back from the
  texture, 1.0 = SDR white, three readbacks per cell, identical within a
  variant:

| Patch | `gpu` | `gpu-next`, same options | `gpu-next`, `target-peak=203` |
|---|---|---|---|
| SDR 50 % signal | 0.1912 | 0.2412 | 0.2181 |
| SDR black | 0.0000 | 0.0042 | 0.0010 |
| HDR10 1000 nits | 4.1641 | 4.1641 | 1.0000 |
| HDR10 500 nits | 2.4697 | 2.3301 | 0.7153 |
| HDR10 203 nits | 1.0039 | 1.0713 | 0.4351 |
| HDR10 100 nits | 0.4912 | 0.5723 | 0.2781 |
| HDR10 10 nits | 0.0495 | 0.0724 | 0.0450 |

- The PR build's own `gpu` renderer matches the bundled build in all nine
  patches, so the library build is not the cause.
- **Matching `gpu`** (later, on our own build and the CI artifact):
  `target-contrast=inf` and `tone-mapping=clip`, with
  `hdr-reference-white=203`, bring `gpu-next` within 0.001 of `gpu` on the
  nine grey patches. **But `gpu` is the wrong target**: the shipping player
  already draws with `vo=gpu-next` in its own window.
- **Matching the shipping player's window** (Stage 24, 33 patches: grey,
  BT.709 and BT.2020 colours at 75 and 100 %, SDR and HDR10; Desktop
  Duplication in half float; the shipping DLL, the CI artifact and ours
  agree within 0.012):
  - **What that window does:** SDR and HDR alike go into an HDR10 swap
    chain; SDR white sits at the desktop's SDR white (240 nits here),
    SDR grey is a pure 2.4 power with no black lift, HDR is absolute with
    peak detection on.
  - Patches matching it: default options 6 of 33; the `gpu`-matching set
    above 11 (HDR 18 % too bright, because it puts 203 nits on the
    desktop's SDR white); `hdr-reference-white` = the desktop's SDR white,
    `target-peak` = the display's peak, `target-contrast=inf`,
    `hdr-compute-peak=yes` and `target-prim=bt.2020` give **27 in our
    texture and 26 on screen**.
  - Two steps of that set belong to the host, not mpv: a gain of
    203 / SDR white and a BT.2020 → BT.709 matrix that keeps negative
    values. The probe stood them in with a GLSL output hook.
  - **What differed:** up to `8c912edc` Skathi clamped a surface to
    0 … headroom (from source), so three HDR patches outside BT.709, right
    in our texture, lost their negative components on screen. Filed as
    #36 and fixed in `9c7f52b6`.
  - **Re-measured on master `64d2e42f`** (which contains `9c7f52b6`):
    the negatives reach the desktop. The HDR10 BT.2020 red patch, scRGB:

    | | R | G | B |
    |---|---|---|---|
    | our texture | 4.154 | −0.143 | −0.055 |
    | desktop, `8c912edc` | 4.145 | 0.000 | 0.000 |
    | desktop, `64d2e42f` | 4.145 | −0.143 | −0.055 |
    | mpv's own gpu-next window | 4.169 | −0.142 | −0.036 |

    The desktop now equals the texture on every channel of all 33 patches
    (negatives within 0.0015, positives within 0.010). On screen **27 of
    33** match mpv's own window (was 26), the texture still 27. In: the
    HDR10 BT.2020 R100, G100 and G75 patches. Out: two SDR BT.2020 blue
    patches that matched only because the clamp hid a negative our
    texture has and the reference does not (−0.218 against −0.048). Three
    rounds identical; `8c912edc` as a control reproduces 26.
  - **What still differs** is ours: the six SDR BT.2020 patches, because
    the shipping window narrows them to the display's gamut, which mpv
    does not know through the render API (inferred). Not checked after
    the fix: an 8-bit window or a switch from HDR to SDR, a tint or
    opacity over the surface, an upscaled frame, D3D11 or wgpu, macOS, a
    real film.
- **Our own libmpv with the PR.** The official LGPL libmpv for Windows is
  built in a Linux container, which this machine could not run (no Docker
  or WSL), and MSYS2's FFmpeg is a GPL build. mpv's native `win32` CI recipe
  (clang and meson, dependencies as static subprojects) with
  `-Dgpl=false -Dffmpeg:gpl=disabled` built `libmpv-2.dll` from the PR head
  `a98335f5`. In the prototype it initialises gpu-next in 68–78 ms, decodes
  with `d3d11va` and presented 188–191 frames in 8 s with 0 errors (4 runs).
  It lacks Vulkan, subrandr, libarchive, JPEG XL and AV1 (aom) against the
  CI artifact and links the Visual C++ runtime dynamically.
  - **Reproducible** (Stage 24): 42 subprojects pinned in a lock file that
    the build script enforces, `SOURCE_DATE_EPOCH` for SPIRV-Cross's
    timestamp and mpv's build date off; two builds from fresh trees gave
    the same DLL byte for byte (438 and 475 s).
  - **What the player needs of the missing parts:** none of the five. AV1
    decoding is dav1d and FFmpeg's own decoder, both present. One part not
    in that list is needed: **libxml2**, without which FFmpeg has no DASH
    demuxer; the shipping DLL opens an `.mpd` and ours did not. A variant
    with it (+1.1 MB) opens it. libarchive did not configure; subrandr
    (styled YouTube captions) needs nightly Rust.
  - **libxml2 2.15.4 is now in the default build** (decided since):
    pinned in the lock file like the rest, a 50 274 304-byte `libmpv-2.dll` that
    plays an `.mpd`, two builds identical byte for byte. The build trees
    were cut down to what the build needs.
- A reading, not established: libplacebo models an SDR display's black level
  and maps HDR10 with its own curve.

#### On Skathi 0.5.0

The prototype was rebuilt on `39ba435f` (0.5.0, `skathi-gpui` 0.5.2), with
the payload rebuilt by the same version:

- **Memory:** unchanged from `c7a524d5`. For lean 1080p SDR: 552–554 MB
  private in the window and 909–910 MB screen-sized; 384 and 996 MB GPU.
- **Cadence:** 99.2–99.9 % of frames on their intended refresh with
  `exact_frame_feedback` off, and 99.9–100 % with it on (12 runs each: 24
  and 60 fps, both window sizes).
- **One present per refresh loses one frame in fifteen. The driver's
  224 cap causes it, not Skathi** (#26):
  - **The loss:** 6.7–7.2 % `Superseded`, and a new desktop picture on
    93.3–93.5 % of refreshes, which is 224 a second. The loss repeats every
    62.5 ms, the beat of 240 against 224. Measured on 0.5.0 and on master
    `c29ab351`, 6 runs each.
  - **Not our app:** the window's paints run back to back, 4.42 ms median
    against a 4.167 ms refresh, and the time is inside the window's own
    `Present`. Our page and harness do nothing periodic (one traced run).
  - **Below the cap:** at about 190 presents a second nothing is lost on
    either build (3 runs each).
  - **Logging:** master logs each such frame as
    `Superseded: newer frame N ready`.
  - **Not tested:** Skathi's latch fix for the case it reproduced
    (`20733eea`), because that needs the cap off.
- **Closing** (#20, #24):

  | Close path | Error lines per run | Accepted presents with no event |
  |---|---|---|
  | `Handle::close()` while presenting (10 runs) | 0 | 1–3 in 9 of 10 runs |
  | Producer stopped, then `Handle::close()` (5 runs) | 0 | none |
  | Posted `WM_CLOSE` while presenting (10 runs) | 1 | 1–2 in 8 of 10 runs |

  The remaining line is `Invalid window handle. (0x80070578)` from
  `gpui_windows::window`.

  On master `c29ab351` both close paths, 10 runs each, give no error line
  and no accepted present without an event. A present after quit returns
  `SurfaceError::Closed` whenever the producer still had one to make (#24).
- **Regression check on master `c29ab351`:**
  - **Memory** is unchanged.
  - **Cadence** at 24 and 60 fps was 96.7–99.6 % with 0 `Busy` (12 runs).
    The batch ran while another program redrew the desktop, and 0.5.0
    interleaved under the same disturbance gave the same range. On a quiet
    desktop master read 99.4 and 99.6 %.
  - **Not obtained:** a full repeat on a quiet desktop.

#### The JIT against AOT in our own executable (master `64d2e42f`)

The prototype as an embedder's own executable, the payload published four
ways with the `sk-aot` 0.6.1 release binary, all without devtools; a ring
of 2 and the video-sized render; private bytes in MiB at 30 s (the report
writes MiB as "MB"; so does this subsection), 3 runs a
cell (the range), 192 clean runs. "Window / screen" are the two window
sizes of the memory table above.

| Build | Executable, MB | Idle window | 1080p SDR, window / screen | 2160p HDR10, window / screen |
|---|---|---|---|---|
| JIT (`sk publish`) | 95.7 | 515–519 | 533–535 / 582–585 | 890–891 / 1165–1168 |
| AOT image, default memory profile | 96.7 + 2.6 image | **886–890** | 909–911 / 957–960 | 1263–1268 / 1542 |
| AOT image, `SK_AOT_MEMORY_PROFILE=compact` | 96.7 + 2.6 | 560–563 | 578–581 / 620–626 | 930–934 / 1206–1213 |
| AOT, no JIT and no WebAssembly, compact | 80.8 + 2.6 | 558–560 | 576–581 / 625–627 | 933–936 / 1208–1213 |
| The same, features trimmed, size profile | 60.6 + 2.6 + 11.6 ICU pack | as the row above | | |

- **The default AOT profile commits 367–378 MB that are never touched**:
  a 256 MB block committed whole, 86 MB inside a 32 GiB reservation and a
  58.5 MB block. The working set is 6–10 MB *lower* than the JIT's.
  `compact` leaves 35–48 MB above the JIT. Without JIT and WebAssembly the
  default profile gives the same numbers as the AOT image's.
- **Removing the JIT changes no memory number**: the JIT build held about
  1 MB of JIT code. It saves 15.9 MB of executable.
- GPU memory is identical in every variant. Start-up medians are 556–568 ms
  everywhere (JIT 565, AOT 557, compact 558).
- The JS engine's share on the JIT build: 55–59 MB of private bytes (about
  11 %), 7 MB of it allocated.
- **For the player**: AOT only with `compact`, and then it costs 35–48 MB;
  no JIT buys size, not memory. Filed as #39 (compact as the default for
  application images, and four snags on the AOT page).
- **Caveat**: the page is the prototype's minimal 249 KiB page. With the real
  interface the picture changes (next subsection).

#### The real interface on Skathi (master `2b08fd37`, 0.7.0)

The player's Svelte interface, as ported for macOS (below), built for Windows
unchanged (`sk check`: 0 errors, 66 warnings, the macOS count) and run as our
own executable over the prototype's D3D11 producer: a ring of 2 at about the
video's pixel count, `display-resample` with the per-refresh pacing. Of the
interface's 131 commands 33 are answered natively, 8 window commands by
`appWindow` and 90 by stubs (torrents, casting, the catalog, updates,
diagnostics, the HDR status). The start screen, playback, seeking, frame
stepping, seekbar previews, subtitles, the context menu, the settings sheet,
resize, fullscreen and the mini player were driven by script, one pass each.
All 36 measured runs exited clean with 0 `Busy`; one of them still logged an
`Invalid window handle` line at close. Memory in this subsection is MiB
(the report writes it "MB").

- **Broken once, not isolated**: fullscreen → mini player → back left the
  window screen-sized, neither fullscreen nor maximised, and its close
  button did nothing (two `RefCell already borrowed` errors in the log), in
  one pass; not repeated.
- **Two traps.** In the first runs the display had had no input for 23
  minutes, and the window, in the foreground, reported its page hidden; one
  pixel of mouse movement brought it back. While hidden, with
  `display-resample` and the producer pacing only on mpv's updates, mpv ran
  ahead: a 30 s clip reached its end in under 2 s. The producer now starts
  every turn at a fixed point of the refresh, hidden or not.
- **Memory (JIT)**, against the test page with the same ring and recipe,
  earlier the same day (two Skathi revisions and two payloads apart,
  devtools compiled in, warm thumbnail and shader caches; "screen" here is
  the interface's own fullscreen, there a caption-less window of the
  screen's size): with the 1080p clip +52 to +57 MiB of private bytes in
  the window and +51 to +65 screen-sized, with the 2160p clip +30 to +59;
  working set +41 to +50 (+14 to +35 at 2160p); GPU memory +1 to +3. On the
  start screen 388–392 MiB private and 186 MiB GPU, because libmpv, the
  ring and a half-float window existed before any file. The ring was
  deferred since (see the Windows matrix below); the window still switches
  to half float at start.
- **CPU** while playing is about twice the prototype's, 9.2–9.8 % of a core
  (the conversion from cycles is inferred). The interface observes about
  fifty mpv properties and resyncs its mirrors every second; which part
  costs what was not measured.
- **AOT.** The image is 7.1 MiB for a 1437 KiB bundle. It did not start
  until the player's console capture (`console[level] = …`) was put in a
  `try` (#40). With `compact`: +3 to +11 MiB of private bytes over the JIT
  in the window cells (inside the JIT's own spread screen-sized) and 23–29
  MiB less working set while playing. The default profile, run in the
  window cells only: +340 to +345 MiB. With the real bundle the image
  nearly pays for its fixed block, which the test page's did not.
- **Subtitles and mpv's OSD in a layer of their own.** The D3D11 render API
  has no overlay target, so a patch to our own libmpv adds one:
  `MPV_RENDER_PARAM_D3D11_OVERLAY_FBO`, 290 lines in seven files, gpu-next
  only. mpv lays subtitles and the OSD out for the element's size and draws
  them into a second texture; one D3D11 pass of ours divides by alpha, and
  the result is presented into a second `<sk-surface>` right before the
  picture of the same render. 8-bit, a ring of 1, the bottom third of the
  element unless text is drawn above it, freed 2 s after the last text, and
  not used while the picture is the element's size. Measured with a 1080p
  clip at screen size:
  - **sharpness**, one generated SDR frame paused with one white SRT line,
    one 2× scale, three captures of that frame (identical bit for bit): the
    text against mpv's own window 35.3 dB (49.1 dB with
    `target-contrast=inf` and `tone-mapping=clip`), against 14.1 dB with the
    text in the scaled picture. Image subtitles, mpv's OSD text, a second
    subtitle line and ASS with its own fonts were not checked;
  - **sync**, a frame-coded clip with an ASS magenta box on six frames of
    every twelve, three runs: text and picture changed on the same desktop
    frame in all 240 changes. With the textures forced to be freed 100 ms
    after each box, the box came one refresh (4.2 ms) late in 13–15 of 40
    appearances; in use that is the first cue after 2 s without subtitles;
  - **memory**: +60 MiB of GPU memory and +75 to +91 MiB of private bytes
    while a subtitle shows; rendering the whole picture at the screen's
    size instead costs +415 MiB of GPU memory and +350 MiB of private
    bytes;
  - after the subtitles clear, +22 MiB of GPU memory and +43 to +59 MiB of
    private bytes stay: our textures are freed, and what stays is, from its
    size, Skathi's copies of the last text (inferred), which neither
    `clear()` nor a 16×16 transparent frame released. Not reported upstream.
  - Stage to stage, without subtitles, the runs with the patched libmpv
    (the pinned 0.42, gpu-next) read 9 MiB less GPU memory than the earlier
    stage's with the shipping 0.41 and the `gpu` renderer at 1080p, and
    about 100 MiB more with the 2160p HDR10 clip; the window policy
    (always-on-top) and the payload changed between the stages too. The
    later DLL swap under the old app (Windows matrix) measured the libmpv's
    own share directly.

#### Not measured

- The shipping player: its power and latency with its web view (its true
  fullscreen, CPU, GPU time and cadence were measured since, in the Windows
  matrix below).
- The real interface on Skathi: its power and latency, an SDR desktop, and
  everything behind the 90 stubs.
- `exact_frame_feedback`: its CPU, GPU and power cost; the option at other
  refresh rates; the option with a ring of 2.
- Sound against picture with instruments, and runs with a non-zero
  `audio-delay`.
- Other displays and modes:
  - refresh rates on the current Skathi (120 Hz and 60 Hz were run once per
    cell, on `1eacd896` only);
  - more than one display, and variable refresh;
  - a display path the compositor does not compose.
- Other hardware and settings: AMD and Intel GPUs, Windows 10, HDR off, an SDR
  display.
- A switch between SDR and HDR output while playing (its trigger, latency and
  glitches); the colour values above hold for the mode described.
- A real driver reset. Device loss was provoked only with `RemoveDevice` on a
  healthy adapter.
- `gpu-next`: its cost at 60 fps, tone-mapping options matched to `gpu`, a
  second driver or GPU.
- The memory split per allocation with a tool (VMMap, PIX, the debug layer).
  The split above comes from sizes and creation counts.
- What the compositor does with a late frame (a present trace needs
  elevation).
- Long sessions.

### macOS: the prototype, measured

Apple M3 Max laptop, built-in XDR display at 120 Hz (EDR headroom 6.11 of 16.0 at
the brightness used), macOS 26.5.2, other applications running. "On screen" is an
HDR ScreenCaptureKit capture, never light on the panel. In this section a value
with no run count is from one run. The capture's `displayTime` is not Skathi's
`shown_at`: an `Exact` frame's `shown_at` is about 9.5 ms after its capture
time (stages MX–M52). Stages M1–MX asked the capture for 1/120 s and got about
110 rows/s; stage MB asked for 1/240 s and got one row per refresh
(119.8–120.3 rows/s), and only MB's numbers are read per refresh.

#### Setup and build

- Skathi 0.4.0 (stages M0–M2), `1eacd896` (MM), `c602b5a4` (MX), `c7a524d5` (MY),
  `a7e307b9` = 0.5.0 (MZ), `39ba435f` with `skathi-gpui` 0.5.2 (M52); the player's
  libmpv v0.41.0 and MoltenVK. **`runtime-shaders` everywhere**: the Metal
  toolchain was missing; prebuilt shaders were never measured.
- **The JS payload must be built by the same Skathi version** (0.5.0 refuses a
  0.4.0 bundle, exit 2). The `customElements` shim is gone on master (#5).
- Separately, and ours to solve: linking Skathi's packages from a clone gave
  **two copies of svelte**, and `mount()` threw `effect_orphan`, an empty page.

#### Route A as built

mpv's GL render API (`4.1 Metal`) → `CGLTexImageIOSurface2D` → IOSurface `'RGhA'`
(RGBA16F) → GL fence → `setSignaledValue:` on the slot's `MTLSharedEvent` →
`present_shared_fenced`. Three slots, today's libmpv, no patch (stage M2).

- `MPV_RENDER_PARAM_FLIP_Y` = **0** (1 is upside down). `target-peak` is an
  **integer** (`"203.0"` → −9), set from a control thread. `videotoolbox` direct.
- Colour: `target-trc=linear target-prim=bt.709 hdr-compute-peak=no
  target-peak=round(203 × headroom)`. IOSurface R / G / B (M2, one run): PQ 1000
  nits → 4.82 / 4.97 / 4.85 (expected 4.926), PQ 203 → 0.98 / 1.01 / 0.98.
- Red and blue below green: 0.4–3 % across the M2 IOSurface patches, and on
  screen 0.1–1.05 % for MB's SDR patches (median of ~600 captured frames, 3
  runs). gpu-next through the same route is neutral to 0.05 %, so the deficit
  belongs to the old renderer in this GL/RGBA16F path.
- SDR 50 % on screen (MB, linear R / G / B, 3 runs identical to four
  decimals): 0.1893 / 0.1912 / 0.1892 on the prototype against
  0.2162 / 0.2161 / 0.2159 on today's video path, a 2.4 against a 2.2 transfer.
  Those levels are measured; which transfer is intended is a decision.
- Each `target-peak` change costs one 20–70 ms render, 7 per ramp (3 runs).

#### EDR

- MM (`1eacd896`): the window enters EDR unaided (3 of 3; 0.4.0 needed a
  primer window). No glitch frame was captured in 21 switches: 10 with every
  refresh near the switch captured, the other 11 with 16 refreshes uncaptured
  near them. macOS ramps 1.0 → 6.11 in 1.31 s (3 of 3), 2.0 → 6.11 in
  1.07–1.08 s, holds 15.0 s after the last EDR layer, falls in 2.0 s (3 of 3).
- MM, from the page's acknowledgement of the command: `standard` clamps on
  screen after 29.5–45.3 ms (n = 12); `no-limit` with headroom left shows HDR
  after 40.3–49.8 ms (n = 5).
- MX (`c602b5a4`): the element's clamp now trails the system's headroom by
  32 ms p50 (24–44, 6 runs), smoothly. The producer's `content_headroom()`
  still moves in coalesced steps at least 192 ms apart (8 values per ramp), so
  a `target-peak` that follows it still steps.
- **MX, entering from 1.0, the first ~110 ms of the ramp are not shown**: after
  the ramp start (the first `NSScreen` value above 1.0), 11–15 captured frames
  stay at 1.000, then a 17–18 % jump. The first brighter frame came
  157–172 ms after the command (6 runs). From 2.0 there was no jump (3 runs).
  **The hold is macOS's.** A native `CAMetalLayer` app without Skathi shows
  the same (M53, 8 runs): the 8.0 band first rises above 1.0 at 161–168 ms,
  with 12–17 captured frames at 1.000 before it. Frames keep reaching the
  screen during the hold: a marker band changes at +39–40 ms. Requesting EDR
  alone gives the same 161.5 ms. Skathi's 17–18 % first jump is its own
  clamp: the HDR capture records the layer's values, even above the current
  headroom, not the panel's light.
- **"From 1.0" means the app's own reading.** `NSScreen`'s headroom differs
  between processes: a windowless process read 8.79 for minutes while the app
  read 1.0. Every "from 1.0" in this section is the app's value, not the
  panel's state.

#### Cadence against the shipping video path

Stage MB, Skathi `c602b5a4`. Baseline = today's video path without the web view
in a 960×541 pt window (the player's libmpv and options: gpu-next, `macvk`,
`video-sync=audio`). Prototype = old `gpu` renderer, `display-resample`
(23.976 fps is retimed to the display, so its ideal hold changes), render 5 %
into the refresh. Both `ao=null`, muted. 1080p, ~35 s runs with 30 s captured,
3 runs. The metric is the share of source frames held exactly the ideal number
of refreshes: a hold is the distance between two first appearances in the
capture (about 725 holds per run at 24 fps, 1800 at 60), and a transition next
to a missing capture row (0–6 refreshes per 30 s) is excluded.

| Clip (ideal) | Baseline | Prototype | Source frames never captured (prototype) |
|---|---|---|---|
| 24 fps SDR (5) | 98.4, 99.7, 99.7 % | 99.3, 98.1, 97.4 % | 0, 0, 2 |
| 23.976 fps SDR (5, a 6 per ~200) | 74.5, 77.6, 76.8 % | 96.7, 99.2, 99.0 % | 0, 0, 0 |
| 60 fps SDR (2) | 72.9, 47.8, 99.6 % | 98.9, 99.9, 98.8 % | 10, 0, 0 |
| 24 fps HDR10 (5) | 97.8, 92.6, 100.0 % | 98.3, 98.6, 96.1 % | 1, 1, 0 |

Baseline misses are phase-dependent 4/6 pairs and it never skipped a source
frame; the prototype's are late presents. With the speakers as clock
(`ao=coreaudio`), 24 fps: 99.5, 78.9, 86.0 % against 94.3, 96.6, 95.8 %.

#### Latency

Stage MB. Paused, 10 commands per run, 3 runs per path; from just before the
command to the `displayTime` of the first captured frame with the new picture,
p50 per run in run order (range of 30):

| Command | Baseline | Prototype |
|---|---|---|
| `frame-step` | 101.4, 101.5, 99.0 ms (86–106) | 94.9, 82.8, 90.1 ms (50–117) |
| `seek absolute+exact` | 97.8, 94.9, 95.2 ms (33–201) | 158.3, 137.5, 141.3 ms (64–266) |
| `frame-back-step` | 160.6, 198.1, 184.4 ms (144–224) | 243.4, 217.1, 216.1 ms (184–273) |

By run order, the prototype's medians differ by −6.5, −18.7, −8.9 ms for steps,
+60.5, +42.6, +46.1 ms for seeks and +82.8, +19.0, +31.7 ms for back steps;
the runs are in order, not paired replicates. Stage MM reported 44.9–48.0 ms
median from present to `shown_at`; it may explain part of the seek and back-step
difference.

**Most of the present-to-screen time is the display link, not Skathi** (M53,
27 runs × 10 commands):
- A native `CAMetalLayer` app paced the way gpui paces windows on macOS 14+
  (`CAMetalDisplayLink`) takes 37.0 ms p50 (31.9–47.3) from command to
  captured frame. Skathi's comparable figure is 36.8–43.1 ms.
- The link targets each frame 41.6 ms (5 refreshes) after its callback, in 12
  of 12 runs. `preferredFrameLatency = 1` did not change that.
- Without the link (`nextDrawable` and `presentDrawable`) it is 13.6 ms. With
  `displaySyncEnabled` off it is 6.9 ms; tearing was not checked.
- The drawable count (2 or 3), `presentsWithTransaction` and
  `present(atTime:)` changed nothing beyond the spread.
- Inferred: Skathi's own share is 0–6 ms. The rest is gpui's choice of
  `CAMetalDisplayLink`, served by macOS.
- Sparse presents on the link get no `presentedTime` in the native app either
  (131 of 132 were 0). That likely explains the frames without a time that
  Skathi reported at low frame rates.
- Upstream answer (#32, `227dfd1d`, master, for 0.6): an opt-in
  `lowLatencySurface` per window (`<SkWindow lowLatencySurface>` or
  `"window": { "lowLatencySurface": true }` in `sk.config.json`, read when
  the window opens). The window then paces with its own timer and presents
  with display sync on; a seek after a pause can take a drawable at once.
  The link stays the default; UI animations in such a window can look less
  even on a variable-refresh display.
- **Measured on master `a4469450`** (M54; 3 runs each, off and on
  alternating, 90 paused commands per setting):
  - present → captured frame p50 **33.4 ms off, 16.3 ms on**, against the
    native control's 32.4 ms with the link and 13.3 ms without. Command →
    captured frame for frame-step, seek and back-step: 90.7, 147.2 and
    240.5 ms off; 73.6, 120.8 and 225.1 ms on. `shown_at` comes 12 ms after
    the captured frame in both modes, with under 1 ms of spread;
  - **playback cadence breaks with the option on**: frames held exactly the
    intended refreshes 57–59 % at 24 fps and 57–63 % at 60 fps, against
    86–97 % and 99 % off (the 86–87 % off runs most likely a capture
    artefact). The misses are symmetric (4 and 6 refreshes at 24 fps), and
    Skathi's own `shown_at` shows them, so its feedback is right. Inferred
    from source: the window timer counts each interval from its last fire,
    not from vblank, and drifts across the refresh;
  - an animated bar stood still on 4.5–9.3 % of refreshes with the option
    on, 0–0.1 % off. EDR engages the same way. On 11 of 30 forward steps
    the new frame was `Superseded` by mpv's redraw, which carries the same
    picture.
  - Reported in #32 with two suggestions: a timer on the vblank grid, or the
    link during continuous playback and the timer only for sparse presents.
- **The hybrid, measured on master `b682a9e3`** (M56; upstream took the
  second suggestion in `f531dd11` and `7a9f653a`: the first ready frame
  after a pause takes a drawable at once, continuous playback and CSS
  animation use the link, 125 ms of idleness re-arms the immediate path).
  Same method, 3 runs each, 320×180 pt window never focused:
  - paused present → captured frame p50: **9.3 / 9.8 / 9.2 ms on**
    (step / seek / back-step) against 32.8 / 33.5 / 33.8 off, about 4 ms
    below the native control without the link. The p90 on seeks is still
    about 35 ms, so some new frames after a seek take the link (not traced);
  - 60 fps held exactly 2 refreshes 99.3–99.9 % on, 98.9–99.6 % off;
    24 fps held exactly 5 refreshes 86.7–90.4 % on, 89.8–95.4 % off. The
    ranges overlap and both sets lost 170–250 capture rows, so no
    difference is claimed from three runs;
  - the CSS bar stood still on 0.04–0.16 % of refreshes on, 0–0.13 % off;
    `Superseded` on forward steps 0 of 30 on, 2 of 30 off;
  - the window's foreground state did not change frame-event kinds or
    latencies here (unlike Windows, see #20).
  - Confirmed in #32, which we closed.

#### CPU and GPU

Stage MB, the runs above: ~31 s, 3 runs, with the capture running. CPU is % of
one core, not normalised for core type (one setup read 15 % and 29 %); GPU time
is the process's own. Neither has a web view or WindowServer's share, and
neither is split between mpv, Skathi and the harness. The machine was not idle
(load average 4.7–5.6 from other applications). GPU percentages are not
comparable with the Windows ones.

| Clip | CPU: baseline / prototype | GPU: baseline / prototype |
|---|---|---|
| 24 fps SDR | 7.9, 10.0, 9.1 / 15.1, 26.6, 29.2 % | 4.1, 4.4, 3.8 / 9.1, 9.8, 10.6 % |
| 24 fps SDR, no capture (30 s) | 12.7, 12.0, 13.1 / 29.0, 29.4, 34.3 % | 4.3, 3.6, 4.4 / 10.7, 11.8, 16.6 % |
| 60 fps SDR | 17.0, 19.1, 15.0 / 31.4, 35.1, 31.0 % | 9.9, 9.8, 6.2 / 22.1, 23.0, 20.6 % |
| 24 fps HDR10 | 10.4, 8.9, 10.1 / 25.6, 26.2, 23.4 % | 5.5, 4.4, 4.9 / 8.9, 9.2, 8.6 % |

The no-capture runs were a separate batch that read higher on both paths; they
are not a clean measurement of the capture's cost.

#### Memory against the shipping player

Stage MB, `phys_footprint` (on Apple Silicon graphics memory is system RAM, so
this is not the Windows private-bytes metric) at 22 s, no capture, window
960×541 pt (1920×1082 px) on both, 3 runs, ±1 MB. The prototype's interface is
minimal: **the real interface's cost was not measured in this stage** (it
was later: see "The real interface on Skathi, against the shipping player"
and the macOS matrix).

| | 1080p SDR | 1080p HDR10 | 2160p HDR10 |
|---|---|---|---|
| Prototype, harness quiet | 706–707 MB | 754–755 MB | 973 MB (1 run) |
| Shipping video path, no web view | 482–483 MB | 532–533 MB | 727–731 MB |

Matched, same session: at 1080p the prototype is 221–225 MB above the bare video
path. Unmatched: the new player has no web view, so the prototype can also be set
against the shipping player's whole tree, WebKit processes included, but the only
figure for that is a single earlier observation on macOS 26.5 (~845 MB, WebKit's
GPU process ~214 MB playing), not a run of this stage; against it the prototype
is ~90–140 MB lower. Neither comparison supports a production saving. The
difference to the bare path is made of:

- the producer ring, 16 MB per slot; a ring of 2 had 0 `Busy` at 24 fps (3 runs);
- five layer drawables against three: 40 MB at 8 bit, 80 MB in EDR;
- 83 MB against 7 of IOAccelerator objects, mostly the old renderer's video-sized
  intermediates (inferred from sizes); gpu-next is 30–40 MB lower;
- 7–8 extra 8 MB chunks of an unidentified graphics pool (56–64 MB).

**The window-size step on macOS is the window's drawables** (M53, minimal
Skathi app, 18 runs):
- `phys_footprint` is about 50, 62–70 and 95 MB at 480×270, 960×540 and
  1600×960 pt.
- IOSurface memory goes 4.2 → 16 → 48 MB: two 8-bit drawables of the
  window's size. They account for 44 of the 45 MB difference.
- An `<sk-surface>` with no frames costs nothing measurable.
- The peak is 331–397 MB at start-up in every run. Runtime shader compilation
  is the guess, not isolated.
- One run reached 374 MB at the large size (264 MB of unmapped graphics
  memory). It was not reproduced.

Memory with frames playing, and in an EDR window, was not measured this way.

#### Audio through CoreAudio

Stage MB. `ao=coreaudio` to the built-in speakers at `volume=0`, 60 s captured
per run, 3 runs; nothing was heard or recorded. "Audio clock minus picture" is
mpv's `audio-pts` (CoreAudio's reported latency included, a line fitted through
it) at the capture `displayTime` of each frame's first appearance, minus that
frame's pts: a difference of two clocks, not of sound and light, and the
speakers' and panel's own delays are not in it.

- Baseline +9.1…+11.1 ms; prototype **+51.8, +52.0, +53.0 ms**, about +40 ms
  relative to the baseline, consistent with Skathi's present → screen time,
  which mpv cannot see.
- With `audio-delay=0.050` (a separate batch of 30 s runs, 3 runs) the
  difference falls to +4.9…+5.8 ms. That is a tested correction for this
  configuration; a product default needs optical and acoustic measurements.
- The prototype in `video-sync=audio` read −2.4…−3.9 ms, with baseline-like
  cadence (72–84 % at 24 fps).
- At **24 fps** the audio clock ran at 0.99877–0.99902× (audio resampled,
  `audio-speed-correction` 0.998750). mpv's final estimate from our swap
  reports was 119.3–119.6 Hz despite `display-fps-override=120`; the
  display-sync interval stayed at the nominal 1/120 s.
- **Later measured (M55), and the first explanation below did not hold.**
  With per-iteration logging, mpv answered each report in 12–18 µs, so its
  turnaround was not the cause. Refreshes were lost after occasional stalls:
  8–61 ms inside mpv's render call and 10–23 ms in the GPU fence wait, on a
  Mac that was in use (that the load caused them is inferred; an idle Mac
  was not measured). The proposed fix did not meet the target: lost
  refreshes 0.21–0.44 % before and 0.09–0.92 % after, 5.0053–5.0069 against
  5.0015–5.0213 refreshes per 24 fps frame, and the correction still at its
  −0.125 % limit most of the time. The 0.3–0.6 % against 0.11–0.17 %
  mismatch is explained: each lost refresh lengthens one frame, and mpv wins
  back 50–76 % of them by shortening later ones. Counted directly,
  iterations equal refreshes minus those lost. **`audio-delay` from a
  running median** of `shown_at` minus the swap report worked as well as a
  fixed 50 ms: it chose 46–48 ms, residual −0.5 to +5.8 ms (fixed: −3.5 to
  +1.7 ms), and with `lowLatencySurface` on it followed the shorter path to
  30 ms by itself (residual +0.6 and +4.2 ms).
- **Why, from mpv's source and the same runs' logs** (the earlier offscreen
  analysis, kept for its reasoning):
  - The panel ran at 120.000 Hz (a fit of `shown_at` against refresh
    counts: 119.9993–120.0007 Hz). The video ran at 0.9989, 0.9983 and
    0.9988× (5.0053, 5.0084 and 5.0061 refreshes per 24 fps frame). In 55 s
    the three runs had 28, 12 and 28 gaps of 6 or more refreshes between
    presents, and 17, 2 and 12 gaps of 4; the 4-refresh gaps are probably mpv
    dropping a vsync to catch up (inferred).
  - `vo_libmpv` has no vsync timestamps. In display-resample, mpv counts
    each `report_swap` as one nominal vsync, and a frame lasts a counted
    number of reports. If the producer misses a refresh, the frame can stay
    one physical refresh longer without advancing mpv's count, and mpv
    cannot see it.
  - When audio leads by more than half a vsync, mpv 0.41 lowers its audio
    multiplier by the full 0.125 % (and raises it when audio lags), holds
    that until the error crosses zero, then returns to a clamped regression
    estimate. The reading of 0.998750 is that correction, not a measured
    rate. The PR head replaces it with a 1 s low-pass and a proportional
    controller.
  - The gaps do not follow long draws (4.3–4.8 ms median before them). The
    prototype starts an iteration only at 5 % into a refresh and only if
    mpv's update has arrived. A draw iteration reaches its report about
    4.7 ms after it starts, which leaves about 3.6 ms for mpv's next update.
    A late turnaround in mpv then loses the whole refresh. **This is a
    hypothesis.** Missed reports were not counted directly (no per-iteration
    times), the run counted more iterations than refreshes (about 7968 against
    ~7.8k), and mpv's estimate implies 0.3–0.6 % of recent refreshes without
    a report, more than the 0.11–0.17 % video deficit. Neither mismatch is
    reconciled.
  - **Proposed fix, on our side** (in resample-local mode): report as soon as
    `mpv_render_context_render` returns, before the fence and the present,
    and start a late iteration up to about 3 ms past the phase point, if
    render and fence still make Skathi's latch, instead of skipping the
    refresh. A model of the display-sync code (arithmetic only, an invented
    timing tail calibrated to the measured rate) gives 0.60 % lost
    refreshes for the prototype's policy and 0.006 % with both changes; only
    the comparison means anything. In a separate sweep of the model, raising
    `video-sync-max-audio-change` left the video rate unchanged. Not tried on
    the screen.
  - **`audio-delay`**: present → `shown_at` was 44.6, 46.3, 44.7 and 45.9 ms
    p50 in those runs, beside the excess of about +40 ms above (+51.8…+53.0
    against +9.1…+11.1). Inferred, not tested: in display-resample only, set
    `audio-delay` from a running median of `shown_at` minus the actual swap
    report time (which moves if the report moves earlier), not a fixed 50 ms.
    `shown_at` is the display link's target, not light from the panel;
    acoustic and optical checks are still needed. `video-sync=audio` did not
    show the excess.

#### Frame events (#19)

On `1eacd896` (stage MM, one captured run): at 2 presents/s 19 of the 38
captured frames were reported `Superseded` (stage MX's tool counts 39 presents
in the same capture); a scripted resize gave 0 `Presented` in 1.2 s (inferred
cause: a frame waited for the window's next present, which an idle window does
not make). 0.4.0 showed the same without capture (23 of 60 `Superseded`).

The resize is `setContentSize:` from inside the app, not a mouse drag. Load
varied between runs: the steady case's rate of two-refresh gaps differed about
fivefold, so builds are compared within interleaved batches.

| Build | 2 presents/s: event callback against its own `shown_at` | Resize: captured yet `Superseded` | Steady 120 fps: `Estimated` events | Steady 120 fps: captured yet `Superseded` |
|---|---|---|---|---|
| c602b5a4 (MX) | fixed; `Estimated` callbacks 36–40 ms before their `shown_at` (3 runs) | 3 of 358 | 2–9 per run, skipped drawables (7 runs) | 0 of 132 |
| c7a524d5 (MY) | 0.1–3.6 ms after (3 runs) | 5 of 361 | 6 in 6 runs | 0 of 28 |
| a7e307b9 (MZ) | unchanged (1 run) | **16–24 per run** (4 runs) | 0 in 6 runs | 0 of 74 |
| 0.5.2 (M52) | unchanged (1 run) | 0–2 per run (4 runs; a7e307b9 interleaved 18–25) | 1, 2, 0 per run, all outside the capture (3 runs) | 0 of 164 |

On 0.5.2 (`f2a79987`: only an exact time takes a refresh from a drawable
without one) the reproduced regression is gone in the scripted resize, with
residues: 2–3 pairs of `Estimated` events sharing one refresh per resize run,
and the 0–2 captured-yet-`Superseded` frames above. A real-mouse resize and the
mpv steps were not re-run on 0.5.x.

Steps (MX, `c602b5a4`; 3 recipe runs and 1 `video-sync=audio` run, 10 each):
the stepped frame's own id was `Presented` in 38 of 40; command → its
`shown_at` was 74–98 ms in every recipe step (48.3–102.9 ms in the `audio`
run); the callback can come earlier than that when the event is `Estimated`.
mpv presents twice per paused step (the frame, then a redraw), so take the first
`Presented` id at or after the first present after the command.

#### gpu-next through PR #18845 on GL 4.1

Stage MB, prototype on `c602b5a4`, capture running unless said otherwise. The
public PR has moved on since and reports problems in other configurations.

- PR head `aade20e8c`, libplacebo 7.398.0: initialises, decodes and renders on
  our GL context in the prototype, `videotoolbox` direct; 0.41 ignores the new
  parameter (it then renders with `gpu`), so it can always be passed.
- **A libplacebo shader error once per affected file, in 27 of 28 gpu-next
  runs** (the exception is the `deband=yes` run): `textureSize(sampler2DRect,
  int)` → `Failed dispatching scaler.. disabling`; the lost scaler is not
  measured. No compute on GL 4.1, so no peak detection (inferred).
- Cadence, recipe, 24 fps SDR, against 0.41 `gpu` in one interleaved control
  batch (3 each): 99.4, 100.0, 96.2 % / 99.8, 99.4, 99.4 %. An earlier batch on
  the PR build read 80–96 % for both renderers, with 30–160 transitions excluded
  for capture gaps, while the machine was busy; it is kept for render times only.
- Render CPU per drawn frame about 20–25 % lower: against 0.41 `gpu` in the
  interleaved control, and against the PR's own `gpu` in that busy batch.
  GPU time 8.3–8.4 % / 10.6–10.7 %.
- Colour, IOSurface values (the screen agrees to 0.3 %), EDR, peak 1240, 3 runs:
  **neutral** to 0.05 %. With `target-trc=linear hdr-reference-white=203` the
  output is off the nominal 203-nit scale, and not by a constant factor:
  PQ 1000 → 5.0664 (nominal 4.926, +3 %), PQ 203 → 1.2109 (+21 %), PQ 10 →
  0.0800 (+62 % against 10/203 = 0.0493). `scrgb` is absolute (1.0 = 80 nits)
  and within readback precision of the nominal values on the tested patches
  after × 80/203, which nothing applies today; unscaled, it clips at the
  headroom on screen.

#### The Vulkan render API (built, and running in the Skathi prototype)

**Verdict: proceed** with a downstream gpu-next-only Vulkan backend on MoltenVK:
app-owned device, RGBA16F IOSurface pool, one timeline semaphore/event per slot.

**Built** on the Stage MB checkout of PR #18845, `aade20e8c` (the PR head has
since moved), with libplacebo 7.398.0 and MoltenVK 1.4.2: 7 files, +578 −1, of which `include/mpv/render_vk.h` is 216
lines with its documentation. The renderer itself needed no change: the
existing `ra_pl` path already turns a `pl_tex` into a target.
- **API**: the app passes its instance, devices, extensions, features and one
  queue family (`MPV_RENDER_PARAM_VULKAN_INIT_PARAMS`). Each render call
  (`MPV_RENDER_PARAM_VULKAN_FBO`) gets the image, its layouts, an optional
  acquire timeline value and a required release value. Success guarantees
  the release signal was submitted. `INVALID_PARAMETER` and `UNSUPPORTED`
  touch nothing; after a failure past the acquire (`GENERIC`) the backend
  tries to signal, but the app must not rely on it and retires the image
  (`MPV_RENDER_PARAM_VULKAN_RETIRE`, also required before an image is
  destroyed). The old renderer refuses the API.
- **Measured offscreen** (a client that reads every frame back through
  Metal, a ring of 3, no window):
  - 240 of 240 frames read back and distinct through a resize and a retire;
    at the end every slot's Vulkan, Metal and expected timeline values were
    80. No frame was compared with an independent reference picture;
  - VideoToolbox decodes straight into Vulkan. It matched software decoding
    on every printed SDR frame (H.264) and on all 21 distinct HDR frames with
    `hdr-compute-peak=no`; with peak detection on, HDR frames differed after
    the first (temporal state, inferred);
  - a PQ clip's 10 000-nit box reads 6.1055 with `target-trc=linear
    target-peak=1240` (1240/203 = 6.108), so values above 1.0 reach the
    IOSurface;
  - an ASS subtitle box reads (1, 1, 1) at the centre; mpv's OSD uses the
    same overlay path but was not drawn;
  - mean CPU time in the render call 1.12 and 1.53 ms in the two 240-frame
    runs; pipeline creation on first use 47–116 ms each in the logs; GPU
    time not measured.
- **Validation**: 0 errors from the backend. libplacebo needs
  synchronization2 enabled on the device, or its fallback barriers fail
  validation (9–10 errors). With VideoToolbox, about 30 "no memory bound"
  errors on libplacebo's own imported decoder planes, which MoltenVK backs
  from the Metal texture.
- **Regression harness, offscreen** (later the same day): 18 named cases,
  each in its own process, plain, under Vulkan validation and under Vulkan
  plus Metal API validation. 17 of 18 pass with no Vulkan errors beyond the
  known VideoToolbox ones and no Metal validation messages: seek, pause and
  frame-step, redraw while paused, `SKIP_RENDERING`, repeats, refused,
  delayed, dropped and stuck Metal copies, a failure after the acquire with
  and without a release signal, retiring with GPU work in flight, and
  `VK_QUEUE_FAMILY_EXTERNAL`.
  - The 18th, the `VK_EXT_external_memory_metal` import, fails on MoltenVK
    1.4.2: the IOSurface stays zero (a MoltenVK bug, fixed on its main
    branch, not released). Built from main it matches today's
    `VkImportMetalIOSurfaceInfoEXT` import pixel for pixel at the same cost,
    so today's import stays.
  - **A backend bug found and fixed**: retire returned before mpv's last
    release value was visible (2 of 3 runs before; with the extra wait it
    fires in 10 of 10, 1.3–7.8 ms after `pl_gpu_finish`; cause not isolated).
  - **A contract gap closed**: with a Vulkan target `SKIP_RENDERING` waits
    on nothing and signals nothing, so a skip is not a release; the header
    says so.
  - The patch is now 9 files, +1014 −29.
  - **Re-run on 2026-11-21** against the port's libmpv (the same patch
    plus the picture layout, which the harness leaves at the old layout):
    17 of 18 under both validation configurations (`import-emm` only),
    18 of 18 with MoltenVK main, still no Vulkan errors and no Metal
    validation messages. Plain, `delay` failed as well, and repeats show
    two cases are intermittent on the new and the old libmpv alike: over
    30 plain runs each, `delay` failed 5 and 8 times (about 20 %) and
    `refuse` once each (about 3 %), so this is not a regression of the new
    build; the earlier day's 4 of 4 `delay` passes probably hid it. Every
    `delay` failure is a copy that reads frame N−3's picture, the same slot
    in the ring of 3, after mpv's ready signal: a stale read, not an
    overwrite before the copy. Cause not isolated (MoltenVK's IOSurface
    writes seen from another Metal queue, or the harness's bookkeeping
    around the gate, inferred); Skathi's copy of `<sk-surface>` follows the
    same pattern. The screen was locked through these runs.
  - **The lock is not the cause.** The same binaries, 30 interleaved
    offscreen runs each with the screen unlocked: `delay` failed 3 times on
    the new libmpv and 5 on the old (5 and 8 locked); `refuse` 3 and 5 (1
    and 1 locked). Pooled, `delay` 8 of 60 unlocked against 13 of 60 locked
    (Fisher p = 0.34), `refuse` 8 of 60 against 2 of 60 (p = 0.09). The
    failures are of the same kind (`delay`: 58–59 distinct frames of 60).
    The two variants that would isolate the stale read (no gate; the copy
    on mpv's queue) were not run.
- **Subtitle overlay pass, offscreen** (paused frames):
  `MPV_RENDER_PARAM_VULKAN_OVERLAY_FBO` renders the video at its size and
  subtitles and the OSD into an element-sized target. Premultiplied alpha
  (a 50 % box reads rgb = a = 0.4978); the text line matches a single
  element-sized render at 84 dB, against 16.7 dB for today's video-sized
  render stretched; zoom, pan, crop and rotation agree across both passes
  at 62–92 dB in 7 configurations; both passes take 0.51 ms of CPU against
  1.61 ms for one element-sized pass, and 0.06 ms with nothing to draw.
  Not checked: EDR overlay colour, OSD messages, PGS, an RGBA8 target,
  playback.
- **In the Skathi prototype, on screen** (the GL producer replaced by the
  Vulkan ring and `present_shared_fenced(ready, ready + 1)`, no CPU wait;
  each run paired with a GL run on the same libmpv and gpu-next; 3 runs
  each, 320×180 pt window):
  - frames held the intended refreshes: 24 fps 95.5–98.2 % (GL 97.0–99.2),
    60 fps 98.8–99.2 % (GL 99.6–99.9), HDR10 24 fps in EDR 96.2–96.7 %
    (GL 96.6–98.8). Within 1–3 points, a tendency rather than an
    established regression;
  - present → captured frame 33–36 ms, as GL; command → screen overlaps GL
    in every row;
  - the CPU fence wait per frame is gone (0 against 0.7–2.4 ms); the
    render call takes about what GL's call and wait took together;
  - colour identical to GL in the IOSurface and on screen, SDR and EDR
    (PQ patches 4.95 / 2.67 / 1.18; captured maximum 5.12);
  - all frame events `Presented Exact`, no `Busy`, no errors.
  - **It costs more than GL**: about 1.4–1.6× the process's GPU time
    (24 fps 7.5–8.6 % against 5.2–5.7 %, 60 fps 18.5–19.6 against
    12.1–13.0) and 73–93 MiB more `phys_footprint` (579–634 against
    505–541 MiB), at about the same CPU (6 points more at 60 fps). Not
    broken down; MoltenVK's translation and libplacebo's Vulkan caches are
    the guesses.
  - A leak was fixed on the way: a ring slot's Vulkan objects leaked when
    Skathi refused the IOSurface before the window's first draw.
- **In the ported interface**: with a ring of 2, a picture at about the
  video's size and the overlay pass in a second surface; see "The port on
  the Vulkan path" below.
- **Not tested**: device loss, the queue-lock callbacks, GPU time per pass,
  KosmicKrisp.

The research before it:

- **Offscreen proof** (public MoltenVK 1.4.2 release, Vulkan 1.2 requested by
  the application, macOS 26.5.2): offscreen direct-IOSurface image import and
  timeline-event interop passed. 8 combinations passed once each (Vulkan clear
  or libplacebo draw × event made on either side × explicit bind or not); 64×32
  pixels of (2.0, 0.5, −0.25, 1.0) matched bit for bit through Metal. The final
  Vulkan wait on the release value was a synchronisation check with no second
  write. **Not shown**: validation layers, slot reuse, a write after the Metal
  release, libmpv video, VideoToolbox, a display. The `VK_EXT_external_memory_metal`
  image path was read in source and not run in this proof.
- **KosmicKrisp (Mesa 26.3.1) is not usable** for this design (source read): no
  `VK_EXT_metal_objects`, no public shared event. RFC #18258 closed unmerged.
- **Size, inferred** (historical; the built patch is +578 −1 in 7 files):
  350–600 lines in 6–9 mpv files, 250–450 app-side (still unverified), on top
  of PR #18845. **Risks**: cross-queue teardown (MoltenVK #2697 and #2859 are
  open reports of related risks, not diagnoses of this prototype), colour at
  the compositor, footprint (a 2160p slot is ~63 MiB). VideoToolbox
  NV12/P010 into the IOSurface has since been checked offscreen (above).
- Queue-family ownership stays provisional: the offscreen ring ran under
  validation with `VK_QUEUE_FAMILY_IGNORED` and normal layouts only, and the
  `EXTERNAL` path was not run.
- **Which Skathi branch shows the frame** (M53, source with 3 lldb runs and 3
  pixel runs per mode, 0.5.2). The window's output mode decides it on every
  tick:
  - **In an 8-bit window**, every shown frame goes through the 8-bit mapping
    (`Ready::for_atlas`, which saturates), and bands of 2, 4 and 8 read 1.0.
  - **In an EDR window**, the mapping never runs. The frame's half floats go
    into a half-float atlas tile, and the capture reads 1.998, 3.998 and
    7.993.

  An HDR producer therefore needs the window in EDR. One 8-bit run under
  lldb never presented a frame; it was left out and not investigated.

#### Not measured

Light on the panel; other displays, rates, brightness; a real-mouse resize; a
second surface; device loss; acoustic output; 4K and 60 fps HDR cadence; steps on
0.5.x; power; macOS before 26.

#### The real interface on Skathi, against the shipping player

The player's own Svelte interface, ported onto Skathi with the Tauri calls
stubbed, measured on master `b805f961` against Frame Player 1.48.1 (Tauri and
WebKit, the installed version) and the prototype's minimal page; 3 runs each,
480×320 pt window at the top right, clips without audio. Frame Player ran
from an isolated home folder; its own files were checked unchanged
afterwards.

**What the port is and is not.** The interface is the player's code almost
file for file (`sk check`: 0 errors, 66 warnings). Its video still goes
through the prototype's GL route A: a ring of 3, a render at the window's
size and Skathi's copy, so none of the savings measured on Windows (a ring
of 2, the video-sized render) and none of the Vulkan work is in these
numbers. Stubbed: seekbar previews (the FFmpeg module), the audio-output
device, torrents, casting, watching together, the catalog's network side,
updates and diagnostics; there is no macOS menu.

**Footprint** (`phys_footprint` summed over the process tree, median of 3;
for Frame Player that includes WebKit's content, GPU and network processes;
the VideoToolbox decoder service, 48–51 MiB at 1080p and 314–338 MiB at
2160p, is counted for all):

| | Minimal page | Port on Skathi | Frame Player 1.48.1 |
|---|---|---|---|
| Idle, start screen | 74 MiB | **92 MiB** | **112 MiB** |
| 1080p, controls hidden | 641 MiB | **696 MiB** | **581 MiB** |
| 1080p, controls visible | — | **695 MiB** | **789 MiB** |
| 2160p HDR, controls hidden | 1180 MiB | **1224 MiB** | **1333 MiB** (1049–1386) |
| 2160p HDR, controls visible | — | **1223 MiB** | **1264 MiB** (1260–1535) |

- **The controls cost nothing on Skathi** (same footprint and CPU). In
  Frame Player showing them adds about 210 MiB to WebKit's GPU process and
  about 6 points of CPU.
- **With the controls hidden at 1080p the port is 115 MiB above Frame
  Player**: that is the frame path above, the part the ring of 2, the
  video-sized render and the overlay pass are meant to remove.
- CPU while playing is level (30–39 % of a core in every column); idle, the
  port uses 6.5 % against 4.8 %. GPU time is not comparable across the two
  architectures.

**Start-up** (ms from spawn): the port shows its start screen at 610–740
against Frame Player's 475–519; the first 1080p video frame is level
(928–961 against 893–1007); at 2160p HDR the port is about 70 ms ahead. The
minimal page is about 450 ms faster than both: the cost is the interface's
boot, not mpv.

**Caveats:** a small window (video memory follows the window's size), an
older Frame Player than the current release, three runs, and one idle port
window left running through the batch (a separate process, not in the sums
as far as the tools show; not re-checked).

#### The port on the Vulkan path

The same port on master `b4d4e190` (`sk check` unchanged: 0 errors, 66
warnings), same method, 3 runs a cell, against the same binary on route A
and Frame Player 1.48.1. What changed in the port:

- **The frame path** is the Vulkan render API with a ring of 2 and gpu-next.
  Its libmpv is PR #18845's head with our Vulkan patch and a 134-line diff
  on top that adds a picture layout: mpv renders the element's whole layout
  (letterbox, zoom, pan, crop, rotation) into a picture of about the video's
  pixel count, and Skathi stretches it over the element with Catmull–Rom.
  1080p in the screen-sized window: a 1920×1108 picture for a 3456×1994
  element. In the 480×320 pt window, and for 2160p at any size, the picture
  is the element's size.
- **Subtitles and mpv's OSD** go through the overlay pass at the element's
  size into a second `<sk-surface>` stacked above the video. Skathi blends
  straight alpha and libplacebo writes premultiplied, so a Metal compute
  pass divides it out between mpv's release and Skathi's copy, on the
  slot's own timeline, with no CPU wait.
- **Seekbar previews are real**: the player's thumbnail service, in process
  as in the shipping player, with cold caches in every launch.

Window sizes: 480×320 pt, and a window of the whole screen's size (the
port's content 3456×1994 px, Frame Player's 3456×2168 px requested), not
native fullscreen. Footprint summed over the process tree, MiB, medians:

| | Port, Vulkan | Port, route A | Frame Player 1.48.1 |
|---|---|---|---|
| 480×320, idle | 92 | 89 | 112 |
| 480×320, 1080p, controls hidden / visible | 751 / 752 | 745 / 744 | 624 / 835 |
| 480×320, 2160p HDR, hidden / visible | 1517 / 1510 | 1499 / 1494 | 1307 / 1513 |
| 480×320, 1080p with ASS subtitles | 783 | 745 | 635 |
| Screen-sized, idle | 116 | 114 | 213 |
| Screen-sized, 1080p, hidden / visible | **1089 / 1088** | 1314 / 1316 | **756 / 1016** |
| Screen-sized, 2160p HDR, hidden / visible | 2095 / 2116 | 2056 / 2054 | 1500 / 1769 |
| Screen-sized, 1080p with ASS subtitles | **1426** | 1322 | 769 |

Screen-sized variants of the Vulkan port: one pass at the scaled size and
no overlay, 1048 (1054 with subtitles, which are then soft); one
element-sized pass with a ring of 2, 1228 (1242 with subtitles, the
fallback).

- **The new path saves 225 MiB against route A** for 1080p at screen size,
  the Windows finding repeated on the Mac. It is still 333 MiB above Frame
  Player with the controls hidden and 72 MiB above with them visible.
- **Nothing to save in the small window or at 2160p**, where the picture is
  the element's size: there the Vulkan path is 6–18 and 18–40 MiB above
  route A. At 2160p the decoder service holds 385–409 MiB against 314–338;
  that libmpv links Homebrew's FFmpeg, and it was not looked into.
- **Subtitles are sharp, and the geometry is one transform.** Against one
  element-sized pass, the text line reads 45.6 dB (23.2 dB for one
  video-sized pass); the whole frame under zoom, pan, rotation and crop stays
  at 31–38 dB, above the 29.0 dB of a 1-px shift (window captures, one
  paused frame each).
- **But the overlay is the dearest way to keep them sharp**: 1426 MiB
  against 1242 for the element-sized fallback, and about 4.5 points more
  GPU time than the same path without subtitles. Where that memory goes is the next subsection.
- **GPU time is higher** on the new path: 14.3 % for 1080p at screen size
  against 12.8 % on route A and 10.4 % for Frame Player; with subtitles
  18.8, 11.9 and 11.2 %. CPU is level (30–38 % of a core at 1080p).
- **Start-up**: level in the small window (first frame 860–1060 ms in all
  three). Screen-sized the port is about 300 ms behind on both routes
  (1.16–1.29 s against 0.83–0.95 s); inferred, not checked: the port opens
  at its configured 480×320 and then restores the saved geometry.
- The un-premultiply kernel is compiled at start, which puts 13–16 MiB of
  `MTLCompilerService` into every playing Vulkan run; a precompiled
  `.metallib` would remove it (not done).
- **Caveats**: the libmpv builds differ (Vulkan: 0.42-dev with Homebrew's
  FFmpeg 8; route A: the player's 0.41), so the Vulkan/route-A difference is
  the path and the build; a Vulkan process holds two FFmpeg copies
  (inferred); the colours were not compared with Frame Player here, and they
  turned out wrong (next subsection).

#### Where the port's memory goes

`footprint -v` at 12 s, **the main process only** (`fpui` against
`frameplayer`, so the totals are below the tree sums above), screen-sized,
1080p SDR, controls hidden, Skathi `b4d4e190`, medians of 3, MiB. This is
the per-allocation split the Windows section could only infer.

| Chunk | Port | Frame Player | Whose |
|---|---|---|---|
| Window drawables | 265: 5 × 53.0, RGBA16F (EDR) | 79: 3 × 26.3, 10-bit | ours (our half-float surface puts the window in EDR); the count is Skathi's |
| Our picture ring | 32: 2 × 16.2 | 0 | ours |
| Our overlay ring, nothing drawn | 0 resident | — | ours |
| Skathi's copies of the picture | 49 | 0 | Skathi |
| mpv's GPU blocks of 8 MiB | 384 (48 blocks) | 328 (41 blocks) | mpv |
| Other GPU objects | 49 | 34 | mostly mpv |
| A window-sized 28.6 MiB object | 29 | 29 | the system (inferred) |
| malloc (the storyboard about 45 of it) | 127 | 110 | both |
| JS heap and JIT | 29 | 0 (WebKit runs in its own processes) | Skathi's runtime |
| Rest | 30 | 25 | |
| **Total** | **993** | **605** | |

- **The +388 MiB** is drawables +186, mpv's blocks +56, Skathi's copies
  +49, our ring +32, JS +29, malloc +17 and other GPU objects +15.
- **These are main processes only, and Frame Player's 605 is not its
  cost.** It leaves out its WebKit processes (about 96 MiB in this cell)
  and the decoder service (51); the port's 993 leaves out its decoder
  service (60). Counted whole, this cell is 755 MiB for Frame Player (see
  "The ported player against the shipping one"). Every "against Frame
  Player's 605" below in this subsection is a main-process comparison.
- **The 8-MiB blocks are mpv's GPU memory, and both players pay for them.**
  They are not VideoToolbox (`hwdec=no` keeps 48), the thumbnails, Skathi's
  interface (the minimal page holds 48) or the window's size; they go with
  `vid=no`, and Homebrew's mpv with `--vo=gpu-next` holds 41, as Frame
  Player does. Why the port holds 7 more is not explained.
- **Why the window is half-float** (from source): Skathi puts a window into
  HDR output while any visible `Rgba16Float` surface may exceed SDR white,
  which the default `dynamic-range-limit` allows. The port made both
  surfaces half-float for every file, and an empty overlay surface counts.
- **The SDR picture in that window was wrong.** On a grey clip whose
  pass-through value is 64.0 (window captures, 8-bit): Frame Player 63.8
  with a letterbox of 0.2; the port on the path above 77.4 with a letterbox
  of 18.1, so black and the greys are lifted; the port with an 8-bit picture
  and overlay 66.0 and 0.0. The cause, mpv's linear target with a 1249-nit
  peak or Skathi's EDR composition, was not isolated.
- **The overlay blends as the shipping player does.** The 50 % box reads
  158.4 in Frame Player and 159.7 in the 8-bit port, where an sRGB-space
  blend predicts 159.0. The "darker" box of the previous subsection was the
  reference's fault: mpv's single pass blends in a linear target. For HDR,
  Skathi blends overlays in extended sRGB-encoded space; whether that shows
  on subtitles over highlights is not checked.
- **A trap**: `content_headroom()` reports the display's headroom (6.15
  here) even when the window is not in HDR, so an 8-bit picture that
  follows it put SDR white at about 0.69. The 8-bit picture pins
  `target-peak=203`.

**A cheaper subtitle layer** (screen-sized, medians of 3, MiB of the main
process; the overlay's cost is the run with subtitles minus the same
mode's run without):

| Variant | Total | Overlay's cost | Text line |
|---|---|---|---|
| The path above: EDR, half-float overlay, ring 2 | 1293 | +300 | 45.6 dB |
| 8-bit picture and 8-bit overlay, overlay ring 1 | **943** | +123 | 54.5 dB |
| The same, overlay limited to a 300-px bottom band (runs 1–2 of 3) | **871** | +51 | 54.5 dB |
| Element-sized single pass (the fallback) | 1180 | +187 | (reference) |
| Frame Player 1.48.1 | 605 | 0 | — |

- The text line of the 8-bit variants is measured against an element-sized
  pass in the same 8-bit mode. mpv writing an 8-bit linear overlay gave
  41.0 dB: dark edges are quantised before sRGB encoding.
- **After the subtitles end**, our overlay ring can be released, but Skathi
  kept the cleared surface's slots on `b4d4e190`: 2 × 53.2 MiB in EDR,
  2 × 27.0 in 8 bit; `clear()` withdrew the frame only (from source).
  Master releases them (#38, re-measured in "On screen on master
  `64d2e42f`" below).
- The band was a fixed bottom band; one that follows the subtitles would
  need mpv to return their bounding box and Skathi to present into part of
  an element atomically. Not asked: the full-size 8-bit overlay already
  costs only +123.
- **Proposed, measured in the port, not in the player yet**: for SDR video
  an 8-bit picture (`target-trc=srgb`, `target-peak=203`) and an 8-bit
  overlay with a ring of 1, dropped once nothing is drawn; for HDR video
  half float as now; switched on `video-params/gamma`. That gives 1080p SDR
  820 MiB instead of 993, with subtitles 943 instead of 1293, and Frame
  Player's picture within 1–2 levels.
- **What would remain**: about 215 MiB above Frame Player in one process
  (drawables 133 against 79, Skathi's copies 25, our ring 16, mpv's +56, JS
  29, malloc). Frame Player's WebKit processes (content 69–123, GPU 21–230)
  are outside that comparison; with every process counted, the port with
  the lever set below and AOT came out 97 MiB above it in this cell (852
  against 755, see the macOS matrix below).
- **Skathi's part** went to #38: the Metal slot pool never trimmed,
  `clear()` kept the slots, and the layer holds five drawables against
  three. The first two are fixed on master; the five drawables stay by
  design.

Not checked: HDR content with these switches, PGS and OSD messages through
the 8-bit overlay, memory over hours.

#### The bundle, the features and the JIT (Skathi 0.6.0)

The port on v0.6.0 (`ca6e4e08`; against `b4d4e190` it changes versions
only), measured with a hidden window on the start screen: no screen, no
playback. 3 runs a variant, interleaved.

- **`sk check`**: 0 errors, 66 warnings. No finding in the AOT ranges SK1
  (read-only built-ins) or SK2 (`eval`, `new Function`, computed `import()`,
  string timers); one SK3002 (svelte's types); 29 of the CSS warnings are
  SK4007, "needs a 0.6 host", which this host is. A search of the bundle
  found no `eval`, `new Function`, `require`, `WebAssembly` or `import(`.
  Inferred: nothing found rules out the no-JIT AOT workload; the
  definitive list comes from the AOT compiler on Windows, not run.
- **The bundle**: 1 451 512 bytes. The three i18n dictionaries are 20.7 %
  of the script, all loaded and one used (not changed). A second copy of
  `@tauri-apps/api`, nested under the libmpv plugin's package (16 266
  bytes), was removed. **The bundle was not ASCII**: 192 characters in the
  `String.raw` templates of `src/lib/player/chapters.ts` (the skip-intro
  patterns) kept the whole source in UTF-16, about 1.4 MB more by the
  CLI's estimate. Escaped as `\uXXXX`, with the same `skipKind` for 33
  titles, the bundle is pure ASCII and, with the duplicate gone,
  1 436 019 bytes. That workaround is no longer needed: see the next
  subsection.
- **The executable**: dropping `types` and devtools and a size profile
  (`opt-level = "s"`, thin LTO, one codegen unit, opt 3 for the crates
  Skathi lists) take it from 103.0 to 82.7 MB as built, 73.4 to 65.6 MB
  stripped. `icu-root` has no effect on macOS (the system's ICU); on
  Windows it saves 9.1 MB but then needs an ICU data pack with ru, en and
  zh. `runtime-shaders` stays only because the build Mac lacks the Metal
  toolchain.

Idle footprint, median of 3, MiB:

| Step | Footprint | RSS |
|---|---|---|
| Before | 88.7 | 203.3 |
| Bundle cuts | 86.9 | 198.3 |
| Devtools off | 86.4 | 197.8 |
| Features trimmed | 86.3 | 197.3 |
| Size profile | **85.5** | 193.7 |
| The same with `JSC_useJIT=false JSC_useWebAssembly=false` | **76.9** | **153.9** |

- **The JIT is the one lever with a real effect** on macOS: −8.6 MiB of
  footprint and −40 MiB of RSS at idle; JavaScriptCore's regions fall from
  28.1 to 15.5 MiB and its four JIT worker threads go. The start screen
  still boots. What the interpreter costs while the interface is busy is not
  measured. The JS heap itself is 7.6–7.7 MiB, about 9 % of the idle
  footprint: the bundle is not where the memory is.
- **A start-up peak**: every launch reaches about 350 MiB of footprint in
  its first 1–1.5 s, then settles at about 87. An empty 0.6.0 payload with
  no mpv peaks at 229 MiB and settles at 32; at the peak `vmmap` shows
  192.8 MiB of unmapped graphics memory resident. It is the same on route
  A, so it is not Vulkan. Filed as #37; it is the system's, not Skathi's
  (see "On screen on master `64d2e42f`" below).
- **Windows** has more levers (an AOT image with `publishJit: false`,
  `SK_AOT_MEMORY_PROFILE=compact`), measured since on the prototype: see
  "The JIT against AOT in our own executable".

#### On Skathi master after 0.6.0 (`89c511c1`, 0.7.0-dev)

Built only, with the screen locked: nothing was run in a window.

- **`sk check`**: 0 errors, 66 warnings, every finding and location the
  same as 0.6.0's on the same tree.
- **The production bundle is ASCII without our workaround.** With
  `chapters.ts` back as it is in the player, `sk publish` gives no UTF-16
  warning and a bundle with no byte above 0x7F: the CLI hoists each
  non-ASCII tagged template into a frozen, `\uXXXX`-escaped strings table
  (`bcd1f8b5`). Evaluated from that table, all 7 skip-intro literals give
  the same text as the original source. The bundle was not run in the
  host. It is 1 440 002 bytes, 2.1 KB above the escaped variant.
- **Sizes** unchanged: the executables within 7 KB as built
  and 64 bytes stripped.

#### On screen on master `64d2e42f`

The port rebuilt on `64d2e42f` (`sk check` unchanged: 0 errors, 66
warnings, every finding the same as on `89c511c1`), run with the screen
unlocked (checked before each launch and at each reading), muted, never
focused, from an isolated home folder. Small windows 480×320 pt at the
top right; the memory runs in a window of the screen's size, as before.

- **Smoke run**: the 1080p ASS clip plays with VideoToolbox, the pause
  button and a seekbar click work (45.9 s for an expected 45.0), and
  subtitles are drawn.
- **The traffic lights' fades (#35): no button moved** in any sample of 3
  launches, centred at (12, 17) and at an explicit (20, 11), through
  150 ms fades and instant hides, read at +0, +60 and +300 ms; two launches
  of the player's own idle fade kept them at (12, 17). The hit test, the
  buttons' rectangle and the re-centring are right; `getWindowButtonsRect()`
  follows the buttons one frame late (by design: it reads a per-frame
  snapshot).
- **The start-up peak (#37) is the system's.** Idle, 3 launches a cell:

  | Payload | Peak | Graphics memory at the peak | Settled at 7 s |
  |---|---|---|---|
  | Empty, window visible | 231.0–231.7 MiB | 192.8 MiB: 24 objects of 8.00 MiB | 34–35 |
  | Empty, window hidden | 228.1–229.2 | 192.8 | 31–32 |
  | The port, window visible | 354.2–355.8 | 257.1: 32 objects | 90–91 |
  | The port, window hidden | 348.4–349.3 | 257.1 | 87–88 |

  Runtime against precompiled shaders changes it by at most 1.6 MiB, the
  window's visibility by at most 3. It stays above 60 % of the peak for
  1.0–1.2 s. A 26-line AppKit program without Skathi, a never-shown window
  with a `CAMetalLayer` and three presents, peaks at 220–223 MiB with the
  same 24 × 8.00 MiB and settles at 23; 213–214 MiB with 48×32 drawables
  and 315–317 with 3456×2304,
  12.5 MiB without a present, 10.2 MiB for Metal alone. So: a one-off
  ~190 MiB for about a second after a process's first present to a
  `CAMetalLayer`, independent of size. That it is per display or
  compositor is inferred (the maintainer's virtual SDR display gave 38
  MiB); other displays and refresh rates were not checked. Skathi's
  embedding documentation now notes it (`ec3cc308`).
- **Copies and drawables (#38)**, main process, `footprint -v` at 12 s,
  screen-sized, cold thumbnail cache, MiB, medians:

  | Case | `b4d4e190` | `64d2e42f` |
  |---|---|---|
  | 1080p, controls hidden | 993 | **992** |
  | ASS subtitles on screen | 1293 | 1290 |
  | Subtitles cleared | 1232, overlay 2 × 53.2 kept | **1123**, overlay 0 |
  | Cleared, and our overlay ring dropped | 1121 | **1019** |

  A cleared surface now holds no copy (−109 and −102 MiB). After the
  overlay shrinks to a band no old-size copy is left in 6 of 6 launches
  (1 of 3 kept one before, so suggestive, not proof). The 1080p picture
  held 2 slots and a tile in all 15 launches. **Five drawables** in all 55
  launches, `lowLatencySurface` off and on, as the maintainer keeps them.
  **`lowLatencySurface` makes no memory difference**; its frame timing was
  not measured here.
- **Steady playback is unchanged**: 992 against 993 MiB with the same
  chunks (drawables 265, our ring 32, copies 49, mpv's blocks 384, malloc
  127, JS 28); the idle hidden start screen settles at 87.7 MiB against
  86.9.
- **A method trap**: the thumbnail cache lives in the port's own data
  folder, not the payload's, so a warm cache puts about 45 MiB less into
  malloc. Totals above are from cold runs.

Not checked: a real pointer move, a physical trackpad and Reduce Motion
(#35); other displays, Macs, macOS versions and memory pressure (#37); a
burst of presents then idle (the trim itself), HDR content, repeated
clear/present cycles and hours (#38); 2160p, the small window, CPU, GPU and
start-up times on this build.

#### Every memory lever, and AOT without the JIT (`64d2e42f`, `d153b34e`)

Each lever alone and then together, on `64d2e42f` (master up to `9e8c81a1`
changed nothing on the surface path), main process at 12 s, screen-sized
window, cold storyboard, medians of 3, MiB:

| Case | Before | Final set | Δ |
|---|---|---|---|
| 1080p SDR, controls hidden | 994 | **805** | −189 |
| ASS subtitles on screen | 1294 | **939** | −355 |
| After the subtitles clear | 1124 | **824** [793–830] | −300 |
| 2160p PQ | 1643 | 1545 | −98 |

- **What does it**: for SDR content an 8-bit picture, overlay and window
  (`target-trc=srgb`, `target-peak=203`; HDR content unchanged), −173 at
  1080p and −310 with subtitles (drawables 265 → 133, our ring 32 → 16,
  Skathi's copies 49 → 25); an overlay ring of 1 (−76 with subtitles) and
  released after 24 empty frames (−106 once they clear); a picture ring of
  1 (−15); no JIT (−12); a release-size executable without devtools and
  with precompiled shaders (−4). CSS `dynamic-range-limit: standard` in
  place of the 8-bit switch saves 150, 23 less, because Skathi then keeps
  8-bit twins and our ring stays half float.
- **What does not**: none of the mpv and MoltenVK options tried reduced
  mpv's screen-sized pool of 8-MiB blocks (384 MiB). The one exception is
  in the small window, where mpv downscales: turning off its downscaling
  corrections removed two blocks and the linear-light buffer (−27), a
  quality change the shipping player does not make, and screen-sized it
  saved nothing. A
  subtitle band that follows the drawn rows saved 50–101 on bottom-only
  subtitles but dropped a line that appears at the top for about 80 ms, so
  it is not in the final set.
- **Correctness**, on the test clips only: on the grey test clip, paused
  in the small window, black 0.0 instead of 18.1 and the grey 66.0 against
  Frame Player's 63.8 (the 8-bit path); on one paused subtitle frame the
  text 54.5 dB against an element-sized pass; over a 5 s sample of 24 fps
  playback on 120 Hz (2–5 launches) every frame shown and 0 drops by mpv's
  counters, and no lever made cadence measurably worse. HDR with these switches: the PQ clip
  goes half float before its first ring; an SDR file followed by an HDR
  file in one process was not checked.
- **What the final 805 still holds over Frame Player's main process (605)**:
  drawables +54 (five against three), mpv's blocks +56, Skathi's copies 25,
  JS 16, malloc +15, other graphics +15, our ring 8.

**AOT without the JIT on macOS** (#37, master `d153b34e`).
Four things had to change before the player's image ran: the
`--payload-only` image is unsigned and the host refuses it (signed ad hoc by
hand; Skathi signs it since `025ff87c`, not verified by us), the build needs
`REDJS_AOT_BUILTIN_TABLE_DIR` in its environment, the console capture's
assignment throws in an image (#40), and master did not build without the
`audio` feature (#41). Measured, 3 launches a cell; the input timings are
synthetic devtools input (30 hover moves, a drag, a right click) to the
page's own DOM change, on a paused 1080p clip in the small window:

| | JIT | JIT off by environment | AOT, no JIT |
|---|---|---|---|
| Idle, settled (visible window), MiB | 89.7 | 78.8 | **71.1** |
| Idle, start-up peak (visible), MiB | 352.5 | 340.0 | **332.6** |
| 1080p SDR, screen-sized, main process at 12 s, MiB | 809 | 797 | **789** |
| pointermove → DOM, p50 / p95 | 0.33 / 0.91 ms | 0.36 / 1.07 ms | **0.22 / 0.38 ms** |
| Seekbar click / context menu → DOM | 2.76 / 2.03 ms | 3.31 / 2.64 ms | **1.39 / 0.69 ms** |

- **In steady playback** the difference is the JS heap (27 / 15 / 4 MiB);
  graphics, mpv's blocks, malloc and the decoder are the same in the three.
  **At idle** (visible window) JS, VM and JIT code go 24.9 → 3.5 MiB, malloc
  35.7 → 34.6, and the rest 15.2 → 18.7 (the mapped image, inferred). The
  empty payload peaks and settles the same under both engines: the peak is
  the system's (#37).
- The image is the fastest of the three on every handled input in these
  runs; the interpreter alone is the slowest, by about 0.5 ms.
- `SK_MEMORY=low` changes nothing measurable on macOS; the `default` AOT
  memory profile adds 0.4 MiB here, because macOS charges a page when it is
  written.
- A second ~1.1 s hump of the same 32 × 8.00 MiB graphics objects comes in
  visible AOT launches at about 2.2 s (separate in 7 of 9, merged with the
  first in the other 2) and in interpreter launches at about 4.2 s; JIT,
  hidden and empty-payload launches showed none within the 9.3 s observed.
  The cause is not identified.
- Not checked: the `.app` layout, a hardened-runtime signature, AOT with the
  JIT kept.

#### The traffic lights of an inactive window (`a0a25228`, then `ce6aaca2`)

Reported from use: the traffic lights blinked as the idle chrome showed or
hid them and turned white-grey instead of red, yellow and green. Run on the
port's `<sk-surface>` route, 520×320 pt window, the chrome woken through
gpui's in-process pointer and hidden by the port's own idle path
(`setWindowButtonsVisible(v, { fade: 150 })`), captured by a
`CGWindowListCreateImage` loop at about 120 grabs a second.

- **Cause (measured).** On macOS 26 an inactive window's button is a
  SwiftUI-hosted view whose two drawing layers use the `plusD`
  (plus-darker) compositing filter, under a backing layer with
  `allowsGroupOpacity = YES`. The fade sets the button's `alphaValue` each
  frame; below 1 Core Animation flattens the subtree into an offscreen
  group where `plusD` blends against transparency, so the disc's raw
  light-grey fill shows for the whole fade, and the normal inactive look
  snaps back at 1 or 0: the blink. A key window is not affected. 28 frames
  with grey discs in two fade cycles, none in the key window.
- **Not the cause (measured).** The title bar's per-frame layout: in every
  committed visible state the close button stood at the requested
  (12, 17).
- **Fix.** `allowsGroupOpacity = NO` on each button's layer, set in
  `show_buttons`: 0 grey frames. Sent upstream as our pull request
  argon-chat/skathi-gpui#1, merged as `fccd451b` on 2026-11-23; Skathi
  master pins it from `75e7f11b`. Re-measured on master `ce6aaca2`, which
  carries it: **0 grey-disc frames** in 500
  changed frames over 3 fade cycles not key and 82 frames over 2 cycles
  key, against 28 before.
- **A capture trap (measured).** A ScreenCaptureKit stream that includes
  the window makes AppKit draw an opaque light-grey "window is shared" pill
  over the traffic lights, which slides in when they show and vanishes in
  one step when they hide: the same symptoms by another route. A
  `CGWindowListCreateImage` loop and `screencapture -l` do not trigger it.
- Not checked: macOS 15 and earlier, a physical pointer, the native-layer
  route (the buttons are AppKit views above the content, so inferred not to
  matter).

#### The window's frame source, and a paused fullscreen (`ce6aaca2`)

Skathi master `ce6aaca2` (skathi-gpui `fccd451b`) replaced #49's
`metal_drawable_count`, which saved nothing (see "On Skathi's own layer"),
with `Builder::metal_frame_source`: `Auto` (the default: the display link
paces frames, a resize, an activation and a surface frame after a pause
are drawn at once through `nextDrawable`), `DisplayLink` or
`NextDrawable`. Measured in the port, `release,lean`, JIT off by
environment, screen-sized window, controls hidden, cold storyboard, 3
launches per cell, route and source (36 in all, sources rotated);
`footprint` at 12 s, MiB, median [min–max]:

| Route | Cell | Auto | DisplayLink | NextDrawable | Next − Auto |
|---|---|---|---|---|---|
| `<sk-surface>` | 2160p24 SDR | 1430 [1429–1431] | 1431 [1426–1483] | **1375** [1373–1379] | **−55** |
| `<sk-surface>` | 2160p PQ | 1938 [1931–2000] | 1934 [1929–1976] | **1834** [1824–1881] | **−104** |
| `<sk-layer>` | 2160p24 SDR | 1372 [1371–1372] | 1371 [1369–1374] | **1317** [1316–1318] | **−55** |
| `<sk-layer>` | 2160p PQ | 1716 [1712–1719] | 1721 [1719–1768] | **1666** [1659–1724] | **−50** |

- **`NextDrawable` keeps three window drawables, `Auto` and `DisplayLink`
  five**, in every launch, while repainting and after 10 s paused; the
  saving is exactly two drawables (26.3 MiB each at 3456×1994, 52.6 when
  the `<sk-surface>` route's window is half float at PQ). After a resize
  to 1280×720 pt all three sources drop (`Auto` and `DisplayLink` to 3,
  `NextDrawable` to 2–3). The maintainer's own table (a locked screen,
  where `Auto` drew through `nextDrawable`) had `Auto` at 3; on a visible
  window `Auto` measures like `DisplayLink`, which fits his note that a
  window switching between the two paths keeps four or five.
- **Cadence, GPU and CPU: no difference in the medians.** 24.0–24.2
  distinct frames a second in 34 of 36 launches; two single runs (one
  `DisplayLink`, one `NextDrawable`, both on `<sk-layer>` at 2160p SDR)
  read 32.9 and 36.0, because something else changed in the window during
  their captures, so those two counts do not measure the video. mpv's drops
  only at start-up; GPU and CPU time within the run spread. One pacing
  difference within earlier spreads: `NextDrawable` on `<sk-surface>` at
  2160p SDR had 35 gaps off by one refresh against 28 and 14, and 4 gaps
  worse than ±1 refresh against 1 (3 runs each).
- **Live resize** (the `<sk-layer>` route, the page framing the layer with
  a border, 2 launches per source): `Auto` and `NextDrawable` showed no
  gap and no stretched page frame in any phase; **`DisplayLink` showed the
  page's previous frame stretched over the new size in 52 and 55 frames per
  run**, the cost its documentation describes.
- **First frame after an activation** (4 activations per route and
  source): 23–52 ms in the trials that answered, with no source-specific
  delay at this method's resolution (about 8 ms). Two trials, each the
  first activation after start (`Auto` on both routes), showed no response
  within 1.5 s and are not in the range; why was not checked.
- **Found on the way**: with `SK_WINDOW_FOCUS=never`, which every test
  host here sets, the page receives no `focus` or `blur` at all, even when
  the window becomes key; with `raise` it does.
- **A port defect, fixed in the port.** On the `<sk-surface>` route a
  fullscreen entered while paused kept the old picture stretched over the
  new element (zone-plate SSIM 0.153 against a reference, mpv's OSD still
  at the old height), because the render thread drew only when mpv reported
  a new frame. It now renders once when the element's size changes: SSIM
  **0.589**, against 0.600 for the same frame screen-sized and 0.588 on the
  `<sk-layer>` route (2 launches each). Windows does not show the defect
  (see "A lighter render set in the port").
- A smoke run on `ce6aaca2` (start screen, play, a seekbar click, fullscreen
  in and out, ASS subtitles) showed no regression on either route.
- Not checked: the frame source with the controls visible, in small and
  medium windows, at 1080p and in the mini player; pacing under load;
  WindowServer's share and energy.

### The ported player against the shipping one

Both ports against the shipping player on the same machine, as matrices of
window size, content and controls, three launches a cell interleaved, every
process each app owns counted, muted. The port keeps its own route
(`<sk-surface>`, a ring at about the video's pixel count, subtitles in an
overlay) and its own libmpv (0.42-dev with mpv PR #18845 and our patches);
the shipping player keeps its own (0.41, `vo=gpu-next` behind the web view).
Three things differ beyond the route and are part of every number below:

- **The storyboard is built during the sample in both apps**: the thumbnail
  cache was emptied before every launch, so every reading with a file
  includes the seekbar storyboard being built (the same code in both); on
  Windows the 2160p clips decode in software for the whole 30 s.
- **Pacing**: on macOS both apps run `video-sync=audio`; on Windows the
  shipping player uses `audio` and the port `display-resample`.
- **The port stubs what it has not ported** (on Windows 90 commands:
  torrents, casting, the catalog, updates and more), which the shipping
  player starts for real.

**How memory is counted, and why the two systems cannot be compared.** On
macOS the figure is `footprint` summed over every process the app owns:
Frame Player's main process, WebKit's WebContent, GPU and Networking
processes and an audio helper; for both apps the VideoToolbox decoder
service and any Metal compiler service started for them. Graphics memory on
Apple Silicon is system memory charged to the process that owns it, so these
totals include the GPU's share. On Windows private bytes, working set and
dedicated GPU memory (PDH, per process) are three separate counters summed
over the processes (13 for the shipping player: the player and twelve
WebView2 processes; one for the port), in MiB. Dedicated GPU memory is not
in private bytes, although on this driver the unshared D3D11/D3D12 textures
tested were charged to private bytes as untouched commit (one control run
per kind; see "Memory" under the Windows prototype). A macOS total and a
Windows total are therefore different quantities. Earlier comparisons in
this document against Frame Player's 605 MiB on macOS counted its main
process only.

#### macOS: Frame Player 1.48.1 against the port

Skathi `d153b34e` with the final lever set above; the port as an AOT image
without the JIT. The JIT-off-by-environment build is higher than AOT in all
but two cells, by up to 50 MiB (the JS heap, plus more storyboard malloc in
flight in the HDR cells); in the two, 1080p with the controls visible
screen-sized and in fullscreen, AOT read 2 and 24 MiB more. Windows: small
480×320 pt, a window of the screen's size, and native fullscreen (entered
through each app's own key, sent from inside the process; for this the
shipping app was a copy re-signed without the hardened runtime, checked
equal to the original in one cell, screen-sized 1080p). The machine was in
use; a browser renderer held 8–21 % GPU throughout. `footprint` at 12 s,
MiB, median [min–max]:

| Cell | Frame Player | Port | Δ |
|---|---|---|---|
| Idle, small window | 113 [112–116] | 72 [71–72] | −41 |
| Idle, screen-sized | 211 [210–214] | 122 [122–122] | −89 |
| 1080p24, small, controls hidden | 625 [623–627] | 677 [676–678] | +52 |
| 1080p24, small, controls visible | 835 [831–838] | 679 [678–680] | −156 |
| 1080p24, screen-sized, hidden | 755 [754–757] | 852 [850–852] | +97 |
| 1080p24, screen-sized, visible | 1046 [1022–1047] | 856 [856–860] | −190 |
| 1080p24, fullscreen, hidden | 769 [767–771] | 857 [857–867] | +88 |
| 1080p, ASS subtitles on screen | 760 [754–762] | 969 [967–971] | +209 |
| 1080p, paused | 456 [456–456] | 461 [461–461] | +5 |
| 2160p24 SDR | 1211 [1208–1233] | 1422 [1422–1425] | +211 |
| 2160p PQ, screen-sized, hidden | 1499 [1498–1500] | 1927 [1924–1977] | +428 |
| 2160p PQ, screen-sized, visible | 1785 [1766–1793] | 1926 [1924–1969] | +141 |
| 2160p PQ, fullscreen, hidden | 1512 [1508–1567] | 1975 [1968–1991] | +463 |

- **The port is lower wherever WebKit is large**: every idle cell, and
  every 1080p cell with the controls visible, where WebKit's GPU process
  grows to 217–234 MiB. **It is higher wherever video plays with the
  controls hidden**, most at 2160p PQ (+428 to +463).
- **Where the extra memory is** (main process, screen-sized; the decoder
  service apart):

  | Chunk | 1080p, Frame Player / port | 2160p PQ, Frame Player / port | Whose |
  |---|---|---|---|
  | Window drawables | 79 (3, 10-bit) / 133 (5, 8-bit) | 79 / 265 (5, half float) | Skathi keeps five; half float for HDR |
  | Skathi's copies | 0 / 25 | 0 / 160 | Skathi's `<sk-surface>` |
  | Our rings | 0 / 8 | 0 / 53 | ours |
  | mpv's 8-MiB blocks | 328 / 384 | 336 / 400 | the port's presentation path |
  | Decoder service | 51 / 60 | 314 / 385 | our libmpv's FFmpeg build |
  | WebKit processes | 96 / — | 95 / — | Frame Player |

  A control, our libmpv swapped into the old app (video in mpv's own
  window, as stock mpv has no `--wid` on macOS), holds the decoder
  service's 60 / 385 exactly as the port does, and mpv's blocks at 328 /
  336 as the old app does (measured). So the decoder difference goes with
  the build and the blocks' difference with the port's presentation path;
  that it is the render API's own targets is a hypothesis. With ASS
  subtitles on screen our rings go 8 → 34 MiB (+26) and Skathi's copies
  25 → 107 (+82); both return to the no-subtitle values once the subtitles
  clear.
- **CPU** (% of a core, 8–18 s) is level within the spread in the playing
  cells, where the 2160p and ASS cells read 115–155 % in both apps because
  the storyboard is being built. One exception: the medium window with
  1080p and the controls visible, where the port was lower (Frame Player
  36.7 [26.8–36.8], the port 24.9 [24.5–26.0]). Idle and paused the port
  spends more: +0.6 (screen-sized idle) to +5.1 points (paused, 4.4 →
  9.5 %) for AOT, up to +6.9 with the JIT off; inferred, not investigated:
  the interface's once-a-second resync and Skathi's frame loop.
- **GPU time** is higher: 8.7 → 13.2 % for 1080p at screen size, 9.9 →
  14.1 with subtitles, and **about doubled in native fullscreen** (10.6–14.6
  → 20–24 %), inferred to be Skathi's copies and drawables at the full
  element size plus the half-float composition; not broken down. Level at
  2160p PQ in a window.
- **Start-up** to the first video frame at screen size: 933–982 ms against
  1217–1411 (AOT); in the small window level. The fullscreen cells' start-up
  is not comparable (the key and the transition fall inside the capture).
- **Cadence**, window captures at 120 Hz: in the 24 fps cells with the
  controls hidden 24.0–24.2 distinct frames a second in every column (60.0
  in the 60 fps cell); the share of exact five-refresh gaps varies between
  launches with no consistent winner. With the controls visible the capture
  counts the bar's repaints too, so those cells measure the interface, not
  the video.

#### Windows: Frame Player 1.49.1 against the port

The public portable release in a folder of its own against the port on
Skathi `2b08fd37` (0.7.0), no devtools, as JIT and as an AOT image with
`compact`. Both driven the same way from outside: the client area sized
with `SetWindowPos`, real fullscreen by a key posted to the app's own
window, pause through mpv's IPC. Reading at 30 s, CPU and GPU time over
20–30 s; MiB, median [min–max] of 3 (one value where the three agree). The
port's JIT column; AOT is within −12 to +22 MiB of private bytes of it,
12–36 MiB lower in working set, 0–0.9 points lower in CPU.

| Cell | Private bytes, old / port | Working set, old / port | GPU dedicated, old / port |
|---|---|---|---|
| Idle, screen-sized | 539 [538–545] / 561 [561–563] | 773 [772–776] / 225 | 270 [230–270] / 562 |
| Idle, screen-sized, after the idle fix (JIT) | — / 376 [374–376] | — / 203 [202–203] | — / 299 |
| 1080p24, screen-sized | 789 [783–793] / 665 [660–665] | 923 [923–927] / 353 [352–356] | 478 [477–478] / 532 |
| 1080p60, screen-sized | 810 [809–813] / 670 [664–673] | 944 [943–946] / 358 [358–361] | 478 [477–478] / 532 |
| 1080p, ASS on screen | 809 / 763 [757–763] | 944 [942–950] / 393 [391–393] | 478 / 593 |
| 1080p, paused | 844 [841–865] / 652 [648–655] | 913 [910–940] / 356 [347–357] | 541 [541–543] / 533 |
| 1080p, real fullscreen | 806 [801–812] / 669 [666–671] | 924 [923–930] / 356 [356–357] | 562 / 533 |
| 2160p24 SDR, screen-sized | 1100 [1098–1131] / 1391 [1388–1427] | 1144 [1142–1175] / 632 [631–665] | 804 [804–817] / 1310 |
| 2160p HDR10, screen-sized | 1226 [1223–1260] / 1526 [1520–1528] | 1255 [1245–1281] / 743 [739–744] | 1141 [1141–1154] / 1684 |
| 2160p HDR10, 1920×1080 window | 1154 [1147–1171] / 1177 [1172–1183] | 1220 [1219–1248] / 740 [738–742] | 971 / 1134 |

| Cell | CPU, % of a core, old / port | GPU time, %, old / port |
|---|---|---|
| Idle, screen-sized | 5.5 [5.4–5.5] / 0.6 | 0 / 0 |
| 1080p24, screen-sized | 11.5 [11.4–11.5] / 9.7 [9.6–10.2] | 2.29 [2.28–2.30] / 3.59 [3.58–3.60] |
| 1080p60, screen-sized | 18.3 [18.1–18.5] / 17.1 [17.0–17.5] | 5.69 [5.68–5.70] / 8.93 [8.92–8.93] |
| 1080p, ASS on screen | 11.8 [11.6–11.9] / 9.8 | 2.31 [2.30–2.32] / 3.73 [3.72–3.73] |
| 1080p, paused | 5.5 / 0.9 [0.8–0.9] | 0 / 0 |
| 2160p HDR10, screen-sized | 14.1 [14.0–14.2] / 10.2 [10.0–10.3] | 6.51 [6.51–6.53] / 7.67 [7.65–7.68] |

- **Better on the port**: working set in every cell (−39 to −74 %), CPU in
  every cell (the old app spends 5.5 % of a core idle and paused), and the
  first video frame (837 ms old, 687 JIT, 604 AOT). Private bytes are lower
  in every cell with 1080p content and in the idle small, medium and
  fullscreen windows; before the idle fix the idle screen-sized window was
  22 MiB higher (561 against 539).
- **Cadence**: in the 8 s sampled after 32 s, controls hidden, every video
  frame was shown and none skipped by either app in all 114 launches (the
  six control launches included), and mpv dropped none. Timing differs: the
  old app's 99th-percentile gap is often a whole 240 Hz refresh long (45.8
  to 46.3 ms for a 41.67 ms frame), the port's 42.0 to 45.5 ms; the port's
  mpv reported 1 to 4 delayed renders in some cells, none visible as a
  missing frame.
- **Worse on the port**: dedicated GPU memory higher in 19 of 21 cells (over
  the 10 % / 50 MiB threshold in 18): +54 MiB for 1080p at screen size, +453
  to +591 MiB in the 2160p cells at 3840×2160 (screen-sized, fullscreen,
  controls visible); GPU shared memory in all (+37 to +126 MiB); GPU time
  in every playing cell, and the whole increase is the 3D engine (2.32 %
  against 1.01 for 1080p24; video decode reads the same); private bytes in
  the 2160p cells at 3840×2160 (+268 to +339 MiB, either build) and in two
  smaller 2160p HDR10 windows (+52 to +74 MiB), within 31 MiB in the
  others.
- **Why** (texture sizes are arithmetic, which ones exist is read from the
  source, the attribution of totals is inferred unless a control measured
  it): the picture exists twice, once in our half-float ring and once in
  Skathi's copies; the window composes in half float from mpv's first frame;
  the overlay adds 61 MiB while subtitles show and 22 after; the libmpv is
  +12 MiB (1080p) and +96 MiB (2160p HDR10) of GPU memory, measured by
  putting the port's libmpv under the old app; and per shown frame the port
  does a copy, an upscale in Skathi and a whole-window composition where
  the old app's picture is a swap chain DWM takes directly. With a 2160p
  clip at screen size this driver commits in the one process what the old
  app spreads over the player and the web view's GPU process (inferred).
- **The idle fix.** Without a file the port made its ring at the element's
  size and mpv drew a black frame into it. Now the ring is made when the
  video's size is known and dropped when the file is gone: idle at screen
  size 562 → 299 MiB of GPU memory, 561 → 376 MiB private and 225 → 203
  MiB working set (JIT), against the old app +29 MiB of GPU memory instead
  of +293, and −142 in fullscreen. The window still switched to half float
  at start, because Skathi followed a half-float `<sk-surface>`'s format,
  not its frames; playing cells were not re-measured with this build (the
  ring is the same once a file is loaded, so unchanged is inferred).
- **#46's fix, re-measured** (master `9dcec055`, JIT, the port's sources
  and payload unchanged, 3 launches per idle cell, one per playing cell,
  reading at 30 s): the idle start screen keeps the window 8-bit (no
  "switched to HDR output" line in 6 of 6 idle launches, where every idle
  launch on `2b08fd37` had one) and holds **140 MiB of GPU memory and
  309–311 MiB private** at screen size and in real fullscreen, against 299–
  300 and 373–376 before; that is the native-layer route's idle figure and
  130 MiB below the old app's 270 at screen size. Once a file plays the
  window goes half float as before, for SDR content too: that is the
  port's doing, since it presents half-float frames for every file (an
  8-bit path for SDR files was not built). The playing cells read as in the
  matrix (1080p24 532 MiB of GPU memory, 2160p HDR10 1684).

### Native players as a reference

The matrices' cells and timeline for the players people use, to read the
two Frame Players against. Not re-run for Frame Player or the port: their
columns are the matrices' medians. Not every player could be put in every
cell the same way: on macOS VLC's windowed control bar is always on and its
fullscreen panel stayed up, IINA's screen-sized window is aspect-locked
(1728×972 pt), and many fullscreen launches had to be repeated because the
machine was in use and the fullscreen Space was left.

**macOS** (MiB, all processes; CPU % of a core and the app's own GPU time,
8–18 s; screen-sized window, controls hidden unless said):

| | Frame Player | Port | AVKit stand-in | IINA 1.5 | mpv 0.41 | VLC 3.0 | Elmedia 8.24 |
|---|---|---|---|---|---|---|---|
| Idle | 211 | 122 | 17 (QuickTime 62) | 125 | 190 | 64 | — |
| 1080p24 | 755 | 852 | 67 | 815 | 820 | 452 | 495 |
| 2160p PQ | 1499 | 1927 | 240 | 1479 | 1489 | 743 | 942 |
| 2160p PQ, fullscreen | 1512 | 1975 | 241 | 1513 | 1512 | 810 | — |
| ASS on screen | 760 | 969 | — | 811 | 799 | 487 | — |
| Paused | 456 | 461 | 69 | 643 | 481 | 468 | — |
| CPU / GPU, 1080p24 | 26.2 / 8.7 | 27.4 / 13.2 | 3.5 / 0.0 | 15.5 / 6.4 | 17.0 / 7.7 | 11.1 / 1.9 | 28.6 / 2.4 |
| GPU, fullscreen 1080p / PQ | 10.6 / 14.4 | 21.3 / 20.2 | 0.0 / 0.0 | 8.0 / 11.1 | 9.4 / 13.8 | 3.2 / 3.0 | — |
| First frame, 1080p, ms | 962 | 1308 | 281 | 1057 | 487 | 1821 | — |
| HDR on screen (max, 1.0 = SDR white), windowed / fullscreen | not checked | EDR (inferred) | 2.1 / 3.9 | 8.9 / 9.0 | 1.00 / 1.00 | 1.00 / 1.00 | 1.00 |

- **QuickTime** opens a file paused and could not be started without
  input, so a ~80-line AVKit player (one `AVPlayerView`, muted, autoplay)
  stands in for its playback. **Its column is not like the others**:
  AVFoundation hands frames to WindowServer, so the app's own GPU time is 0,
  and the picture's surfaces beyond the decoder service's pool are not in
  its footprint (charged to WindowServer, inferred). WindowServer's GPU time
  read 18.3–19.1 % in its windowed cells against a 1.5 % no-player baseline
  median (baseline range 0.9–17.8), so about 17–18 points; WindowServer's
  memory needs root and was not read. Read the column as what the app is
  charged, not what playback costs. WindowServer was not recorded at all
  for Frame Player and the port. Elmedia ran after the others, not
  interleaved, three cells only, and its 1080p window was narrower than
  screen-sized.
- **HDR** (the window's maximum in an HDR capture, 1.0 = SDR white): IINA
  passes PQ through into EDR (8.9×), AVFoundation
  shows EDR with Apple's own headroom handling (2.1 windowed, 3.9
  fullscreen), and stock mpv, VLC and Elmedia tone-map to SDR (1.00). So
  part of the cheaper HDR cells is not showing HDR at all. Frame Player and
  the port were not peeked in this run.
- **Reading**: the libmpv players form one class. Bare mpv, IINA and
  Frame Player are within about 10 % of one another in the comparable
  cells (Frame Player 755 against mpv's 820 at 1080p, 625 against 690 in
  the small window), and **at 1080p, paused and idle the port is in that
  class** (+32 to +37 MiB over IINA and mpv at 1080p, below mpv idle and
  paused). **4K is its largest memory gap**: at 2160p PQ +438 to +463 MiB
  (+30 %) over IINA and mpv, at 2160p SDR +101 over IINA and +254 over
  mpv; with ASS on screen +158 to +170; all of it the chunks above.
  Fullscreen GPU time (about 1.5–2.7× the libmpv players' medians) and idle
  and paused CPU (+4–9 points) are worse too. The CPU over mpv and IINA at
  1080p (~10 points) Frame Player has as well, so it is not the port's
  route; its cause was not isolated. VLC is about half of any libmpv
  player.

**Windows** (MiB, all processes; 1080p24 and 2160p HDR10 at screen size;
first frame with a warm disk cache):

| | Frame Player 1.49.1 | Port (JIT) | Media Player | MPC-HC 2.8.5 | VLC 3.0.24 | mpv 0.42-dev |
|---|---|---|---|---|---|---|
| Private bytes, 1080p / HDR10 | 789 / 1226 | 665 / 1526 | 168 / 371 | 188 / 288 | 230 / 404 | 211 / 509 |
| Working set, 1080p / HDR10 | 923 / 1255 | 353 / 743 | 216 / 336 | 118 / 199 | 192 / 345 | 136 / 237 |
| GPU dedicated, 1080p / HDR10 | 478 / 1141 | 532 / 1684 | 558 / 710 | 184 / 799 | 247 / 999 | 268 / 1098 |
| Idle: private / GPU | 539 / 270 | 561 / 562 (376 / 299) | 154 / 83 | 11 / 0 | 68 / 0 | 86 / 110 |
| CPU, % of a core, 1080p / idle | 11.5 / 5.5 | 9.7 / 0.6 | 1.4 / 0.0 | 1.6 / 0.0 | 2.4 / 0.4 | 3.5 / 0.0 |
| GPU time, 1080p (3D engine) | 2.29 (1.01) | 3.59 (2.32) | 1.37 (0.16) | 1.87 (0.60) | 1.63 (0.37) | 2.31 (1.03) |
| First video frame, ms | 837 | 687 (AOT 604) | 1283 (from its activation) | 3492 | 1564 | 223 |
| HDR10 clip, peak over the window × SDR white | 5.30 | 4.16 | 69.2 | 69.2 | 69.2 | 5.30 |

- **Caveats**: Media Player's window belongs to a shared system host whose
  processes are not counted, which flatters its numbers in every row; its
  fullscreen cells were not run and its "paused" cell was playing. mpv is a
  third-party build of 0.42-dev with its own OSC; kept always visible, the
  OSC costs it 180 MiB more. All decode in hardware by the decode engine's
  load, which reads the same for all. Both Frame Players build their
  storyboard during every reading with a file; none of the native players
  builds one. MPC-HC's 3.5 s to the first frame was the same in all three
  launches and was not looked into.
- **HDR**: the peak is read from a capture of the composed desktop, in
  scRGB units of the SDR white level. Media Player, MPC-HC and VLC read
  69.2, the saturated synthetic clip's full code values read as PQ:
  consistent with handing this clip to the display untouched, not a
  luminance anyone sees. mpv and Frame Player render to a 1000-nit HDR10
  target, and the composed desktop's maximum over the window reads 5.30
  (about 1270 nits); the port maps to the display's headroom in scRGB
  (4.16). Which of them looks right on this display was not judged.
- **Reading**: **both Frame Players are several times heavier than any
  native player** in process memory and CPU. Against bare mpv with 1080p24,
  the fairest floor: the port holds 3.2× its private bytes and spends 2.8×
  its CPU, the shipping player 3.7× and 3.3× (at 2160p HDR10 the port's CPU
  is 2.0× mpv's). Against the cheapest native player in each cell the port
  is at 3.2–5.3× the private bytes while playing and 5.4–7.1× the CPU. The
  whole-app difference over bare mpv is 450–600 MiB and 6–8 points of a
  core; the interface runtime (WebView2, or Skathi's JavaScript) is the
  likely main part, together with the storyboard both Frame Players build,
  but it was not isolated. **The port is additionally heavier in GPU**: 2×
  mpv's GPU memory and 1.6× its GPU time at 1080p, where Frame Player is at
  mpv's GPU time (2.29 against 2.31) and its 3D engine (1.01 against 1.03).
  In these warm-cache runs only mpv showed its first frame sooner than the
  port.

### A native video layer under the page (prototype)

The `<sk-surface>` route's costs above are mostly the picture's own path:
our ring, Skathi's copies, the window going half float for HDR, and on
Windows the overlay. The shipping player avoids all of it by putting mpv's
output under a transparent web view. Both platforms were tried the same way
under Skathi's window, on Skathi 0.7 (macOS `dd8b4ccc`, the 0.7.0 release
plus a build fix; Windows `2b08fd37`), first with a test pattern, then with
the port's mpv. **This is a prototype**: it relies on handles Skathi
exposes "at your own risk". The supported form was asked for in #43 and
split upstream into #44 (a native layer under an element), #45 (re-homing
after device loss), #46 (the empty half-float surface), #47 (headroom per
window), #48 (a backdrop the page cannot change) and #49 (the macOS
drawable count). All six are closed: Skathi now has `<sk-layer>`, and the
port's run on it is under "On Skathi's own layer" at the end of this
section. What follows first is the prototype on raw handles.

- **macOS: works on unmodified Skathi.** The main window's `NSView` comes
  from `HostContext` (#35); with `backdrop: "transparent"` the window and
  gpui's `CAMetalLayer` are non-opaque. A layer-hosting `NSView` with our
  own `CAMetalLayer`, inserted below gpui's view (where gpui puts its own
  blur view), shows through the page. A sublayer or a borderless child
  window work as well and cost the same. The page blends over it in
  sRGB-encoded space with no fringe (the 50 % panel reads 0.544 where
  0.539 is expected), and the layer follows resize and native fullscreen.
- **Windows: works on unmodified Skathi through an owner window.** Skathi's
  window is a D3D12 composition swap chain shown through DirectComposition,
  with an empty "underlay" visual under the page that `<iframe>` uses (read
  from the source). Our own composition target on that window is refused
  (`DCOMPOSITION_ERROR_WINDOW_ALREADY_COMPOSED`; with `topmost` it lands
  above the page), and a child window with its own swap chain lands above
  the page too. What works: a top-level window of ours, made the owner of
  Skathi's window and kept on its client area, hit-test transparent and
  never activated. Skathi's own underlay also works, through a 22-line
  local patch that hands it to the embedder, and costs the same to the
  megabyte. The panel blends within 3 of 255 of the expected mix; the layer
  follows resize, maximise and real fullscreen.
- **HDR lives in the layer alone.** Skathi's window stayed 8-bit SDR in
  every launch on both systems while the layer showed EDR (macOS: 2, 4 and
  8× SDR white read back exactly) or HDR10/scRGB (Windows: 4.09 and 4.00×),
  and the interface's white read 1.00, also drawn directly over the EDR
  area.
- **The port's mpv in the layer.** macOS: mpv's Vulkan render API renders
  straight into the drawable's own IOSurface, imported into MoltenVK, and a
  Metal command buffer presents it once mpv's release value arrives.
  Windows: mpv's D3D11 render API renders into the back buffer of an HDR10
  flip swap chain (`R10G10B10A2`, ST 2084/BT.2020, two buffers). Neither
  has a ring of ours, a Skathi copy or an overlay pass: the picture is the
  window's size and mpv draws the subtitles into it.

All processes, screen-sized window, controls hidden, medians [min–max] of
3 unless said. On macOS both routes come from the same executable and
Skathi, run as one interleaved batch (JIT off by environment in both). On
Windows only the layer route was run in this stage; its `<sk-surface>` and
Frame Player columns are the Windows matrix's medians from the day before,
and the native players' from their own stage. Windows memory in MiB.

| | `<sk-surface>` | Native layer | Frame Player | IINA / mpv |
|---|---|---|---|---|
| macOS, 1080p SDR, MiB | 865 [865–923] | 873 [865–877] | 755 | 815 / 820 |
| macOS, ASS on screen, MiB | 985 [972–1023] | **881** [876–891] | 760 | 811 / 799 |
| macOS, 2160p PQ, MiB | 2008 [1925–2050] | **1719** [1718–1779] | 1499 | 1479 / 1489 |
| macOS, app GPU time 1080p / ASS / PQ, % | 13.1 / 16.6 / 15.7 | **9.3 / 8.8 / 12.1** | 8.7 / 9.9 / 14.7 | 6.4 / 7.6 / 9.9 and 7.7 / 8.2 / 13.1 |
| Windows, 1080p: private / GPU dedicated | 665 / 532 | 669 [668–696] / **487** | 789 / 478 | mpv 211 / 268 |
| Windows, 1080p + ASS: GPU dedicated | 593 | **489** | 478 | mpv 269 |
| Windows, 2160p HDR10: private / GPU dedicated | 1526 / 1684 | **1107** [1106–1109] / **1228** | 1226 / 1141 | mpv 509 / 1098 |
| Windows, GPU time 1080p / ASS / HDR10, % | 3.59 / 3.73 / 7.67 | **2.52 / 2.54 / 6.23** | 2.29 / 2.31 / 6.51 | mpv 2.31 / 2.30 / 6.51 |
| Windows, idle start screen: GPU dedicated | 562 (299 after the idle fix) | **140** (one launch) | 270 | mpv 110 |

On macOS the `<sk-surface>` column agrees with the macOS matrix's
JIT-off medians within 1–2 % in these three cells, so the move to 0.7 made
no difference visible there; it is not a re-run of the matrix, and the
decoder service at 2160p PQ read 409 MiB here against 385 there. An AOT
build of the layer route is expected 10–50 MiB lower (inferred from the
matrix, not measured).

- **What it fixes**: the copies, the ring and the overlay pass, and with
  them the half-float window. macOS closes about half the gap to IINA and
  mpv at 2160p PQ (+490 → +230 MiB) and about 60 % of it with subtitles;
  the app's own GPU time falls by 23–47 %, which puts it above both IINA
  and mpv at 1080p (9.3 against 6.4 / 7.7) and with subtitles (8.8 against
  7.6 / 8.2) and between them at PQ. Windows reaches Frame Player's GPU
  memory at 1080p (487 against 478) and comes within 87 MiB of it at 2160p
  HDR10; the DLL swap under the old app measured 96 MiB for the newer
  libmpv there, so on this route that share is an estimate. GPU time falls
  below Frame Player's at 2160p HDR10 (6.23 against 6.51) and stays a
  quarter of a point above it at 1080p and with subtitles (2.52 and 2.54
  against 2.29 and 2.31).
- **What passed in the scripted checks**: on macOS cadence (24.0 distinct
  frames a second in both routes, three launches a cell), frame stepping,
  screenshots through mpv and native fullscreen (entered in process); on Windows cadence in the 8 s sample (193 of 193 frames shown in
  all nine launches; mpv's drop counter rose by one in one launch without a
  frame missing in the capture), frame stepping, a resize while paused and
  real fullscreen (one launch).
- **What it does not fix**: the interface runtime. CPU does not move
  (Windows 9.4–9.8 % against 9.7–10.2; on macOS level within the noise of
  the storyboard), and on Windows the route barely changed total private
  bytes at 1080p (669 against 665); what the rest of that total is was not
  broken down. The distance to native players in process memory and CPU
  stays.
- **What is left on macOS**: Skathi's window keeps a pool of five
  screen-sized 8-bit drawables, 133 MiB, while playing and also when paused
  (one paused run per route). Three instead of five would save about 53 MiB
  at this size. A minimal page shows why five: Skathi holds three while
  idle, even with our layer animating under it, and five once it repaints
  every frame itself, so the five of #38 are a busy-window effect, and HDR
  only doubled their size. Why the port's page repaints every frame with
  the controls hidden was not checked. Paused, the layer route costs +64
  MiB for its own three drawables (one run); a count of two or releasing
  them on pause was not tried. (Both answered since: a drawable count for
  Skathi's window saved nothing and was replaced by the frame source,
  whose `NextDrawable` keeps three; our own layer is capped at two. See
  "On Skathi's own layer" and "The window's frame source".)
- **A GPU-busy chunk** that is no one's in particular: a process holds a
  block of unmapped driver memory while it keeps submitting GPU work and
  gives it back within about a second of going idle (258 MiB with the test
  pattern, 193 for a bare Metal app with the same layer, 268 for Skathi
  animating its own spinner, 551 in the port at 2160p PQ). The libmpv
  players hold the like while playing (mpv's falls from 430 to 154 MiB when
  paused).
- **Workarounds found on macOS**: MoltenVK cannot import a drawable
  allocated lossless-compressed, so the layer needs `framebufferOnly` off
  (the memory is the same; the bandwidth saving is lost); layer changes for
  EDR made off the main thread never land, because nothing commits that
  thread's implicit transaction; and the screen's current headroom stays
  1.0 until EDR content is on screen, so the target peak has to come from
  the potential headroom (16.0 on this panel, `target-peak` 3248). The PQ
  clip's window maximum in the HDR capture, sRGB-encoded, then reads 3.29
  on this route against 2.19 on the `<sk-surface>` route, which follows the
  current headroom: two different tone-mapping targets, not a measure of
  either's correctness (inferred).
- **SDR on Windows** (one launch per condition, on this HDR desktop). mpv
  puts SDR white at 203 nits in an HDR10 target, and this desktop's SDR
  white is 240, so the clip's saturated bars first read 0.82 of the
  interface's white. With `hdr-reference-white` set to the desktop's SDR
  white level (as Skathi reads it for the display) they read 0.97, as in
  the file; the HDR10 clip's peak is unchanged (5.13). The SDR clip's
  window maximum went from 0.85 to 1.99 at the same time, along the test
  pattern's sharp edges (scaler overshoot in an unclipped target, inferred).
  An 8-bit path (`B8G8R8A8` sRGB, mpv's `target-trc=srgb`, HDR content
  tone-mapped) is chosen when the display is not in HDR. In the code the
  options are re-set when the mode, the white level or the peak changes,
  and swap chains are remade when the display's mode changes; no live
  transition was tested, and the 8-bit path was measured only forced on
  this HDR desktop, not on a real SDR desktop. Mid-greys still differ by
  path (120 here, Frame Player 121, the 8-bit layer 127, `<sk-surface>`
  134), which is the rest of the colour option set, not applied here.
- **Open**: resize synchronisation (macOS: the layer follows AppKit's own
  layout and mpv renders the new size on its next frame, so the picture can
  be one frame behind the interface, inferred and not checked; Windows: the
  layer window moves on the render thread's next turn, up to 50 ms while
  paused, not measured); the mini player's always-on-top panel; another
  window getting between the two on Windows, and minimise and restore,
  virtual desktops, Alt+Tab and the taskbar thumbnail there; the context
  menu over the picture; WindowServer's and DWM's share of the work; the
  AOT build; Skathi's own screenshots, which do not contain the picture (by
  design, as with a web view). Resize synchronisation and the mini player
  have since been measured on `<sk-layer>` (below); the owner window on
  Windows is no longer needed there.

#### On Skathi's own layer: `<sk-layer>` (macOS `a0a25228` and `ce6aaca2`, Windows `04e24dee`)

The port moved onto the supported API: a `<sk-layer name="video">` under
the interface, our `skathi::Layer` registered with `host.layer("video", …)`,
`Builder::backdrop("transparent")` (#48) instead of the payload's own
backdrop, and the display's values from `Builder::on_display_change` (#47)
instead of our own reads. `attach` hands over the place under the page (on
macOS the page's `NSView`, our view goes into its superview below it; on
Windows the window's composition device and underlay visual, our swap
chain becomes the content of a visual of ours added to it), and `place`
gives the element's box in device pixels, or `None` when it is hidden.
Both platforms built and ran it with nothing to fix in Skathi. On Windows
this route is `FPUI_ROUTE=sklayer`: the same HDR10 or 8-bit composition
swap chain as the owner-window route, with no second window.

**Cost: as the prototype on raw handles** (measured, screen-sized,
controls hidden):

| | macOS, all processes, MiB | Windows, GPU dedicated / private, MiB |
|---|---|---|
| Idle start screen | 130 | 139 / 312 (owner window 140 / 309) |
| 1080p SDR | 867 (prototype 873) | 487 / 668 (owner window 487 / 663) |
| ASS on screen | 885 (881) | 489 / 686 (489 / 719) |
| 2160p PQ / HDR10 | 1718 and 1763, two launches (1719) | 1228 / 1126 (1228 / 1107) |

One launch a cell (macOS PQ two), so a few per cent is within what one
launch tells. App GPU time on macOS 8.5 / 9.3 / 12.4–13.3 % (prototype 9.3 /
8.8 / 12.1), on Windows 2.55 / 2.55 / 6.22 % (owner window 2.54 / 2.55 /
6.23); 24 distinct frames a second, no frame dropped. The HDR window
maximum is the prototype's: 3.293 on macOS, 5.14 × SDR white on Windows,
where Skathi's window stays 8-bit under the HDR10 layer.

**Placement and resize.**
- **macOS: in the page's frame** (measured). Window-captured at full pixel
  size with the page framing the layer by a border: **0 frames with a gap
  in 275 frames** of a native live-drag resize (86 sizes), grow and
  shrink; `place` came 176–207 times for ~150 size changes. An animated
  `appWindow.setBounds` grow had 2 of 84 frames with a 1-pt seam on all
  four sides at once, the page's own box 1 pt short, so not a lag (cause
  not checked). On `ce6aaca2` with the frame source `Auto` or
  `NextDrawable` the same probe showed no gap and no stretched page frame;
  `DisplayLink` stretched the page (see "The window's frame source").
- **Windows: one desktop refresh behind** (measured on `04e24dee`, paused,
  64 steps of 20 px at one every 16 ms, every desktop frame read at 240 Hz).
  Growing, there was one desktop frame (4.2 ms) per step in which the page
  was at the new size and the layer one step short, 63 frames in 64 steps;
  the owner-window route was behind for one or two (97 frames in 64 steps).
  In 12 further frames the layer was up to 20 px wider than the page, our
  route's own ordering (its buffer resize lands before the stretch is
  reset). Shrinking could not be resolved by this probe. The maintainer
  traced the lag to the commit order: a layer committed the shared
  composition device from `place`, apart from the page's frame. Since
  `beab4bb5` (on master with `329afd60`) Skathi commits it after presenting
  the frame that placed the layer, and a layer must no longer call
  `Commit` itself. **Not re-measured by us.**
- `display: none` gives `place(None)` (within 10 ms on macOS) and the area
  is empty; showing the element brings the layer back on the next frame.
  Native and real fullscreen, maximise and restore: one `place` each way,
  no second `attach`.

**The mini player (macOS).** Skathi moves the content into a level-3
panel, a new window. On `a0a25228` `attach` was not called on that move;
the layer showed only because our view moved with the content view. Since
`beab4bb5` `attach` is called again whenever the page's view is in another
window or under another superview, with the new `NSWindow` in `LayerHost`:
on `ce6aaca2` four attaches (start, into the panel, back, into the panel
again), the picture placed to the panel's rounded edges and live in every
capture, and closing the window from mini mode exited cleanly in 250 ms
(measured, the player's own key posted in process).

**Device loss (Windows).** In process only (`RemoveDevice` on Skathi's
D3D12 device, a clip playing): Skathi made a new device and called
`attach` a second time with a new device and underlay, then `place`; our
visual was made anew with the same swap chain, and the picture was on the
desktop at the same brightness in the shots taken 1, 6 and 13 s later
(measured; the first shot was at 1 s, so how long the recovery took was
not timed). This is #45's case,
which the maintainer closed as covered by #44. Not checked: a removal of
the layer's own device, a real driver reset.

**`on_display_change` (#47)** (measured). macOS: fires before the first
frame with `hdr true`, `potential_headroom 16.0`, `headroom 1.0` and the
refresh rate; with PQ playing it reports the current headroom climbing
1.02 → 6.15 in about 200 ms steps (9 calls over 2.6 s); once with SDR or
nothing playing. mpv's target peak (3248 nits) and the HDR capture peak
(3.293) are unchanged from our own reads. Windows: once at start, before
the first file, with HDR on, SDR white 240 nits, peak 1000, full-frame
1000, potential headroom 4.167; the route reaches the earlier brightness
from it alone, and it did not fire again in any launch, the device loss
included. A change while running (HDR switched, the SDR white level,
another display) was not tested on either system.

**`Builder::backdrop` (#48)** (measured). The window is transparent from
its first frame; the page setting `appWindow.backdrop` (`"none"` on macOS,
`"mica"` on Windows) reads back `"transparent"`, the picture stays visible,
and the log carries one advisory line.

**macOS drawables.**
- **Skathi's window: a count does not bound the pool** (measured on
  `a0a25228`, 2160p PQ playing and paused, 1080p SDR, 2 launches per case
  at PQ). With #49's `metal_drawable_count(2)` the layer read back
  `maximumDrawableCount` 2, yet `footprint -v` still listed five
  screen-sized `CAMetalLayer Display Drawable` regions, 133 MiB: **0 MiB
  saved**, and no pacing change either. The maintainer reproduced it,
  removed the setting and replaced it with the frame source, which does
  move the count (see "The window's frame source").
- **Our own layer: capped at two** (measured on `ce6aaca2`, 1080p
  screen-sized, 12 + 12 + 2 launches). Under the lighter render set our
  layer held 3–4 screen-sized drawables instead of 2, +52 MiB: a cheaper
  render reached the third `nextDrawable` in mpv's start-up burst while
  the first two were queued, so Core Animation's pool grew to the layer's
  maximum of 3, and our import of each surface kept a fourth resident.
  Neither half alone helped: releasing an import unused for 48 frames gave
  ours back but Core Animation kept the surface; a maximum of 2 alone
  still saw 3–4 distinct surfaces handed out in 6 of 13 runs. **Both
  together hold 2 in all 13 runs**, the lighter set then costs less than
  the default in every run (790–851 against 867–920 MiB), and cadence is
  unchanged (24.0 frames a second, no drops after start-up). A port
  change: `maximumDrawableCount` 2 and the release through
  `MPV_RENDER_PARAM_VULKAN_RETIRE` in our render API. Not checked: 60 fps
  content, fullscreen and the mini player with two.

**Port fixes found on the way (measured).** MoltenVK refused drawables
that were handed out compressed when `framebufferOnly` was turned off
after the layer existed (in 3 of 8 PQ launches no frame was shown), so it
is now set when the layer is made; a few transient import errors still
appear in the first frames after the SDR → half-float switch, then
playback is normal (ours, not Skathi's).

Not checked: AOT; the context menu over the layer; clip and corner radii
other than a full-window box; on Windows a real SDR desktop, minimise and
restore, virtual desktops and the taskbar thumbnail with the layer.

### A lighter render set in the port

Whether a cheaper mpv option set costs anything visible, measured in the
port on both systems and both routes, and in the shipping player as a
control (macOS 2026-11-22/23, Skathi `a0a25228`, `release,lean`, JIT off by
environment, our libmpv; Windows 2026-11-23, Skathi `04e24dee`, JIT, the
port's D3D11 libmpv, mpv 0.42-dev, RTX 5080). Every launch muted, cold
storyboard, 3 launches a cell interleaved, medians; window sizes read back
in every launch. The sets, applied on top of the port's own options:

| Set | Options |
|---|---|
| P0 | the port today: mpv's defaults (`scale` lanczos, `dscale` hermite, sigmoid upscaling, `correct-downscaling`), `hwdec-threads` 4, `hdr-compute-peak=no` from the host |
| **P1** | `profile=fast`, `dither=fruit`, `linear-downscaling=yes`, `correct-downscaling=yes`, `hwdec-threads=1`, with `hdr-compute-peak=no` kept |
| P2 | P1 without `hwdec-threads=1` |
| P3 / P4 | P1 + `scale=bicubic` / P1 + `sigmoid-upscaling=yes` |
| P5 | P0 + `libplacebo-opts=force_low_bit_depth_fbos=yes` |
| P6, P0 + upscaler | P1 or P0 with the ArtCNN shader appended 0.5 s after the first frame, as the player turns it on |

P1 is the bare-mpv candidate called f2 elsewhere in our notes. The cells:
1080p SDR, 2160p SDR, 2160p PQ/HDR10 and 1080p with ASS at screen size; a
small window; native or real fullscreen; and **the mini player at its real
size** (420×236 pt on macOS, 630×354 px on Windows, entered by the app's
own resize or key), where a 4K picture is downscaled about 4.6× and 6×.

**P0 → P1, memory and GPU time (measured).** macOS, all processes'
`footprint` at 12 s, MiB, and app GPU time:

| Cell | `<sk-surface>` | `<sk-layer>` |
|---|---|---|
| 1080p, screen-sized | 862 → 787 (−75), 13.6 → 9.7 % | 872 → 900 (+28 ¹), 8.8 → 4.4 % |
| 2160p SDR | 1431 → 1356 (−75) | 1377 → 1273 (−104) |
| 2160p PQ | 1935 → 1798 (**−137**), 17.9 → 14.5 % | 1718 → 1584 (**−134**), 12.2 → 10.3 % |
| ASS on screen | 973 → 903 (−70) | 932 → 887 (−45) |
| Small window, 1080p | 684 → 657 (−27) | 688 → 659 (−29) |
| Fullscreen 1080p / PQ | −33 / **−206** | +6 ¹ / **−132** |
| Mini player 2160p SDR / PQ | −102 / **−178** | −99 / **−135** |

¹ Our layer's own drawable pool grew to 3–4 under P1 (2 under P0); the
cap of two since then removes it, and P1 then costs less than P0 in every
run (see "On Skathi's own layer"). P2–P4, which do not touch 1080p memory,
landed 57–81 MiB under P0 in that cell.

Windows, the one process, MiB of private bytes / GPU dedicated, and GPU
time:

| Cell | `<sk-surface>` | `sklayer` |
|---|---|---|
| 1080p, screen-sized | −24 / −17, 3.58 → 3.48 % | **−96 / −65**, 2.52 → 2.00 % |
| 2160p SDR | −89 / −64 | −82 / −64 |
| 2160p HDR10 | −93 / **−112** | −71 / **−112** |
| ASS on screen | −24 / −16 | −86 / −65 |
| Small window, 1080p | −4 / +1 | −23 / −16 |
| Real fullscreen 1080p / HDR10 | −27 / −17 and −80 / −112 | −98 / −65 and −81 / −112 |
| Mini player 2160p SDR / HDR10 | −86 / −64 and −96 / −112 | −70 / −64 and −95 / −112 |

- **Playback is untouched where it was read**: 24 distinct frames a
  second in every cell on macOS and in every Windows cell with a readable
  frame code (the small window letterboxes the code, so there only mpv's
  counters were read), no drop after start-up on macOS, none at all on
  Windows (108 launches in batch A), `vo-delayed` 0–4 under both sets. GPU time falls by 0.4–6.3
  points on macOS and 0.10–0.53 on Windows, most where mpv itself scales.
- **`hwdec-threads=1` carries most of the HDR saving** (P2 against P1):
  macOS −114 to −121 MiB at PQ (malloc and the VideoToolbox decoder
  service), 26 at 1080p, 82 in the mini player at 4K; Windows 96 MiB of
  GPU memory with HDR10 on both routes, 48 at 4K SDR, 12 at 1080p. Its
  cost, measured on Windows outside the port (libmpv alone, `vo=null`,
  untimed, `d3d11va-copy`, 3 interleaved rounds): 4K HEVC Main10 decodes at
  **450 instead of 512 frames a second (−12 %)**, still about 19× real
  time, with the same 96 MiB less GPU memory (721 → 625); 4K H.264 and the
  software decoder do not change. On macOS decode cadence was unchanged in
  every cell; throughput was not measured there.
- **On `<sk-surface>` mpv never upscales** (from source and measured on
  both systems): the route renders the picture at about the video's own
  pixel count and Skathi stretches it, so `scale`, `sigmoid-upscaling` and
  the upscaler do nothing there. On Windows P3 equals P1 to the megabyte;
  on both systems the upscaler is in mpv's list and changes neither memory
  nor the picture. **If the port keeps that route, it should render at the
  element's size while an upscaler is chosen, or not offer one.**
- **P3** costs −1 to +42 MiB of total memory on macOS (+16 of it graphics
  on `<sk-surface>` at 1080p) and +17 of GPU memory on Windows (`sklayer`) for
  an upscale the zone plate cannot tell from P1; **P4** is free. **P5** saves
  24–137 MiB but bands (8-bit intermediates lose the dither: on Windows the
  dark half of a 10-bit ramp keeps 8 distinct values of 128 on
  `<sk-surface>`; on macOS the backward steps fall from 400–700 to 0–40):
  **rejected on both systems**.
- **`swapchain-depth` does not apply to the port** (from source): the render
  API has no swap chain of mpv's; only `vo_gpu`, `vo_gpu_next` and a few
  others read it.

**Picture (measured; 8-bit window captures on macOS, half-float desktop
captures on Windows, against an ffmpeg Lanczos reference of the same
frame; SSIM on Y):**

| Picture | macOS P0 / P1 / P1 without `correct-downscaling` | Windows P0 / P1 / without |
|---|---|---|
| 4K zone plate into the mini player | 0.509 / **0.546** / 0.073 (both routes) | 0.934 / **0.862** / 0.082 (`sklayer`; `<sk-surface>` alike) |
| 4K into the small window | 0.533 / 0.482 / 0.239 | 0.934 / 0.844 / 0.102 |
| 1080p into the screen (upscale) | 0.588 / 0.609 (`<sk-layer>`); within 0.008 on `<sk-surface>` | 0.997 / **0.945** (`sklayer`); 0.962 for all on `<sk-surface>` |

- **The anti-aliased downscale matters, and P1 keeps it.** Without
  `correct-downscaling` the 4K zone plate in the mini player is moiré over
  the whole picture on both systems.
- **But the systems disagree on P0 against P1 in the mini player.** On
  macOS P1 scored above P0 and looked alike to the eye; on Windows P1 is
  below P0 and shows faint ghost rings where P0 has none: bilinear
  `dscale` from `profile=fast` against the port's hermite. Both ports run
  mpv's default `dscale`, hermite (read back on Windows; the macOS crops'
  notes say the same), so why the ranking flips is not checked; the two
  harnesses differ in capture, size and reference, so their absolute
  values are not comparable. **Whether the rings show on real content is
  not checked.**
- **Upscaling on the layer route costs sharpness on Windows** (0.997 →
  0.945, bilinear against lanczos); on macOS the zone plate did not show
  it. On `<sk-surface>` the set changes nothing, as above.
- **Chroma** (Windows, read and measured): `profile=fast` makes the chroma
  scaler bilinear too, since an unset `cscale` follows `scale`. P0's
  lanczos chroma overshoots in the unclipped HDR10 target (peak 2.84 ×
  SDR white on a chroma zone plate against 1.87 under P1); SSIM 0.958 and
  0.974 between the two.
- **HDR**, in the HDR test captures: the window peak under P0 and P1 on
  each route is macOS 3.293 against 3.291–3.293 on `<sk-layer>` and 2.191
  for both on `<sk-surface>`; Windows 5.135 / 5.138 and 4.161 / 4.161 × SDR
  white. **Banding**, on the synthetic ramps captured: P1 keeps P0's dither
  (the same backward-step counts) and adds no merged levels, except on
  macOS's dark 8-bit ramp (codes 16–48), where the longest flat run is
  196 px under P1 against P0's 108 on `<sk-layer>` (196 under both on
  `<sk-surface>`): a one-level difference in an 8-bit capture, not seen in
  the real dark-sky crops. Real content on Windows was not captured.
- **Real content** (macOS): four of the owner's clips (anime 1080p, a
  1080p series, the same series in 4K SDR and HDR10), three moments each,
  both routes, screen-sized and mini player, P0 / P1 / P3 (144 captures),
  plus P1 with the anime upscaler for the anime clip (12): 156 in all. The
  captures are 8-bit SDR, so the HDR10 crops show EDR highlights clipped.
  P0 against P1 is 40–62 dB PSNR, the largest differences in the mini
  player at a low-light crowd and in 1080p foliage upscaled. Side by side
  at 1:1 no difference could be pointed to (looked at, not the owner's
  verdict). **The owner's judgement is pending.** Windows: not run (the
  clips are not on that machine).

**Behaviour under P1** (measured, both systems, both routes): exact seeks,
frame steps forward and back (the software hand-over on a step back
included), screenshots with and without subtitles, ASS on screen, the
upscaler turned on and off live, pause: as under P0, no drop. A
fullscreen entered while paused gives the same picture on Windows as one
entered before the pause (SSIM 1.000, both routes); on macOS the
`<sk-surface>` route kept a stale, stretched picture, a port defect fixed
since (see "The window's frame source").

**Switching live** (measured; `scale`, `dscale`, `correct-downscaling` and
`sigmoid-upscaling` changed 10 times each during playback, against the
same launch without changes): no drop and no skipped frame, but **one
visible hold in the mini player**: 217 ms in one of four macOS runs, 117
and 131 ms in two of four Windows runs. On macOS memory grew as in the
control (+4 to +10 MiB against +2 to +9); on Windows the changes kept
+16 to +92 MiB of GPU memory to the end of the launch, whether it grows
with more changes was not checked. A policy that switches sets therefore
belongs at moments that already interrupt the picture (pause, seek,
entering or leaving fullscreen or the mini player), never mid-playback.

**The upscaler** (measured, 1080p screen-sized, the layer route):
- It brings the full chain back: macOS +544 MiB over P0 (+508 over P1),
  GPU time 23 %; Windows +419 MiB of GPU memory over P0 (906 against 487)
  and +464 over P1, GPU time 2.52 → 6.59 %. Under it P1 saves 36 MiB on
  macOS and 20 on Windows: nothing worth tuning.
- **Removing it does not give the memory back.** Windows: cleared at
  14 s, GPU memory went 906 → 870 and stayed there to the end, **36 MiB
  back and 383 kept**, in both runs; the same in the behaviour launch
  (424 before, 814 after clearing). macOS: a first attempt that set the
  shader at start-up, which the frontend clears on file load, still held
  +500 to +630 MiB at 12 s on both routes after it had drawn only the
  first frames. That libplacebo keeps what a removed hook allocated until
  the video output is rebuilt is inferred.

**The shipping player under the same set** (measured, an isolated copy,
f2 given as `mpv.conf` lines, which win over the player's defaults; the
installed player's data checksummed before and after):

| | 1080p | 2160p PQ / HDR10 | GPU time |
|---|---|---|---|
| macOS 1.48.1, main process and decoder | 655 → 569 (**−86**) | 1399 → 1072 (**−327**) | 9.3 → 3.3 % and 13.7 → 6.0 % |
| Windows 1.49.1, private bytes, all 13 processes | 827 → 728 (**−99**) | 1243 → 1049 (**−194**) | 2.30 → 1.85 % and 6.54 → 5.67 % |
| Windows 1.49.1, GPU dedicated | 491 → 425 (−66) | 1154 → 935 (**−219**) | |
| Windows, f2 + `swapchain-depth=1` | GPU 394 (−31 more) | GPU 903 (−32 more) | unchanged |

- The shipping player saves more than the port at HDR on macOS (graphics
  526 → 322 MiB, measured). That this is because it runs mpv's whole chain
  at screen size with peak detection, which f2 removes, is **inferred**: the
  six options were changed together, and peak detection was not read back
  on macOS. No drops, 24 frames a second in every
  launch. On macOS the WebKit helpers were not attributed in these runs
  (about 100 MiB in the matrix); they do not read mpv's options (inferred).
- `swapchain-depth=1` is meaningful there, unlike in the port: the player
  presents through mpv's own swap chain under `--wid`. Cadence unchanged
  (gap p99 45.8–46.3 ms against 45.4–46.3).
- On Windows `hdr-compute-peak=auto`, one of f2's lines, read back `false`:
  the player applies `profile=fast` after it (inferred from the read-back),
  so f2 ran there without peak detection. On macOS it was not read back.
- The Windows control found a defect of the shipping player: a portable
  copy's hidden `veil` window has no data directory and writes into the
  installed player's web view profile (76 housekeeping files changed, no
  Local Storage data file). Filed as Frame Player issue #81.
- The shipping player's `dscale` is hermite too (read back on Windows), so
  the mini-player caveat above applies to it.

**Recommendation (not yet decided by the owner).**
- **P1 as the port's default, as one fixed set**, with no per-size or
  per-content policy: it saves the most in the cells that cost the most
  (PQ/HDR10 and the mini player), keeps the anti-aliased downscale, and a
  switching policy costs a visible hold.
- **Except `dscale`**: keep the port's hermite, or set one explicitly,
  rather than taking bilinear from `profile=fast`, because of the Windows
  mini player. A P1 with hermite `dscale` is **not measured**: neither its
  memory nor its picture.
- **Next to `mpv.conf`**: apply the set under the viewer's lines, skipping
  any key `isUserConfKey` reports (as `picture-adjust.svelte.ts` does);
  send the resolved keys one by one, not `profile=fast`, which would
  override a viewer's `scale=` or `dither=` depending on order; and send it
  apart from the viewer's lines, because one refused line makes the player
  drop all of them.
- A "Picture quality: Efficient / Best" choice was proposed for the
  settings, Best adding `scale=bicubic` and sigmoid; not decided, no
  strings written.
- The upscaler: switching it off should rebuild the video output once the
  kept memory is confirmed in the player itself.
- For version 1 the same set is the larger win; it is listed in
  [v1-backports.md](v1-backports.md), not scheduled.

Not checked: the owner's verdict on the crops; real content on Windows;
peak detection in the port (kept off); a P1 with hermite `dscale`; AMD and
Intel GPUs; energy; AOT builds; whether the memory held after live
switching grows.

### Skathi defects found, and their state

Filed in github.com/argon-chat/skathi while building the prototypes. #1–#20
are closed, most of them within hours. #21–#34 are closed (#25 and #30 as
not planned, the others fixed and, from #29 on, confirmed by us on master).
#35–#38, answered on 2026-11-20 with fixes on master after Skathi 0.6.0,
were re-measured by us on master `64d2e42f`; #35, #36 and #38 are closed,
and #37, reopened by us for AOT on macOS, was closed by the maintainer on
2026-11-22. #39–#42 were answered within hours and are closed. #43, the
request for a supported native layer under the page, was filed on
2026-11-22; the maintainer confirmed its source readings and split it into
#44–#49, with one item answered as by design. All of #43–#49 are closed:
#45–#48 on 2026-11-22, #44 and #49 reopened after our measurements and
closed again on 2026-11-23 with `329afd60`. Skathi 0.7.0 has been released
since (`cde894f5`). The native-layer prototype ran on macOS on master
`dd8b4ccc` (the release plus a build fix) and on Windows on the port's pin
`2b08fd37` (crates 0.7.0, before the release); `<sk-layer>` was measured on
macOS on `a0a25228` and `ce6aaca2`, on Windows on `04e24dee`. One defect
went upstream as a pull request of ours to `skathi-gpui` instead of an
issue (the last item below).
"Re-measured" means our own runs confirmed the fix, on the build named; "landed" names the
upstream commit, which is not always the build we tested.

- **#1** `<sk-surface>` was inert without the cargo feature `video`. Fixed (`ae8dca2c`), re-measured on `1eacd896`.
- **#2** `sk types --exe` refused an optimised executable. Fixed (`d9f84387`).
- **#3** All surfaces shared one process-wide copy queue, so a fence that never arrived froze every surface. Fixed by the maintainer (`43da7b21`, per-surface queue and release fence, with their own regression test); not re-measured by us (the prototypes have one surface).
- **#4** The documentation said half-float frames were clipped to 8 bits. The documentation was fixed (`711649e2`). A separate follow-up (`aad939ae`) sets `preferredDynamicRange`/`contentsHeadroom` and decides the HDR switch by the screen's potential headroom, so EDR can start on macOS without a primer; re-measured on `1eacd896`, macOS 26.5.2.
- **#5** Svelte threw `customElements is not defined` on `<sk-surface>`. Fixed; the shim is gone from `1eacd896`.
- **#6** Nothing said whether a window draws HDR. `output_hdr` was added; on macOS it equalled "the layer is `RGBA16Float`" in 102 063 of 102 063 probes over 48 runs (MM).
- **#7** A host built with devtools opened the devtools port although the payload was published with `publishDevtools: false`. Fixed (`bc43e91e`): such a host now starts with devtools off; a build with the `no-devtools` feature contains none. Re-measured on macOS (MM).
- **#8** The macOS EDR layer was tagged with the display's colour space. Fixed (`8a1f1728`), re-measured 3 of 3 (MM): the layer is now tagged extended sRGB.
- **#9** A surface showed nothing while the newest frame was in flight. Fixed (`c68618ad`). Before the fix, at exactly one present per refresh, the loss on Windows depended on the phase (61 % of frames just after the vblank, 8 % at 0.3–0.9 of the refresh). The later "one frame in fifteen" at one present per refresh on Windows is the driver's 224 cap (#26), not Skathi. The recipe's at most one present per refresh, started by the formula above, comes from the pacing work of #15.
- **#10** The documented mpv recipe (`report_swap` from `on_frame`) could not work. The page was rewritten along the recipe above.
- **#11** The macOS EDR switch produced a glitch frame and a flood of `DisplayChanged`. Fixed, re-measured (MM): no glitch frame captured in 21 switches, 10 of them with full capture coverage near the switch.
- **#12** A ring of 4 or more gave permanent `Busy` in an HDR window on D3D12. Fixed, re-measured on `1eacd896`.
- **#13** `last_vblank` was the coming vblank and went stale. Fixed (`0a158b12`, then `9f24eb0c`), re-measured on `74e63fad`: p95 14.5–19.0 µs off the grid over 5 runs. `9f24eb0c` also carries the last known vblank forward after a refresh-rate change; we did not re-test a rate change on the current build. Before any vblank is known the value can be `None`; the producer then waits one `refresh_period`.
- **#14** Covered, minimised or display-off windows reported frames as shown. Fixed (`2706f963`, `e373486f`). `Hidden` starting 30–169 ms late under a cover that does not take the foreground is #25.
- **#15** At 24 fps frames came a refresh late and were labelled `Estimated`. The vblank clock (`9f24eb0c`) and the recipe changes (docs `74e63fad`) fixed the drift. Our patch landed upstream as the opt-in `exact_frame_feedback(true)` (`810843bd`, documented in `02dfa33d`) and was re-measured on `c7a524d5` above, on one display. It is off by default because it costs about one refresh of latency.
- **#16** A removed D3D12 device was never replaced or reported. Fixed (`3f948c3e`), re-measured on `6cd67304` with 24 injected `RemoveDevice` removals and the producer's own D3D11 device intact: `SurfaceEvent::DeviceLost` came 16.6–33.6 ms after the removal and again 471–506 ms after it, when the replacement device was ready; video was back 690–708 ms after the removal. On this machine the old shared fences read `UINT64_MAX` for good and gave `Busy` when reused, so the producer makes new fences after a device loss; the maintainer's machine kept the last value, and Skathi's documentation (`02dfa33d`) now says the fence breaks "on some systems". During the ~0.7 s the window shows the desktop behind it. A real driver reset was not tested.
- **#17** A refused GPU frame's reason was promised in the log but not printed. Fixed (`71f66833`): each surface logs a warning once per reason. Verified in source only: in our device-loss runs presents were refused before paint, so the line never appeared.
- **#18** On macOS the HDR clamp climbed the EDR ramp in 200 ms steps. Fixed (`12f7f88e`) for the clamp Skathi paints with, re-measured on `c602b5a4`. `content_headroom()` and `DisplayChanged` still move in coalesced steps, so a producer that tone-maps to it still steps. The ~110 ms hold when EDR starts from 1.0 is macOS's: a native app without Skathi shows it too (M53).
- **#19** On macOS, frames shown at sparse presents or during a resize were reported `Superseded` or late. Fixed through `skathi-gpui` 0.5.2 (`c602b5a4`, `810843bd`, `77030d59`, `f2a79987`), re-measured on each in the scripted cases, with the residues listed above.
- **#20** Closing a window logged `[ERROR]` lines. Fixed (`skathi-gpui` `186ed842`). Re-measured: no line after `Handle::close()` on 0.5.0. A posted `WM_CLOSE` there still logged one `Invalid window handle` line, down from 5–6. The remaining `WM_CLOSE` line depends on focus: it appears when the window is the foreground window as `WM_CLOSE` arrives (15 of 15) and never otherwise (0 of 43), on `8c912edc` and on `c29ab351` alike; the earlier "0 of 10" runs never had the foreground (inferred from their shutdown times). A real click on the close button closes a foreground window, so expect the line in real use. Reported in #20 with the source reading (a deferred activation task running after the window is destroyed, not traced). Side finding: with `exact_frame_feedback` off, a foreground window's 60 fps frames come back mostly `Estimated` and a background one's all `Exact`, so earlier "option off" numbers, which did not record focus, are incomplete. `Handle::close()` stays clean. A click on the close button was never measured.
- **#21, #22** `sk check` missed CSS the runtime drops or flags. Fixed for 0.6. In `6db5fc22` the runtime learnt `transition` with `var()`, state-made `::before`/`::after`, `min()`/`max()`/`clamp()` over percentages, stops from CSS, and `position: relative` paint order. In `cdde8ac6` `sk check` began running the host's own CSS code compiled to WebAssembly, so it reports what the runtime reports. What the renderer still lacks is a warning (`SK4006`), and what needs a 0.6 host is marked `SK4007`.
- **#23** Inline `<svg>` lost CSS shape geometry and `var()` in stops. Fixed (`6db5fc22`), confirmed on macOS: 0 of 23 808 pixels differ from the attribute control.
- **#24** A present accepted after the app's quit hook got no event and still returned `Ok`. Before the fix: 2 frames in each of 6 closes on macOS, and 1–3 frames in 17 of 20 on Windows. Fixed (`040bfb3c`): a present after quit now returns `SurfaceError::Closed`. Re-measured on master `c29ab351`, Windows: 0 frames without an event in 20 of 20 closes.
- **#25** (not planned) On Windows, `Hidden` is up to 200 ms late when a window that does not take the foreground covers ours. Occlusion is cached for 200 ms, and closing the gap would need system-wide window hooks; the maintainer keeps the bound by design. Every other way of being covered is detected at once.
- **#26** (closed, ours) One frame in fifteen lost at one present per refresh on Windows. It is the driver's 224 cap on our machine; see "On Skathi 0.5.0". The maintainer found and fixed a separate cause, a frame latched after the UI callbacks (`20733eea`), and added `Superseded` logging.
- **#27** A git dependency did not build without the devtools bundle. Fixed (`39ed898d`): a Cargo git checkout without the bundle now builds without devtools and prints a `cargo:warning`.
- **#28** `sk check` missed `-webkit-mask-*` and `vertical-align` with a length. Fixed by `cdde8ac6`, confirmed on `c29ab351`.
- **#29** A custom property changed by `:hover` did not reach `var()` in other rules, and nothing was logged. Fixed (`a6e9289d`): it reaches the element, its descendants and inline SVG stops. Confirmed on `03fb7ecf`, in the issue's example and in the ported player's catalog ring; closed.
- **#30** (not planned) `sk check` does not see a refused value that arrives through `var()`. This is by design: it checks declarations as written, and such values show up only in the runtime log.
- **#31** `KeyboardEvent.code` was always empty and `repeat` always false, so shortcuts on physical keys could not be ported. Fixed (`2081d5c7`, consecutive X11 keyups `feb5b3ad`): `code`, `key`, `location` and `repeat`; `keyCode` and `charCode` stay unavailable. Confirmed on `03fb7ecf` in the ported player (J/K/L, arrows with repeat, `,`/`.`, Shift+L, digits, the keypad, a Russian layout; typing in a field fires no shortcut); closed.
- **#32** (open) On the tested 120 Hz XDR display, the window's `CAMetalDisplayLink` targets each frame 5 refreshes ahead. Answered with opt-in timer pacing, `lowLatencySurface` (`227dfd1d`); see "Latency" above. The first version (`a4469450`) took present to screen 33.4 → 16.3 ms but broke playback cadence (57–63 %); the hybrid that followed (`f531dd11`, `7a9f653a`) gives 9.3 ms paused with cadence and animation intact (see "Latency"). Confirmed on `b682a9e3`; closed.
- **#33** Each half-float slot kept an unused 8-bit twin. On D3D12 on master (`886ebf94`) the twin is made on a slot's first SDR read and kept until it retires; the maintainer counted 63.75 MiB for the two 4K twins in an SDR test window on an RTX 5070, and checked HDR separately by readback. The slot-to-atlas copy stays by design. Re-measured on `8c912edc`: 62.0 MiB less in an HDR window with 4K frames, one twin (+32 MiB) left after an SDR → HDR switch. Closed by us.
- **#34** `<sk-surface>` upscaled with one bilinear tap. On master (`684e7036`) `image-rendering` is honoured, and `auto` uses Catmull–Rom on enlarged axes. Re-measured on `8c912edc` (31.4 dB against 27.8 on fine detail, no HDR halo); closed by us.
- **#35** (closed, filed for the interface port) The embedder had no handle to the main window and no run-time minimum size. Implemented on master in `6a9338be`, documented for 0.6: `appWindow` run-time min/max size, `setAspectRatio`, `setTrafficLightPosition`, fades on show and hide, an animated `setBounds()` and `visibleOnAllWorkspaces`; `HostContext` and `Handle` expose an unsafe raw handle (main thread only; on macOS the content `NSView`). Checked in the ported player on `03fb7ecf`: min/max size, aspect ratio, traffic-light position, fades and the animated `setBounds()` work, read back from the `NSWindow`; the mini player did not stay above another app's fullscreen Space. The follow-up (`b5fe7552`, `2e88b48b`, `f531dd11`) added `setWindowButtonsVisible`/`getWindowButtonsRect`, `setFullscreen(…, { transition: "fade" })`, pinch as a cancelable `wheel` with `ctrlKey` (Option kept in `altKey`) and a panel swap for the mini player. On `b682a9e3`, with synthetic events (no physical trackpad, no real clicks): the black fullscreen transition works both ways, pinch and Option+pinch split as documented, the button fade and the re-centring after a title-bar height change work. Broken there: `getWindowButtonsRect()` returned the whole content area; showing the buttons after hiding them dropped the centring; the mini player left the screen over a foreign fullscreen Space, because no panel was created (inferred from source: the swap compared the window's class exactly, and AppKit on macOS 26.5.2 turns it into a KVO subclass). Re-measured on v0.6.0 (`ca6e4e08`, which carries the fixes of `b4d4e190` and the KVO fix; 3 runs, unlocked screen): the rectangle is right, `[12, 17, 60, 14]`, the union of AppKit's three button frames, and `null` while hidden; an instant hide and show keeps the centring and an explicit position; **a hide or show with a 150 ms fade still drops either to AppKit's (9, 9)**, which the player hits on its first idle hide; the mini player now becomes a panel that stays on a foreign fullscreen Space and takes an in-process click and key while the other app keeps its menu bar (one run per flag; Cmd+Tab not tried). Reported. The maintainer answered with `a5526ce7` (and a gpui change): the button frames are reapplied after each opacity or visibility change. Re-measured on `64d2e42f` (3 launches, plus 2 of the player's own idle fade): no button moves through either fade, centred or at an explicit position (see "On screen on master `64d2e42f`"). Closed as fixed.
- **#36** (closed) In a Windows HDR window a half-float `<sk-surface>` was clamped to 0 … headroom, so colours outside BT.709 (negative scRGB components) never reached the screen; found by the 33-patch colour comparison above. Fixed in `9c7f52b6` (with a gpui change): D3D11, D3D12, wgpu and Metal keep the sign of extended-sRGB channels through the HDR path; SDR output still clips negatives. Re-measured on `64d2e42f` on the RTX 5080: the red patch's negative green and blue reach the desktop, the desktop equals our texture on all 33 patches, and 27 of 33 match on screen (was 26); see "gpu-next through mpv PR #18845 on D3D11". Checked through D3D12 in an HDR window only.
- **#37** (closed) Memory for an embedder: an AOT or no-JIT route for our own executable, and the start-up graphics peak (above). Answers: on Windows `dc32c71e` exposes the AOT features to embedders (`sk publish --aot --payload-only --workload <exe>`; documented as "since 0.7"), and `bcd1f8b5` makes production and AOT bundles ASCII by default; `no-interpreter` is experimental and not for apps. At the time macOS had no AOT (planned for 0.7), and `JSC_useJIT=false` and `JSC_useWebAssembly=false` in the environment were supported to rely on meanwhile. The ASCII bundle is confirmed by us on `89c511c1` (built and compared, not run in the host); the Windows AOT route on `64d2e42f` (see "The JIT against AOT in our own executable", and #39). **The peak is the system's**: on `64d2e42f` the empty payload peaks at 228–232 MiB, 24 × 8.00 MiB of graphics memory for about a second, and a 26-line AppKit program without Skathi reproduces it with three presents to a `CAMetalLayer` (see "On screen on master `64d2e42f`"). That it depends on the display is inferred, not checked. Skathi's embedding documentation notes it (`ec3cc308`). Reopened by us for macOS AOT, which has since landed on master for macOS arm64 (`cebac4f9`): measured on `d153b34e` (see "Every memory lever, and AOT without the JIT"), the player's idle footprint settles at 71.1 MiB against 89.7 with the JIT, inputs reach the DOM about twice as fast, and the empty payload's peak is unchanged. Of the snags reported with it, `--payload-only` now signs the image on a Mac (`025ff87c`); the maintainer could not run that on a Mac, and we have not verified it yet. The maintainer closed the issue on 2026-11-22 without a further comment; the signing check stays on our list (Still open).
- **#38** (closed) On macOS a surface's copies were never trimmed and outlived `clear()`, and the window held five drawables, not three (above). `a355114c` trims Metal's copy pool after a burst or a resize, and Metal and D3D12 release a cleared surface's slots after a 50 ms grace. The five drawables are kept by design: the maintainer reproduced them and keeps the display link's pacing. Re-measured on `64d2e42f`: a cleared surface holds no copy (1232 → 1123 MiB, and 1121 → 1019 with our ring dropped), no old-size copy after a resize in 6 of 6, five drawables in all 55 launches, `lowLatencySurface` off and on. The trim after a burst itself was not exercised. Later, in the native-layer prototype, a minimal page held three drawables while idle and five once it repainted every frame: the five are a busy-window effect, kept also while paused (133 MiB at screen size in the port), and #49 answers it: no automatic trimming; first a drawable-count setting, which saved nothing, then the frame source, whose `NextDrawable` keeps three (measured on `ce6aaca2`).
- **#39** (closed) Embedder AOT on Windows: the default memory profile committed 367–378 MB more than the JIT, none of it touched; with four snags on the AOT page (`setup.ps1` under PowerShell 5.1, `pwsh` for `sk-aot setup`, `--sk-host-info` having to be the first argument, the `icu-root` message). Answered: application images use `compact` by default (`86194799`). The maintainer's reading of the engine source (not measured on a live process) puts the 256 MB block in the AOT structure rows, the 86 MB most likely in the allocator's metadata for its reserved range, and the 58.5 MB in a per-VM table that Windows commits whole and no option sizes; the guide now has a section on it (`696f15d2`). Of the snags (`2b08fd37`), the scripts changed: every `.ps1` requires PowerShell 7 and says so under 5.1, and `setup.ps1` unpacks with the system's `tar`; the other three are fixed in the documentation and an error message (`pwsh` for the Windows steps, `--sk-host-info` reaching `Builder::spawn`, the ICU data file travelling with the exe). Our Windows runs since set `compact` explicitly, so the new default itself was not re-measured there; on macOS the `default` profile measured 0.4 MiB above `compact`. See "The JIT against AOT in our own executable".
- **#40** (closed) Assigning a `console` member throws in an AOT image, `sk check` reported nothing, and the window stayed empty; found with the player's console capture on both systems. The read-only `console` stays intentional, as for the other built-ins; the fixes add diagnostics: `sk check` reports the assignment as `SK1001`, including computed keys whose literals are known (`d388f23e`), and an entry that throws before it has mounted anything opens the host's error dialog with the exception, or only logs it under a test harness (`d8e601aa`). The intended form is a guard on `Runtime.isAot()`; the ports use a `try` for now. Not re-measured by us. That `globalThis.console = …` is silently ignored on macOS was not looked at upstream.
- **#41** (closed) Master did not build without the `audio` feature: `mod memory` had taken the `#[cfg]` of `mod microphone`. Fixed (`2bb45650`); we built with a local fix until then.
- **#42** (closed, found in the macOS port's title bar) `position: static` was laid out as `relative`, so a base rule's `left` still moved the box and the title-bar logo sat flush against the window's edge; and `flex-direction: inherit` was dropped. Neither was logged or reported by `sk check`. Fixed (`0277f7b5`): insets apply only to positioned boxes and `getComputedStyle().position` reads `static` back; `inherit` on a property that does not inherit is not resolved but reported, once at run time (`flex-direction: inherit was dropped`) and by `sk check` (`SK4002`), so the port keeps an explicit direction. Not re-measured by us; the port carries a two-line CSS workaround that is harmless in WebKit.
- **#43** (closed, filed 2026-11-22) A supported native layer under the page, from the prototype in "A native video layer under the page", as seven requests in one issue. The maintainer confirmed the source readings behind items 1, 3 and 7 and split the items into #44–#49; item 7 (a page with no framework mount painting nothing on 0.7) is by design: a built bundle's stylesheet is installed by `createApp().mount()` and `mountDocument()`, so a page that draws without a framework calls `mountDocument()` from `@skathi-native/runtime` and appends inside the root it is given. Not tried by us.
- **#44** (closed) No supported native layer under an element: the `<iframe>` backend that places content under the page from the paint pass was a single slot, which is what forced the workarounds. Our comment there: what the player needs is the element's rectangle in the frame the page is laid out in, and a place under the page; no punch, as the page is transparent where the video is. Landed in `04e24dee`: the `<sk-layer>` element and the `Layer` trait (`attach` with the place under the page, `place` with the box in device pixels or `None`). Measured by us on both systems (see "On Skathi's own layer"): it builds and works unchanged; on macOS in the page's frame through a live resize; on Windows one desktop refresh behind while growing; on macOS `attach` was not called when the mini player moved the view into its panel. Reopened by the maintainer for the last two and closed with `beab4bb5` (on master with `329afd60`): `attach` is called again when the page's view is in another window or under another superview, with the window in `LayerHost`, and on Windows Skathi commits the composition device itself after presenting the frame that placed a layer, so a layer must not call `Commit`. The macOS half re-measured by us on `ce6aaca2` (four attaches through the mini player, the picture placed and live); the Windows commit order not yet.
- **#45** (closed) A notification when the Windows composition tree is rebuilt after device loss. Closed as covered by #44: a `<sk-layer>`'s `attach` is called again when the window's composition device or underlay changed, followed by a placement. Measured by us on `04e24dee` with an in-process `RemoveDevice` on Skathi's device: a second `attach` with a new device and underlay, and the picture present at the same brightness in shots 1, 6 and 13 s later (the recovery itself not timed). A real driver reset was not tested.
- **#46** (closed) An empty half-float `<sk-surface>` switched the window to half float, which kept the port's idle start screen in scRGB (299 MiB of GPU memory against 140 on the layer route). Fixed (`9dcec055`): the window follows half-float surfaces that hold a frame, and `clear()` on the last one switches it back. Re-measured by us on `9dcec055`: the idle window stays 8-bit in 6 of 6 launches, at 140 MiB of GPU memory screen-sized and in fullscreen (see "Windows: Frame Player 1.49.1 against the port").
- **#47** (closed) The display's headroom per window for an embedder. Landed in `0727891d`: `Builder::on_display_change` with the window's `SurfaceDisplay` (`hdr`, `headroom`, `potential_headroom`, `sdr_white_nits`, `peak_nits`, `full_frame_nits`) when first known and on each change, and `Handle::display()`. Measured by us on both systems as the layer's only source of its target; a change while running was not tested.
- **#48** (closed) A backdrop the page cannot change. Landed in `a03fc0a3`: `Builder::backdrop("transparent")`, the page's setter ignored with one advisory line; measured by us on both systems. The DirectComposition query moved into #44 as the layer's own answer.
- **#49** (closed) macOS drawables: no automatic trimming of the pool after a busy spell (the five of #38, 133 MiB at screen size). First answered with `metal_drawable_count` (`a0a25228`); measured by us: the layer reads back the count, five drawables stay resident, 0 MiB saved. Reopened by the maintainer, who found the count decided by where drawables come from (three with `nextDrawable` only, four with `CAMetalDisplayLink` only, four or five when switching between them, as the default does), and closed with `329afd60`: `metal_drawable_count` removed, `Builder::metal_frame_source(Auto | DisplayLink | NextDrawable)` added. Measured by us on `ce6aaca2`: `NextDrawable` keeps three, −50 to −104 MiB at 2160p, same cadence (see "The window's frame source"). Not yet reported back on the issue.
- **skathi-gpui pull request #1** (ours, merged 2026-11-23 as `fccd451b`) An inactive window's traffic lights turned light grey for the whole of a `setWindowButtonsVisible` fade and snapped back at its end, on macOS 26: group opacity over the buttons' plus-darker layers. The fix sets `allowsGroupOpacity = NO` on each button's layer. Skathi master pins it from `75e7f11b`; re-measured on `ce6aaca2`: no grey frame in either window state (see "The traffic lights of an inactive window").

## The shape of the design

```
FFmpeg decode → mpv render API → a texture we own, shared with Skathi
             → Skathi surface: GPU wait, copy, map, draw into its window

Windows, now:   mpv's D3D11 render API (old vo_gpu renderer) → shared D3D11 texture → Skathi (D3D12)
Both, later:    mpv gpu-next (libplacebo) on Vulkan → IOSurface / D3D12 resource → Skathi
```

Superseded in part by "Decided for version 2". The current matrix: Windows,
D3D11 render API with gpu-next through PR #18845; macOS, the GL render API
(route A) as the interim, with gpu-next through the PR (stock 0.41 renders it
with `gpu`), and a downstream Vulkan render API as the target. The PR supplies
renderer selection and gpu-next on the D3D11 and GL render APIs; the Vulkan API
integration stays downstream.

- **mpv stays.** libplacebo only turns a decoded frame into pixels; mpv is the
  player around it — A/V sync, audio output, exact seeking and stepping, libass
  subtitles, the demuxer cache that torrent streaming leans on, `mpv.conf`,
  scripts, the property/event API the whole interface is built on. A player of
  our own on FFmpeg + libplacebo (libplacebo's `plplay` demo is the starting
  point people quote) would have to rewrite all of that. gpu-next already *is*
  libplacebo, so if Skathi uses libplacebo for its own HDR output later, the
  colour maths on both sides of the hand-off is the same library.
- **Windows can start today, with no mpv patch.** mpv v0.42.0 (released
  2026-11-05) ships a D3D11 render API backend (PR #17764, merged 2026-10-15,
  client API 2.6), and our bundled build (commit `a39482e7a`, 124 commits after
  the merge) already has it. It drives the **old `vo_gpu` renderer**, not
  gpu-next, but it renders into an `R16G16B16A16_FLOAT` texture we own, keeps
  `d3d11va` zero-copy decoding, can put HDR highlights above 1.0, and its
  texture can be shared with Skathi's D3D12 renderer bit-exact. Details in
  "Windows today: the D3D11 render API" below.
- **gpu-next through the render API is the later step, and it needs a patch.**
  For HDR quality (the old renderer tone-maps worse and has no
  `hdr-reference-white`) and for macOS, where the render API offers only OpenGL
  and our libmpv refuses OpenGL. (Superseded: the refusal was `gpu-api=opengl`
  as a window backend; the OpenGL render API itself works with our libmpv,
  stage M2.) libplacebo has D3D11, Vulkan and OpenGL
  backends and no D3D12; Skathi's default Windows renderer is D3D12 and its
  documented Vulkan route is "create on D3D12, import into Vulkan"; on macOS mpv
  already renders through Vulkan (MoltenVK, context `macvk`). One Vulkan render
  backend therefore covers both platforms.
- **If Windows video moves from D3D11 to Vulkan,** measured on the RTX 5080
  with today's libmpv in its own window: identical pictures (window screenshot
  SSIM 1.000000 for SDR, 0.999989 for HDR10; FP16 desktop captures differ by a
  mean of 0.00001), no dropped or delayed frames either way. What changes is the
  hardware decoder: `hwdec=auto-safe` picks `d3d11va` on D3D11 and `nvdec` on
  Vulkan. AMD and Intel GPUs were not tested and must be before shipping.

| 1080p clips, 3 repeats each | SDR, D3D11 | SDR, Vulkan | HDR10, D3D11 | HDR10, Vulkan |
|---|---|---|---|---|
| Decoder | d3d11va | nvdec | d3d11va | nvdec |
| Exact seek | 46–94 ms | 49–91 ms | 46–93 ms, grows with distance | flat ~45 ms |
| Frame step | 33–44 ms | same | same | same |
| Frame back-step | 11–13 ms | 20 ms | ~78 ms | ~78 ms |

### Windows today: the D3D11 render API

Measured with the bundled `libmpv-2.dll` (mpv v0.41.0-1303-ga39482e7a), a
standalone probe, RTX 5080.

- **Constants:** `MPV_RENDER_API_TYPE_D3D11 = "d3d11"`,
  `MPV_RENDER_PARAM_D3D11_INIT_PARAMS = 21` (`mpv_d3d11_init_params { void
  *device }`, at create), `MPV_RENDER_PARAM_D3D11_FBO = 22` (`mpv_d3d11_fbo {
  void *tex }`, per render). Header `include/mpv/render_d3d11.h`. Leaving out
  the init params gives -4 (`MPV_ERROR_INVALID_PARAMETER`) — which an earlier
  probe misread as "backend missing".
- **The device:** create it without `D3D11_CREATE_DEVICE_SINGLETHREADED`; mpv
  turns multithread protection on itself (it was off before
  `mpv_render_context_create`, on after) because `d3d11va` decodes on that
  device from mpv's decoder thread. A single-threaded device logs `Failed to get
  Multithread interface (0x80004002)` and falls back to `d3d11va-copy`. mpv
  renders on the immediate context and does not restore state; nothing else may
  use the immediate context during an `mpv_render_*` call.
- **What it runs:** `current-vo = libmpv`, `hwdec-current = d3d11va` for H.264
  and HEVC Main10 (`nv12` / `p010`, zero-copy), log `[libmpv_render] Using
  Direct3D 11.1 runtime`, `Using FBO format rgba16hf`. No libplacebo renderer;
  `current-gpu-context` and `video-target-params` are unavailable on this path.
  `mpv_render_context_create` took 59.6 ms; the first frame 83–128 ms (shader
  compilation).
- **Target formats:** rgba16f, rgb10a2, bgra8, rgba8 render. **An sRGB format
  is refused on every call** (-18, `Could not find a suitable RA format for
  wrapped resource`) and, since the frame stays pending, a naive update loop
  spins (9692 failed calls in the probe). Check the format once and stop.
- **`MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME` (12) defaults to 1:** the render
  call then sleeps until the frame is due (~41.3 ms per call at 24 fps). Pass 0
  and do the pacing ourselves when the render thread does anything else.
- **Cost per frame** (`block=0`; CPU time of the render call / GPU timestamps;
  0 drops over 15 s):

| Run | CPU µs mean / p95 | GPU µs p50 / p95 / p99 |
|---|---|---|
| SDR 1080p | 283 / 395 | 403 / 526 / 961 |
| SDR, 4K target | 301 / 422 | 586 / 749 / 1086 |
| HDR 1080p, default options | 336 / 463 | 670 / 1081 / 1237 |
| HDR 1080p, linear, peak 846 | 318 / 454 | 646 / 1043 / 1172 |
| HDR 4K, linear, peak 846 | 344 / 499 | 1438 / 1835 / 1982 (1 drop at start-up) |

  Under sustained load the same SDR frame takes 101 µs of GPU; at 24 fps the GPU
  drops into low-power states between frames, which inflates the numbers above.
  GPU timestamp queries need their own `Flush` after the end query, or they read
  one frame period.
- **Not tested on this path:** OSD and subtitles (they go through the same
  renderer and should appear in the texture), Windows HDR off. Imported
  subtitle fonts are mpv options, not window state — `sub-fonts-dir` set before
  init (`session.ts`) and copied to `osd-fonts-dir` by `sublines.lua` for the
  second line — so they should carry over unchanged; the prototype checks a
  family from that folder on both lines.

### What has to change in libmpv (for gpu-next, and for macOS)

The render API has no gpu-next backend upstream yet. Signs that it is coming:
on 2026-11-15 about forty commits split `vo_gpu_next` so the renderer
(`gpu_next/video.c`) no longer owns the window and swapchain. The patch we would
need, on top of the stack `docs/mpv-patches.md` describes:

1. **A gpu-next render backend for Vulkan.** (Built since, offscreen, as a
   +578 −1 patch on the PR; see "The Vulkan render API". The original
   reasoning follows.) The client supplies a `VkImage`
   (plus its device, queue family and a timeline semaphore); libplacebo renders
   into it. The D3D11 backend's API (device at create, target per render) is the
   model to copy. Upstream state as of November 2026: PR #16818 (gpu-next render
   backend) is a draft limited to OpenGL with an sRGB-only target, an empty
   end-of-frame hook (no `report_swap`), and no `set_parameter`/`get_image`;
   maintainers called it a mess and asked for it to be done "like vo_gpu". The
   follow-up RFC #18607 is open, and the maintainer response to the IINA
   developer's proposal — a separate `MPV_RENDER_PARAM_API_TYPE` per backend
   rather than one switch — was "looks sane". A Vulkan API type written that way
   is something upstream may take; check upstream first, the refactor above may
   make it arrive on its own. The OpenGL route is closed for us: our macOS
   libmpv refuses `gpu-api=opengl` and `gpu-context=cocoa` (-7). (Superseded:
   that refusal concerns mpv's own window. The OpenGL render API works with our
   libmpv (stage M2), and PR #18845 now covers gpu-next on it and on D3D11.)
2. **`report_swap` with a timestamp.** (Superseded as a requirement: both
   prototypes paced mpv without it, calling `report_swap` once per render-loop
   turn on the render thread, see "The pacing recipe that won". A timestamped
   report stays optional future work and would need its own timing contract.
   The original reasoning follows.) `mpv_render_context_report_swap()` takes no time: mpv assumes the
   frame was shown at the moment of the call. Skathi reports presentation after
   the fact (below), so the patch must accept `shown_at` and the refresh
   interval, and know about dropped frames, or display-resample timing will be
   wrong.
3. **A Windows build of libmpv of our own** for anything patched. Today Windows
   uses a prebuilt libmpv and only macOS applies patches. mpv now publishes
   official libmpv builds for Windows (x86_64 and arm64, LGPL and GPL) from its
   CI; that workflow is the starting point for ours.

Size estimate for (1)+(2): on the order of PR #16818 itself, 1–2 thousand lines.
(Historical: that estimate assumed the renderer work PR #18845 now supplies.
The Vulkan backend on top of the PR came to +578 −1 in mpv, inside the
350–600 estimated; the 250–450 app-side estimate is unverified. See "The
Vulkan render API".)

## Skathi's frame contract

From Skathi 0.4.0 (released 2026-11-18: `skathi`, `sk-tree`, `sk-app` 0.4.0 on
crates.io, `skathi-gpui` 0.4.1; npm `@skathi-native/runtime` `^0.4.0`), its
documentation (`/embedding/native-frames/`, `/embedding/gpu-frames/`,
`/embedding/gpu-frames-macos/`, `/web-platform/sk-surface/`) and its source at
commit `aa232d0`. The first measurements were against `8ab3a3d`; what 0.4
changed is marked. MIT OR Apache-2.0. Paths below are in that repository.
`crates/sk-app/examples/surface.rs` is the complete producer example.

- **API.** `skathi::surface(name)` → `SurfaceHandle` (`Send + Sync`).
  `surface.set_format(SurfaceFormat::Rgba16Float)` once, before presenting.
  `SharedTexture::open(handle)` / `from_iosurface(IOSurfaceRef)`, then
  `.with_fence(handle)` (Windows) or `.with_shared_event(event)` (macOS), and
  `present_shared_fenced(&shared, wait, release)`. `info()` gives the element's
  size in physical pixels, `visible`, `display` and `dynamic_range_limit`, and
  `info.content_headroom()` is the peak to tone-map to; `on_change` returns a
  `SurfaceWatch` and reports `SurfaceEvent::Changed(info)`, `DeviceLost` and
  `DisplayChanged(display)`. Every present returns the frame id (`u64`);
  `present_d3d12` and `present_metal_texture` return `FramePresent { frame,
  release }`. `skathi::monotonic_ns()` is the clock of every timestamp.
- **Skathi's own D3D11 device (0.4 docs) — not usable for us.**
  `d3d11_device()` + `present_d3d11_texture(tex)` is the docs' "fewest moving
  parts" path and their mpv example uses it: render on Skathi's device, the copy
  is queued before the call returns, no shared texture, no fence. Under the
  D3D12 renderer that device is a **D3D11On12** device created with only
  `D3D11_CREATE_DEVICE_BGRA_SUPPORT` (`crates/sk-app/src/shared_gpu/legacy.rs`).
  mpv's D3D11 render API decodes with `d3d11va` on the device it is given, and
  D3D11On12 does not provide the video interfaces, so that path would cost the
  zero-copy hardware decode (not measured — check once in the prototype, it
  decides nothing else). We keep our own device and the shared textures.
- **Formats.** Windows: `R16G16B16A16_FLOAT` (or `_TYPELESS`) on every GPU path.
  macOS: `RGBA16Float` textures, IOSurface `'RGhA'`, CVPixelBuffer
  `64RGBAHalf`. Alpha is straight, not premultiplied.
- **Colour.** Linear, BT.709 primaries, **1.0 = SDR white** on every platform;
  values may exceed 1.0 or go below 0. Skathi converts to scRGB itself
  (scRGB = value × `sdr_white_nits` / 80).
- **What happens on present** (`crates/sk-tree/src/surface/d3d12.rs`,
  `metal.rs`): on a command queue of the surface's own (from `43da7b21`,
  Skathi #3; released 0.4.0 still shared one copy queue between all surfaces,
  stage M2 read it in the code), GPU-wait the producer's fence for `wait`, copy into one of up to 4 surface textures (`MAX_SLOTS`),
  signal Skathi's own fence, signal the producer's fence with `release`. So
  `release` means "my copy finished". The D3D11 renderer instead waits for
  `wait` **on the CPU** of the presenting thread (100 ms timeout → `Timeout`),
  and keeps one texture per surface.
- **The fence or event is per `SharedTexture`.** Wait and release both go to the
  object given to that texture. Skathi does not check that values rise.
- **`Busy`, `FormatMismatch`, `DeviceChanged`, `Timeout`** are decided before
  anything is queued: neither the wait nor the release is ever submitted for
  that frame. Every present that returned `Ok` gets its release, in order, even
  if a newer frame replaced it before it was shown.
- **A `wait` value that never arrives stalls that surface's queue** on D3D12
  and Metal (from `43da7b21` the queue is per surface, so other surfaces and
  the UI keep going; in 0.4.0 and at `8ab3a3d` it stalled every surface). The
  surface is still dead until it arrives, so a `wait` value that has been
  handed to Skathi must be signalled; see rule 4 below for what that means when
  rendering fails. Under the D3D11 renderer the wait is on the presenting thread's
  CPU, up to `.with_timeout` (100 ms) → `Timeout`.
- **Slot reuse is lazy on D3D12:** a finished slot is reclaimed inside
  `commands()`, which runs after `free_slot`, so it frees one present late.
  Whether a ring of three is enough against the real thing has to be checked on
  the prototype. (Historical, 0.4.0: #12 fixed the permanent `Busy` this caused
  with larger rings, and a ring of 3, and of 2, kept the cadence in the
  Windows prototype.)
- **HDR output, on demand (0.4, commit `32baf69`).** A window stays on the
  8-bit path (`B8G8R8A8_UNORM` swap chain on Windows, `BGRA8Unorm`
  `CAMetalLayer` on macOS), bit for bit as before, until its display is in HDR
  **and** a visible element shows a half-float surface whose CSS
  `dynamic-range-limit` allows more than 1.0 there. Then it switches: Windows
  (D3D12 and D3D11 renderers) to `R16G16B16A16_FLOAT` +
  `DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709`, macOS to `RGBA16Float` extended
  in the screen's colour space (via gpui). (Superseded for macOS: since
  `8a1f1728`, Skathi #8, the layer is tagged extended sRGB, which stage MM
  observed; released 0.4.0 also needed a primer to enter EDR at all, fixed
  with #4's `aad939ae`.) The element then draws the frame's
  own half floats from a half-float atlas tile, clamped to its limit and the
  display's headroom; the rest of the page within one 8-bit code of the SDR
  look. Two seconds after the last such element goes, the window switches back.
  About 3× the swap-chain memory; at 4K with 96 translucent layers on an RTX
  5070 a D3D12 frame was 23.4 ms against 23.2 ms. No HDR on Linux.
  - `dynamic-range-limit: standard` keeps highlights at SDR white,
    `constrained` at 2×, `no-limit` at the display's headroom,
    `dynamic-range-limit-mix()` in between. The video element needs
    `no-limit`.
  - `display.output_headroom` is the headroom where the window can draw HDR,
    else 1.0; `content_headroom()` is that limited by the element's
    `dynamic-range-limit` — the number to give mpv.
  - The SDR path is unchanged: values clamped to 0…1, exact sRGB curve
    (`surface/color.rs`).
  - At 0.4.0 Skathi's macOS page (`gpu-frames-macos`) said "the window does
    not ask for EDR yet" while the commit and the native-frames page said it
    did. The pages were brought in line by `711649e2` (#4). Trust the
    prototype measurements (macOS 26.5.2) over either.
- **Display info** (`surface/display.rs`): Windows reads SDR white and HDR state
  from `QueryDisplayConfig` once a second and `IDXGIOutput6::GetDesc1`; macOS
  reads `NSScreen` EDR values and `maximumFramesPerSecond`. `headroom` =
  peak / SDR white. Windows reports no variable refresh.
- **Linux:** surfaces take CPU frames only (no dmabuf). Moving to Skathi would
  cost the `feature/linux` branch its zero-copy render-API path.

### Frame events (shipped in 0.4)

Agreed with Skathi's maintainers in November 2026 and shipped in 0.4 as agreed;
the names differ from the draft (`crates/sk-tree/src/surface.rs`,
`surface/feedback.rs`).

- `surface.on_frame(|e: FrameEvent| …)` returns a `FrameWatch`; reporting stops
  when it is dropped. Rust only — **no frame event reaches JavaScript** (the
  element's JS events are `surfaceresize`, `surfaceready`, `surfacelost`,
  `surfacedisplaychange`). Our frontend never needed it: mpv is fed in Rust.
- While a watch is held, every present that returned `Ok` ends in **exactly
  one** event, in frame-id order (an early-settled frame waits for its elders),
  with no timeout. Measured limits: presents still in flight when the watch is
  dropped get none (1–6 at shutdown on macOS), and on Windows 1–2 frames in
  flight at close got no event in 10 of 13 closes (not filed; whether the watch
  was still held then is not recorded). A producer keeps the `FrameWatch` until
  it stops presenting and treats a frame without an event at teardown as
  dropped. A `Presented` event
  can be `Estimated`, and on macOS two `Estimated` events have shared one
  refresh during a resize (0.5.2):
  - `FrameEvent::Presented { frame, shown_at, refresh_interval, precision }`,
    once, on the frame's first refresh; how many refreshes it stayed is the next
    frame's `shown_at` minus this one's. `refresh_interval` is that refresh's
    real distance from the previous one (real under VRR); the nominal period is
    `info().display.refresh_period`.
  - `FrameEvent::Dropped { frame, reason: FrameDropReason }`: `Superseded` (a
    newer frame replaced it first — reported as soon as known), `Hidden`
    (nothing showing the surface can be on screen), `DeviceLost` (these can
    arrive after the Rust `SurfaceEvent::DeviceLost`: 40–58 ms after it on
    Windows, Skathi #16; their order against the JS `surfacelost` was not
    measured), `Closed` (window closed or app quit; at quit delivered on
    the quitting thread).
- `FramePrecision::Exact | Estimated`. Windows: `Exact` when the swap chain's
  frame statistics name the window present that showed it; `Estimated` (one
  refresh after the first vblank following its GPU work, ±1, with the nominal
  period as `refresh_interval`) when statistics lag, a poll is missed, or the
  swap chain's statistics stay empty — which is what a composition (transparent)
  window gets. macOS: `Exact` from the drawable's `presentedTime`. Linux:
  always `Estimated`. Measured since: on Windows (`74e63fad`) 75–80 % of frames
  were `Estimated` with the default feedback, and late frames were reported on
  the refresh they missed; `exact_frame_feedback(true)` made them all `Exact`
  in the prototype's window (`c7a524d5`). On macOS some `Estimated` events
  remain at sparse presents and in resizes (see "Frame events (#19)").
- How it is derived: the tick records which surface frame each window present
  carries; gpui reports each present's outcome (`gpui::present_feedback`); a
  frame is presented by the first shown present that carried it, superseded
  once nothing holds it. A window present that DWM discarded therefore makes its
  frames `Superseded` — the case the draft asked to confirm.
- The callback runs on Skathi's vsync thread on Windows (with
  `exact_frame_feedback(true)`, on its separate feedback thread) and on a
  thread of its own on macOS and Linux, never the UI thread. Keep it short; it may present or
  drop its own watch. Only a stalled UI thread delays an event.
- `shown_at` and `display.last_vblank` are in `skathi::monotonic_ns()`: QPC on
  Windows, `mach_absolute_time` on macOS, `CLOCK_MONOTONIC` on Linux, in ns —
  the clock mpv's `mp_raw_time_ns` reads. `mpv_get_time_ns` counts from an
  arbitrary start: take the offset once.
- `stats()` counts frames presented and shown.

## Synchronisation, measured

### The trap: one fence for the whole ring

Skathi's documentation shows the per-frame values `2n+1` / `2n+2`. With **one**
fence or event shared by all the textures of a ring, the producer's "wait until
the slot's previous frame (n−3) was released", i.e. ≥ 2(n−3)+2, is already
satisfied by the producer's **own** signal for frame n−1 (2n−1). The wait does
nothing and the producer overwrites frames the consumer has not copied.

- Windows, Vulkan producer, emulated D3D12 consumer: the fence went
  *backwards* 57 times (first 229 → 2), every checked frame was wrong, and the
  run deadlocked at frame 296 with the fence stuck at 576. Other runs: 63–67 bad
  frames of 1000 and no deadlock — it depends on timing. D3D11 producer:
  deadlock at frame 284.
- macOS, Vulkan producer, emulated Metal consumer: with the consumer slowed by
  6 ms, 224 of 490 copied frames were wrong; slowed by 12 ms at 120 Hz, 668 of
  675. Every wrong frame was a *later* frame, whole — never torn. A test that
  only looks for tearing passes this.

Two fixes, both verified bit-exact over 1000 frames:

- **One fence/event per texture** (what Skathi's API is shaped for, since the
  fence is attached per `SharedTexture`). This is the choice.
- One shared fence, but the producer also GPU-waits for `2n` before signalling
  `2n+1`, so the two sides strictly alternate.

### Rules for the producer

1. One fence (Windows) or `MTLSharedEvent` (macOS) per ring texture; values per
   texture only rise.
2. Remember, per texture, the `release` value of the last frame Skathi
   **accepted** (`Ok`). Wait only for that before writing again. A frame
   refused with `Busy` was never queued and will never be released — waiting
   for it hangs forever.
3. After `Busy`, the texture is still ours. During playback, skip the frame. If
   the frame must reach the screen (paused, a frame step), present it again.
4. Queue only frames whose `wait` signal has been submitted: render, submit
   the signal, then present. A `wait` value handed to Skathi that is never
   signalled stalls the surface (above). If a recoverable failure leaves an
   accepted frame waiting, resolve that wait without publishing the frame as a
   new picture, and present the next good one. A dead device retires the whole
   generation (rule 7). On Windows the producer waits for the slot on the CPU;
   the Vulkan path needs its own synchronisation and retirement contract
   (never queue a wait on a value that will never be submitted, drain before
   freeing), see "The Vulkan render API".
5. A step or a pause is complete when a `FrameEvent::Presented` arrives with
   an id at or after the first present after the command. mpv presents twice
   per paused step (the frame, then a redraw), and the redraw may supersede the
   first (stage MX: 2 of 40 steps). Present again only if no frame of the
   request reached `Presented`. Expect a missing event at teardown, `Hidden`
   while covered, and `DeviceLost` across a device change.
6. On `Hidden`, stop calling mpv's render; audio continues. Render the current
   frame once when visible again.
7. On the first Windows `DeviceLost`, stop submitting. Skathi sends
   `DeviceLost` again when the replacement device is ready (471–506 ms after an
   injected removal, Skathi #16); opening a texture before that returns
   `NoDevice`. Then create new per-slot shared fences and reopen the textures
   with them against the new device, and rebuild the ring. A fence shared with
   the removed device read `UINT64_MAX` for good on our machine (not on the
   maintainer's). Keep the producer's own device only while it stays healthy;
   in the tested removals mpv's D3D11 device was not affected.
8. Render at `info().width` × `info().height` and recreate the ring when it
   changes.

### Windows: Vulkan → D3D12

- Same adapter on both sides; compare LUIDs (`D3D12` adapter LUID against
  `VkPhysicalDeviceIDProperties.deviceLUID`).
- Create the resource in D3D12 (`D3D12_HEAP_FLAG_SHARED`, state `COMMON`) and
  the fence with `D3D12_FENCE_FLAG_SHARED`; export NT handles with
  `ID3D12Device::CreateSharedHandle`.
- Vulkan: `VkImportMemoryWin32HandleInfoKHR` with
  `VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE_BIT` — the 1920×1080 RGBA16F
  image imports as `DEDICATED_ONLY | IMPORTABLE`, 17 694 720 bytes,
  device-local. The fence: `VkImportSemaphoreWin32HandleInfoKHR` with
  `VK_EXTERNAL_SEMAPHORE_HANDLE_TYPE_D3D12_FENCE_BIT`, as a timeline semaphore;
  `vkGetSemaphoreCounterValue` then reads the D3D12 fence values exactly.
- Release the image to `VK_QUEUE_FAMILY_EXTERNAL` after rendering.
- Unpaced, with the alternation fix: 188.6 µs per frame, producer CPU 29.8 µs
  (p95 55), consumer CPU 126 µs. Round trip from submit to the producer seeing
  `2n+2`: mean 348 µs, p95 402–514 µs, p99 ~1.2 ms, max 2.0 ms. With the
  consumer paced at 240 Hz: 1000 of 1000 correct.
- Validation layers: 0 errors in pipelined runs. In a one-frame-at-a-time run
  they reported "signaled with value 3 which is smaller than the current value
  20" while the driver's counter read correctly — probably the layer's
  bookkeeping, not proven. The D3D12 and D3D11 debug layers failed to load
  (`0x887A002D`) without the optional "Graphics Tools" feature.
- **A D3D11 producer** (`ID3D11Device5::OpenSharedResource1` + `OpenSharedFence`,
  `ID3D11DeviceContext4::Wait/Signal`) is bit-exact too. Two set-ups were
  measured and they behave differently:
  - *Textures and one shared fence created in D3D12, opened in D3D11*
    (synthetic producer): 203 µs per frame, round trip 358 µs, but `Signal`
    **blocked the CPU thread for about 4.2 ms** while a queued GPU wait was
    unsatisfied.
  - *Textures (`D3D11_RESOURCE_MISC_SHARED_NTHANDLE`, no keyed mutex) and one
    fence per texture created in D3D11, opened in D3D12* — with **real mpv**
    rendering into them through the D3D11 render API, ring of 3: mpv accepts the
    shared texture as its target, D3D12 opens it (format 10, flags render target
    + simultaneous access), every sampled frame bit-exact, and `Signal` **never
    blocked** (≤ 0.25 ms, even with 718 GPU waits queued). Why the two differ was
    not isolated. **This is the set-up to use.**
- **With a D3D11 producer, wait for the slot on the CPU, not the GPU.** A GPU
  wait never blocks the thread, so back-pressure turns into latency: work piles
  up on the device and mpv's delivery stalls in ~70 ms bursts. A CPU wait blocks
  the render thread for exactly the consumer's period and keeps latency at about
  three frames:

| mpv → emulated D3D12 consumer, 1080p | Slot wait | Wait µs mean / max | Delivery latency p50 / max | Frames, drops |
|---|---|---|---|---|
| 24 fps | GPU | 25 / 234 | 0.29 / 1.7 ms | 364, 0 |
| 24 fps | CPU | 0.6 / 44 | 0.28 / 1.7 ms | 364, 0 |
| 4K, 24 fps | GPU | 15 / 208 | 0.33 / 1.5 ms | 364, 0 |
| Untimed, consumer at 240 Hz | GPU | 3.7 / 157 | 45 / 80 ms (bursty) | 721, 0 |
| Untimed, consumer at 240 Hz | CPU | 3948 / 4843 | 12.4 / 13.5 ms (even) | 722, 0 |
| `speed=10`, consumer at 240 Hz | GPU | 3.6 / 128 | 23 / 28 ms | 680, 42 |
| Consumer at 20 fps | GPU | 21 / 183 | 262 / 394 ms | 308, 32 |
| Consumer at 20 fps | CPU | 49 581 / 50 838 | 150 / 151 ms | 303, 60 |

- A D3D11 ↔ D3D11 keyed-mutex share also works (values above 1.0 survive, 200
  hand-offs at ≤ 400 µs each including a 16 MB readback), but keyed mutexes do
  not fit D3D12; it is not the path.

### macOS: Vulkan → Metal (MoltenVK 1.4.2)

An earlier, separate probe from the prototypes and from the offscreen proof
under "The Vulkan render API". Its Vulkan API version is not recorded here;
stage M1, with the player's bundled MoltenVK, read the device's version as
1.2.357, and the offscreen proof requested Vulkan 1.2.

- **Import the IOSurface with `VkImportMetalIOSurfaceInfoEXT`**
  (`VK_EXT_metal_objects`). Exporting the image back returns the same
  IOSurface-backed `MTLTexture` (RGBA16Float, 15 360 bytes per row). Readback
  matched exactly: 0 mismatches in 8 294 400 half-floats, before and after 200
  more draws.
- **The `VK_EXT_external_memory_metal` MTLTexture import read back all zeros
  in this probe.** Every call succeeded and the draw succeeded, but MoltenVK
  rendered into a texture of its own; the IOSurface read back all zeros, and
  nothing reported it. (Historical: the probe's exact build and setup are not
  recorded. The Vulkan research found the path supported in the tagged 1.4.2
  source but did not run it, so both import paths are to be compared under
  validation; see "Next steps".)
- **The shared event works in both directions:** a timeline `VkSemaphore`
  exported as `MTLSharedEvent` (`VkExportMetalSharedEventInfoEXT`), or an event
  made in Metal imported with `VkImportMetalSharedEventInfoEXT`. Needs only
  `VK_EXT_metal_objects` and the `timelineSemaphore` feature. Latencies: Vulkan
  signal → Metal wait 0.28–0.33 ms; Metal signal → Vulkan GPU wait 0.13–0.25
  ms; → `vkWaitSemaphores` 0.06–0.22 ms. Lower values are ignored.
- **No barrier was needed for correct pixels in this probe** (none, layout
  transitions only, and a queue-family release to `VK_QUEUE_FAMILY_EXTERNAL`
  all gave 0 bad pixels). That is a pixel check, not a synchronisation
  contract. The later libmpv ring ran under validation with
  `VK_QUEUE_FAMILY_IGNORED`; a release to `VK_QUEUE_FAMILY_EXTERNAL` is still
  untested, so that ownership transfer stays provisional. The offscreen proof used `VK_QUEUE_FAMILY_IGNORED` with
  one family and the research proposes `GENERAL` as the hand-off layout.
- Ring of 3 IOSurfaces into 4 consumer textures, every pixel of every copied
  frame checked on the GPU: 0 bad pixels at 120 Hz and unthrottled (~3300 fps),
  both event directions, with a slow consumer. Per frame at 120 Hz: producer CPU
  0.11–0.19 ms, consumer CPU ~0.05 ms, consumer GPU ~0.2 ms, submit-to-copy-done
  p50 1.6–3.3 ms, p99 3–5 ms. Unthrottled, 23–38 % of presents were `Busy`, as
  expected.
- Without an event (CPU wait before each hand-off): the producer thread blocks
  ~1.3 ms per frame instead of ~0.14 ms, and peak throughput drops to ~2000 fps.
- **A trap that hides bugs:** macOS orders IOSurface access between queues by
  itself — a Vulkan write waits for a Metal command buffer that reads the
  surface, but only once that buffer is scheduled. A buffer parked on
  `encodeWaitForEvent` gets no such protection. Undocumented; don't rely on it.
- Bundled `libMoltenVK.dylib` was loaded directly, without the Vulkan loader.
  Format features for `R16G16B16A16_SFLOAT`: everything (sampled, filter,
  storage, colour attachment, blend, transfer, blit), optimal and linear tiling.

## Colour and HDR

### gpu-next in its own window (Windows, measured)

Pixel values from an FP16 capture of what Windows composes (scRGB, 1.0 = 80
nits). mpv's own 16-bit window screenshots cannot measure this: they stop at
1.0 and render with different settings than the screen.

- **`target-trc=linear` is SDR-relative.** 1.0 is the reference white, and the
  reference white is `hdr-reference-white` (default `auto`, which found 240 nits
  here only because mpv had its own window on that display). **`target-peak` is
  ignored** with `linear`: 203 and 846 gave identical output. Result with the
  defaults: SDR white at 1.18, which Skathi would clip.
- With **`hdr-reference-white=203`**, SDR white is exactly 1.0000; a 1000-nit
  highlight → 0.985; 203 nits → 0.32.
- **`target-trc=scrgb` is absolute** (1.0 = 80 nits) and honours `target-peak`.
  SDR white → 2.996 (240 nits); 1000 nits → 12.15; with `target-peak=846`, 1000
  nits → 10.28. On Vulkan it is the only setting that gets an
  `R16G16B16A16_SFLOAT + EXTENDED_SRGB_LINEAR` surface in mpv's own window.
- With mpv's own FP16 swapchain and the colour-space hint on, mpv overrides
  `linear` to scRGB.

### The D3D11 render API (old renderer) into an FP16 texture (Windows, measured)

Values read back from the texture mpv rendered into; flat-patch test clips;
checked against the renderer's source at `a39482e7a`. The old renderer's rules
differ from gpu-next's:

- **Without `target-trc=linear` the texture holds gamma-encoded values**
  (SDR white 1.000, 50 % signal 0.5015). With `target-trc=linear
  target-prim=bt.709`: white 1.000, black 0.000, 50 % → 0.1912 (BT.1886).
- **In a linear target 1.0 is fixed at 203 nits, and `hdr-reference-white` is
  ignored.** That suits Skathi's contract: Skathi maps 1.0 to the system's SDR
  white itself, so HDR reference white (203 nits) lands on SDR white, which is
  the standard mapping.
- **`target-peak` above 203 lets highlights through above 1.0**, unchanged up to
  target-peak / 203; the tone-mapping curve (bt.2390 by default; clip and mobius
  work) applies only above that.
- **`hdr-compute-peak` must be `no`.** The default `auto` is a scene-adaptive
  exposure, slope = min(1, 0.25 / frame average), applied to the whole frame: it
  only darkens and depends on content (it scaled the patch frame by 0.603 and
  pulled a real HDR10 clip's maximum down to 0.426).
- `scrgb` in this renderer means 1.0 = 10 000 nits (1000 nits → 0.100), not the
  Windows 80-nit convention. `pq` gives correct HDR10 code values (1000 nits →
  0.7520, 203 → 0.5806).

| HDR clip, `hdr-compute-peak=no` | 1000 nit | 500 | 203 | 100 | 10 |
|---|---|---|---|---|---|
| default options | 1.000 | 0.990 | 0.896 | 0.722 | 0.255 |
| linear, peak auto or 203 | 1.000 | 0.980 | 0.785 | 0.488 | 0.050 |
| **linear, peak 846** | **4.164** | **2.467** | **1.004** | 0.491 | 0.050 |
| linear, peak 203, `tone-mapping=clip` | 1.000 | 1.000 | 1.000 | 0.491 | 0.050 |

  With linear + peak 10000 a 1000-nit patch reads 4.930 and a real HDR10 clip
  (PQ up to 10 000 nits) peaks at 49.25. Software decoding gives the same values
  as `d3d11va`. For SDR content `hdr-reference-white` and `target-peak` change
  nothing.

### What to configure

- **D3D11 render API (old renderer):** `target-trc=linear target-prim=bt.709
  hdr-compute-peak=no target-peak=<203 × info.content_headroom()>`, re-set on
  every `Changed`/`DisplayChanged`. On an SDR display or with the window not in
  HDR that is `target-peak=203`; on this display with `dynamic-range-limit:
  no-limit`, 846 (1000 / 240 × 203). This is the recipe Skathi's documentation
  gives, plus `hdr-compute-peak=no`. The `<sk-surface>` needs
  `dynamic-range-limit: no-limit` in CSS, or Skathi clamps highlights to 1× or
  2× SDR white.
- **gpu-next (own window today, render API later), output headroom 1:**
  `target-trc=linear target-prim=bt.709 hdr-reference-white=203`. Here
  `target-peak` does nothing with `linear`, and without `hdr-reference-white`
  white lands at 1.18.
- **gpu-next, headroom > 1:** `target-trc=scrgb target-prim=bt.709
  target-peak=<display peak nits> hdr-reference-white=<sdr_white_nits>`, then
  scale by 80 / `sdr_white_nits` (÷3 at 240 nits) to get Skathi's 1.0 = SDR
  white: SDR white → 0.999, 1000 nits → 4.05. mpv cannot apply that scale
  itself (a user shader was not tried), so either the producer does it, our
  mpv patch makes `linear` honour the peak, or Skathi accepts absolute scRGB as
  a second colour space.
- **With gpu-next, always set `hdr-reference-white` explicitly.** `auto` depends
  on mpv having a window on the display, which it will not.
- **A native HDR10 layer (Windows, render API into an HDR10 swap chain's back
  buffer; measured):** `target-trc=pq target-prim=bt.2020 target-peak=<display
  peak> hdr-reference-white=<desktop SDR white>`; the code re-sets them when
  the mode, the white level or the peak changes (no live change was
  tested). Without the last option SDR white lands at 203 nits on a 240-nit
  desktop (the saturated bars of the test clip read 0.82 of the interface's
  white); with it, 0.97, as in the file (one launch each). On a display not in HDR, an 8-bit sRGB swap chain
  with `target-trc=srgb target-prim=bt.709` and mpv's own tone mapping
  (measured only forced, on an HDR desktop). See "A native video layer under
  the page".
- `DISPLAYCONFIG_SDR_WHITE_LEVEL` (via `DisplayConfigGetDeviceInfo`) read 3000 →
  240 nits; `MaxLuminance` 1000 → headroom 4.17.
- The gpu-next numbers come from mpv rendering into its own window; through a
  render-API texture they need re-measuring once that backend exists. Nothing
  was measured on macOS. (Superseded: the render-API texture has since been
  measured, through PR #18845 on D3D11 and on macOS GL; see the two "gpu-next
  through mpv PR #18845" subsections. There `linear` honours `target-peak`,
  unlike this own-window result.)

### HDR in today's player

- **Windows:** the player sets only `vo=gpu-next`, `hwdec=auto-safe` and `wid`.
  With Windows HDR on, mpv switches its swapchain from `R10G10B10A2` + G22/P709
  to `R10G10B10A2` + **G2084/P2020 (HDR10)** — for SDR content too — and
  renders an HDR10 clip to that swap chain's 1000-nit target.
  `target-colorspace-hint=no` tone-maps to
  SDR at the SDR white level (target bt.709 / srgb / 240). `vo=gpu` does not
  follow the hint and stays 8-bit SDR (bt.709 / gamma2.2 / 203).
  Read off the composed desktop in the Windows matrix (Frame Player 1.49.1):
  the window's maximum with the 2160p HDR10 clip is 5.30× SDR white (about
  1270 nits), as with bare mpv, against the renderer's 1000-nit target; with
  an SDR clip it is 2.28 (measured), along the test pattern's sharp edges
  (scaler overshoot in a target that does not clip, inferred). Media
  Player, MPC-HC and VLC keep SDR at white, and their HDR10 readings are
  consistent with handing that clip through untouched (see "Native players
  as a reference").
- **macOS: HDR is tone-mapped to SDR.** For a PQ BT.2020 clip, video-params are
  bt.2020 / pq / sig-peak 4.926 and the target is **bt.709 / gamma2.2 / 1**. The
  surface is picked once at start-up (`VK_FORMAT_A2R10G10B10_UNORM_PACK32 +
  VK_COLOR_SPACE_SRGB_NONLINEAR_KHR`, Metal layer `bgr10a2Unorm`, colorspace
  sRGB) and not re-picked when an HDR file opens, although the swapchain offers
  `HDR10_ST2084`, `HDR10_HLG`, `EXTENDED_SRGB_LINEAR` and `BT2020_LINEAR`.
  gpu-next logs a dynamic tone-map target of 3848–4315 nits against a 203-nit
  display. `vo=gpu` renders the same clip visibly dimmer (a flat region RGB
  5/21/240 vs 8/30/112; screenshot SSIM 0.866 against gpu-next). This is a
  shortcoming of today's player, independent of the move.

### EDR on macOS (for Skathi's HDR output, measured with a CALayer showing the IOSurface)

Measured before Skathi 0.4 had an HDR window, to know what macOS does with
values above 1.0. Skathi 0.4 now switches the layer itself; these numbers say
what to expect from it, not how to configure it.

- Values above 1.0 reach the compositor unclipped: an HDR ScreenCaptureKit
  capture of the window (extended Display P3, half-float) matched every sample
  to ~4 decimals after conversion (grey 2.0/3.0/4.0 → 1.3535/1.6123/1.8252 in
  P3). Nobody looked at the panel; the brightness on glass rests on the headroom
  readings.
- `wantsExtendedDynamicRangeContent` is **deprecated in the macOS 26 SDK** in
  favour of `preferredDynamicRange`. With `preferredDynamicRange = high` and a
  headroom tag of 4 (`contentsHeadroom` / `kIOSurfaceContentHeadroom`) the
  headroom held at exactly 4.0; with the legacy flag alone it went to the
  panel's full 16.0.
- Headroom ramps: 1.21 after 0.3 s, 2.26 after 1.2 s (tagged); 1.47 → 5.16 →
  16.0 (legacy). macOS keeps raised headroom for several seconds after the
  window goes away.
- `NSScreen`'s EDR values only update while the app processes AppKit events; a
  thread that only spins a run loop sees frozen values.
- Displaying through a `CALayer` whose contents is the IOSurface: one buffer is
  in use by the compositor on ~120 of 120 frames (writing it in place tears);
  two buffers → ~60 fps on 120 Hz; three → ~110 fps. A naive `CAMetalLayer`
  blit loop blocked 5–7 ms in `nextDrawable` with 6.3–18.7 ms frame intervals
  (probe-quality code; worth re-checking in Skathi's own pipeline).

## Frame pacing

Today mpv owns its swapchain and times each frame against the display's
refresh. With Skathi, mpv renders at its own pace and Skathi takes the newest
frame at each refresh. Without the display's clock, frames land near a refresh
boundary unevenly — 24 fps on 60 Hz drifts from 3:2:3:2 to occasional 3:3 or
2:2, visible on pans; less at 120/240 Hz, but not gone.

Superseded: the recipe both prototypes measured is "The pacing recipe that won"
under the Windows prototype. `report_swap` is called once per render-loop turn
on the render thread, right after the present or after a skipped repeat; from
`on_frame` it throttled mpv. What follows is the earlier plan, kept for its
reasoning.

- **The display clock needs nothing from Skathi:** on Windows, `DwmFlush` in our
  own thread plus QPC, or `DwmGetCompositionTimingInfo`; on macOS a
  `CVDisplayLink`/`CADisplayLink` for the window's screen (`info().display`
  says which).
- **Which of our frames landed on which refresh** Skathi reports through the
  frame events above, but a reported `shown_at` can be `Estimated` and, on
  Windows with the default feedback, a late frame was reported on the refresh
  it missed. Use a capture to validate cadence.
- **Skathi's documented recipe (0.4.0, no mpv patch; historical, rewritten
  after #10):**
  `video-sync=display-resample`, `display-fps-override` = `display.refresh_hz`
  (updated on `DisplayChanged`), one `mpv_render_context_report_swap()` per
  frame rendered — shown or dropped — called from `on_frame`, and a render
  thread that waits for `MPV_RENDER_UPDATE_FRAME`, sleeps to the middle of the
  refresh (`last_vblank` + `refresh_period`), renders and presents. Without
  `report_swap` libmpv takes the end of the render call as the flip.
  `report_swap` carries no timestamp, so mpv reads the time when the callback
  runs — at most a vblank after the real flip, not the flip itself.
  `surface.rs` (`feedbackStart`/`feedbackStop`) holds 24 fps on 165 Hz as
  7,7,7,7,7,7,7,6 refreshes this way.
- **Our plan (historical):** try that recipe first and measure it against
  `video-sync=audio` with the e2e metrics below; the timestamped `report_swap`
  patch (feed `shown_at` instead of "now") is the fix if the callback delay
  shows up as jitter. (The callback placement throttled mpv instead; a
  timestamped report is now optional future work with its own timing
  contract.) With `Estimated` precision, stay on `video-sync=audio` and use the
  events only to confirm steps and pauses.
- **Today's numbers to compare against** (macOS, e2e, 120 Hz): with the default
  `video-sync=audio`, 0 dropped and 0 delayed frames; gpu-next with
  `display-resample`: vsync-jitter 0.112–0.120, estimated display fps
  119.0–119.5, 2 mistimed frames, 5 delayed during steady playback. `vo=gpu`
  with `display-resample`: jitter 0.150–0.174, 117.4–118.4 fps, 9 mistimed, 15
  delayed. Windows, D3D11, `display-resample`: jitter 0.266, estimated 224.09
  against 240, 235 delayed (not investigated).
- At `8ab3a3d`, `stats().shown` counted a frame when its atlas copy was
  queued, before draw and present — not a "shown" signal. Use `on_frame`.

### An opaque window on Windows (the `<sk-surface>` route only)

An early-plan note, not a requirement any more: the native-layer prototype
needs the window transparent, and Skathi's window presents through a
composition swap chain whatever the backdrop, unless composition is
disabled (read from the source on 0.7; see "A native video layer under the
page").

Exact timestamps come only from `CreateSwapChainForHwnd`, i.e. an opaque window.
(Superseded in part: with `exact_frame_feedback(true)` every event was `Exact`
in the Windows prototype's window on `c7a524d5`, at the cost of about one
refresh. The prototype's window flags are not recorded here, so check them
before making opacity a requirement.)
Ours is transparent only so that mpv's child window shows through the WebView;
once Skathi draws the video, that reason goes. Before asking for an opaque
window, check what else assumes transparency: the mini player, rounded corners,
and the "nothing painted means desktop visible" rule in
[rules/window.md](rules/window.md). The native-layer route needs the
opposite: `backdrop: "transparent"` on both systems, the layer under the page
the way mpv's child window is under the web view today.

## Copies and the zero-copy question

At `8ab3a3d` a GPU frame took three passes inside Skathi: copy into the surface
texture (`copy_in`), half-float → 8-bit mapping (`Ready::for_atlas` →
`draw_mapped`, lazy: only when first shown), copy into the window's atlas
(`node.rs` → `live.rs` → gpui atlas). In 0.4 a window in HDR mode skips the
mapping and puts the frame's own half floats into a half-float atlas tile (two
passes); an SDR window still maps. Neither was measured in 0.4. For a 3840×2160 FP16 frame that is about
300 MB of memory traffic, ~18 GB/s at 60 fps — roughly 2 % of an RTX 5080's
bandwidth, 5 % of an M3 Max's, and an estimated 20–30 % of an integrated GPU's
shared 60–100 GB/s (estimate from bandwidth figures, not measured).

`release` cannot simply be dropped: it is the only way to know Skathi has
finished reading a texture. Without it the producer either tears, keeps a huge
ring and hopes, or allocates a texture per frame. Zero-copy moves the release
later; it does not remove it. Options, cheapest first:

1. **Measure first**, on a weak integrated GPU at 4K.
2. **Map straight from the producer's texture** instead of copying it first;
   release after the mapping pass. Same semantics for us, local change in
   `copy_in`; the mapping must then run eagerly at present time. Saves ~130 MB
   per 4K frame.
3. Map straight into the atlas: one more copy gone, touches gpui's atlas.
4. **True zero-copy:** the window samples the producer's texture while drawing;
   release after the first window frame that no longer references it has
   completed. Needs a new scene primitive in gpui (today `PaintSurface` exists
   only for CVPixelBuffer on macOS, and the D3D12 renderer has no surface path),
   the mapping moved into the draw shader, and tracking of which window frame
   last used the texture. Worth doing together with Skathi's HDR output, which
   has to replace the 8-bit atlas anyway.

A route outside this list has been measured since: no `<sk-surface>` at all.
With mpv rendering straight into a swap chain or `CAMetalLayer` of ours under
a transparent page, there is no ring of ours and no Skathi copy, and the
window stays 8-bit; what remains is the wait for mpv's release before the
present (on macOS a Metal command buffer waits on the slot's event); the cost moves to keeping the layer in step with the
page (see "A native video layer under the page"). The zero-copy work above
matters only if the picture stays in an `<sk-surface>`.

## Other findings along the way

- **A refused mpv.conf line hangs the player at start-up on macOS.**
  `gpu-api=opengl` is refused (-7), and instead of the frontend's "drop
  mpv.conf and retry", the app deadlocks: the plugin's `init` worker is in
  `mpv_wrapper_create` → `drop_in_place<Builder>` → `mp_destroy_client` →
  `pthread_join`, mpv's core thread in `mp_shutdown_clients` →
  `mp_wait_events`, and two `set_property` calls wait on the plugin's instance
  mutex. Plain libmpv (create, failed option, destroy) does not hang, so it is
  the wrapper's error path. Probably any option refused at set time does the
  same; only this one was tried.
- **Windows: `gpu-api=opengl` with the automatic context crashes mpv.**
  `libEGL.dll` (ANGLE) is not shipped: `Failed to load LIBEGL.DLL`, then a
  segfault. `gpu-context=win` or `dxinterop` work. The player uses D3D11, so it
  is reached only through a user's mpv.conf.
- **libmpv refuses `vo=help`, `gpu-api=help` and friends** and prints nothing;
  read the lists from `option-info/<name>/choices` instead. Windows build:
  `gpu-api` d3d11/vulkan/opengl; `gpu-context` d3d11, winvk, angle, win,
  dxinterop, displayvk; hwdec d3d11va, nvdec/cuda, dxva2, vaapi, vulkan;
  `d3d11-output-format` accepts rgba16f, `d3d11-output-csp` linear and pq.
- **e2e screenshots after a seek can lag mpv's state by more than 400 ms**
  (macOS): two identical builds differ at SSIM 0.94–0.96 on the seek steps.
  `FrameEvent::Presented` for the frame id is the proper wait.
- **SDR, gpu-next against `vo=gpu` on macOS:** flat colours match within 1/255,
  but window screenshots differ at SSIM ~0.92 across the whole video area
  (dithering or scaling; not investigated).

## Previews and posters (planned)

Not measured as part of this work. The figures for today's sidecar come from
[rules/thumbnails.md](rules/thumbnails.md); everything about version 2 is a
plan and an expectation, to be replaced by measurements.

### What today's player does and why

Every frame shown outside playback comes from the FFmpeg sidecar
(`thumb_service.rs`): the seekbar storyboard, the exact frame under a resting
cursor, and the posters of the start screen's "continue watching" cards
(`poster_frame`, and `poster_capture` for torrents). They share one decoder
(`ThumbSession`) and one colour path (`color.rs`). The reason is the embedding:
mpv draws into a native window behind the web view, and a second mpv would
need a second window. Compared with mpv-side thumbnailers that run a second mpv
process (thumbfast, mpv_thumbnail_script), the sidecar wins on what it was
built for — instant hover from a persistent disk cache, the exact frame on
rest, a budgeted background pass — and loses on four points:

- **Colour is a reimplementation.** PQ, HLG and BT.2020 → BT.709 are ours;
  Dolby Vision profile 5 is an approximation, because libavcodec does not apply
  the RPU and libplacebo does.
- **Decoding is software only.** The exact frame on a resting cursor costs up
  to 1.3 s on 4K30 HEVC 10-bit and 1.9 s on 4K60 VP9 (one GOP); a cold 4K
  storyboard holds 1.6–2 cores for ~40 s.
- **Network sources have no previews and no posters** — the hover shows the
  timestamp alone and the card a link mark.
- **A torrent is read twice**, once by mpv and once by the sidecar.

### What the render API makes possible

With mpv rendering through the render API into textures we own, a second
libmpv core in the same process can render too — into a small texture of its
own, with no window. That is thumbfast's model without the second process or
the pipe, and it needs none of the libmpv patches in
[mpv-patches.md](mpv-patches.md) (§6 there, a second decoder inside libmpv, is
the one that conflicts most with upstream).

**Hover previews.** A preview core with `hwdec`, the gpu-next renderer and the
same colour options as playback, rendering at preview size:

- colour identical to playback by construction, Dolby Vision included;
- hardware decoding for the exact frame on rest, the case where the sidecar is
  slowest;
- previews for network sources, since mpv opens HTTP and ytdl itself (the
  hover is somebody waiting, so a request per resting cursor is acceptable
  there; the background storyboard for a stream stays off);
- the frame can go to a Skathi surface as a texture: no JPEG round trip, and an
  HDR preview in an HDR window becomes possible.

**Start-screen posters.** The larger gain is not decoding at all: playback
already renders every frame into our texture, so when a file is closed (or the
viewer leaves for the start screen) the last presented frame can be read back
and saved as the card's poster. That frame is exact, in playback colour, and
exists for **every** source — a YouTube stream or a torrent with holes
included, the cases that today get no poster. The start screen must not open a
network source to make one: that is a request (and for YouTube a ytdl run)
every time the list is shown.

### Order

1. **The preview core for the exact frame on rest only.** The storyboard stays
   on the sidecar. Smallest change, and it removes the slowest case.
2. **The poster from the last presented frame**, with today's
   `poster_usable` check in front of it: a viewer who stopped on the credits or
   a black frame should get a storyboard frame instead. `rgb_score` and the
   candidate search stay for files with no captured poster (anything opened
   before the update).
3. **Only if the measurements favour it: the storyboard through the preview
   core as well.** Then `color.rs` and the preview half of the sidecar go. A
   disk cache needs a GPU → CPU read-back per cell, and the pass must keep the
   five constraints of the background budget in
   [rules/thumbnails.md](rules/thumbnails.md) (capped threads, duty cycle,
   QoS, suspended while casting, slower on battery).

### To measure before committing

- What a second core costs: a libplacebo context and its GPU memory, on an
  integrated GPU as well as the two machines above.
- Hardware decoder sessions: some GPUs limit how many run at once. The preview
  core must never take the decoder playback needs, so it needs a fall-back to
  software decoding (or to the sidecar).
- Seeking in the preview core on long GOPs: hardware decode only helps if the
  core's own seek does not eat the gain.
- macOS: the preview core stays on the OpenGL interim path, without gpu-next's
  compute shaders, until it uses the Vulkan render API, which now runs in
  the Skathi prototype and in the ported interface.
- The read-back of the last presented frame: its cost at 4K, and that it is
  taken from a texture the producer still owns (before its `release`), not one
  Skathi may be reading.

## Still open

The prototypes measured, in their configurations, most of what this list
held before: the real Skathi consumer, the HDR switch on macOS (on Windows the
colour values in HDR mode, not a switch while playing), the frame events, the
pacing recipe, colour on macOS and through a gpu-next render-API texture.
Allocation behaviour, accurate feedback across setups and the colour settings
remain substantive open points below. What is left:

- **Which route the picture takes: `<sk-surface>` or a native layer under
  the page.** Measured on both systems with the ported interface. Against
  the shipping player with every process counted, the `<sk-surface>` route
  is, by metric:
  - macOS memory: lower in every idle cell and every 1080p cell with the
    controls visible; higher with video playing and the controls hidden
    (+97 MiB at 1080p screen-sized, +428 to +463 at 2160p PQ) and, less, at
    2160p PQ with the controls visible (+141);
  - Windows: working set and CPU lower in every cell; GPU memory higher in
    19 of 21 cells (+54 MiB at 1080p screen-sized, +543 with 2160p HDR10)
    and lower only in 1080p real fullscreen and paused; GPU time higher in
    every playing cell;
  - macOS GPU time higher in the 1080p cells and about doubled in native
    fullscreen, level at 2160p PQ in a window.

  The native layer takes most of the memory and GPU cost back (macOS
  2160p PQ 2008 → 1719 MiB, Windows 1684 → 1228 MiB of GPU memory) and
  needs no overlay pass. It is now Skathi's own `<sk-layer>` (#44–#48
  closed), measured in the port on both systems with the same cost as the
  prototype; how the two routes compare as the measurements stand is
  under "Decided for version 2". Open before choosing:
  - on Windows the layer was one desktop refresh behind the page while the
    window grew; Skathi changed the commit order for it (`beab4bb5`), not
    re-measured by us (on macOS the layer moved in the page's frame);
  - the context menu over the picture, AOT builds of the layer route,
    minimise and restore, virtual desktops and the taskbar thumbnail on
    Windows, the mini player over another app's fullscreen Space on macOS;
  - on macOS Skathi's window drawables: five under the default frame
    source, three with `NextDrawable` (decided above); its pacing under
    load, and the frame source with the controls visible, in small windows
    and in the mini player, were not measured;
  - on Windows a real SDR desktop and a switch between SDR and HDR output
    while playing; the 8-bit path ran only forced on an HDR desktop;
  - HDR handling against the other players: the macOS layer takes its peak
    from the potential headroom and comes out brighter than the
    `<sk-surface>` route (3.29 against 2.19), IINA passes PQ through at 8.9×
    SDR white, and Frame Player's own HDR on macOS was not read in the same
    way; on Windows Frame Player and mpv map to a 1000-nit target, Media
    Player, MPC-HC and VLC hand HDR10 through. What "right" means for the
    player is not decided.
- **The interface and its runtime.** Neither route touches what is not
  the picture. On Windows both Frame Players are several times heavier than
  native players in process memory and CPU (the port 665 MiB private and
  9.7 % of a core at 1080p, bare mpv 211 and 3.5 %); on macOS idle and
  paused CPU is 4–9 points above the libmpv players. How much of that is
  the runtime, the interface's per-second work or the storyboard is not
  isolated.
- **The ports.** The remaining stubs (on Windows 90 commands: torrents,
  casting, the catalog, updates, diagnostics, the HDR status), the macOS
  menu; the console capture under AOT (#40's guard instead of our `try`);
  mpv's 7 extra 8-MiB blocks on macOS, which follow the port's
  presentation path (the render API's targets are a hypothesis);
  the decoder service's +71 MiB at 2160p on macOS, which is our libmpv's
  FFmpeg build; on Windows the `<sk-surface>` window going half float for
  SDR files too, because the port presents half-float frames for every
  file (an 8-bit path for SDR files was not built; #46's fix keeps the idle
  window 8-bit, re-measured); HDR content and an SDR → HDR file
  sequence with the 8-bit switches on macOS; values through `var()` visible
  only in the runtime log (#30); the trim after a burst of presents and
  hours (#38).
- **macOS AOT signing.** `--payload-only` now signs the image on a Mac
  (`025ff87c`), not verified by us; the `.app` layout and a hardened-runtime
  signature were not tried. #37 itself was closed by the maintainer; this
  is now our own check, and the beta's macOS build depends on it.
- **The lighter render set** (recommended above, not decided): the
  owner's verdict on the macOS real-content crops; real content on Windows;
  a P1 that keeps hermite `dscale`, measured in the Windows mini player;
  why the two systems rank P0 and P1 oppositely there; peak detection in
  the port; whether the memory kept after switching scalers live grows
  with more switches; the upscaler's memory kept after removal, to be
  released by rebuilding the video output (measured in the port on both
  systems, not checked in the player).
- **Rendering at the video's size, and the subtitles.** On Windows, a ring
  of 2 and a video-sized render bring a 1080p clip at screen size to 100–146 MB
  of private bytes below the shipping player and 58–76 MB of GPU memory above
  it. The cost was Skathi's bilinear scaling (Catmull–Rom on master since
  #34: fine detail 31.4 dB against 27.8) and blurred mpv subtitles and OSD,
  which the new scaler barely helps (18.4 dB against 17.0). Recommended after
  an offscreen analysis, before the ports (both alternatives were built and
  tested since, as the parenthesis says):
  - tested alternative 1, the element-sized fallback: render at the
    element size while a subtitle track, the second subtitle line or an mpv
    OSD message is on, switching on track state, never per
    cue. In the offscreen Vulkan run, the first render call after a size
    change took 17 ms of CPU the first time, then 2–3 ms; subtitled playback
    gives up the video-sized saving;
  - tested alternative 2, the split overlay: a second, element-sized
    overlay pass in the Vulkan render API,
    which libplacebo can already draw (`pl_render_image` with no image);
    proposed, about 150–250 lines, inferred. Its hard part is one transform
    for zoom, pan, crop and rotation in both passes. (Built since and
    measured in the macOS port: sharp subtitles and one transform, but in
    half float it cost 1426 MiB at screen size against 1242 for the
    element-sized fallback; with an 8-bit overlay and a ring of 1 it costs
    +123 MiB. See "Where the port's memory goes". On Windows the same shape
    needed an overlay target in our own libmpv's D3D11 render API, a
    290-line patch: +60 MiB of GPU memory while a subtitle shows, against
    +415 MiB for the element-sized render. See "The real interface on Skathi"
    under Windows.) On the native-layer route none of this is needed: the
    picture is the window's size and mpv draws the subtitles into it.

  Open on Skathi's side after #33: recheck the CPU placeholder and the actual
  swap-chain residency. The slot-to-atlas copy stays by design. The ~188 MB
  is the three-buffer half-float swap-chain step together with the HDR
  scene target, not in addition to it.

  The split is not confirmed with a per-allocation tool.
- **Present-to-screen on macOS.** Paused frames: solved by the hybrid
  `lowLatencySurface` (9.3 ms). During playback the display link's ~45 ms
  stays, which only `audio-delay` has to cover.
- **Colour against the shipping player**: on Windows an option set plus two
  host steps matches 27 of 33 patches, in our texture and, since #36's fix
  on master `64d2e42f`, on screen. Open: the display's gamut (the six SDR
  BT.2020 patches), an SDR desktop, colour outside BT.709 in an 8-bit
  window, under a tint or opacity and through an upscaled frame, and the
  same patch comparison on macOS. There the port's SDR picture in an EDR
  window came out lifted (black 18.1); the 8-bit SDR path puts black at 0.0
  and the grey 2 levels above Frame Player's. On the Windows native layer
  the mid-greys still differ by path (120 to 134 against Frame Player's
  121), with only the white level of the option set applied.
- **The Vulkan render API on macOS**: runs in the Skathi prototype at GL's
  cadence and colour, at 1.4–1.6× its GPU time and 73–93 MiB more
  footprint, and under the ported interface with the overlay pass. Open:
  the intermittent stale read in the harness's `delay` case (not the
  locked screen: it fails unlocked too), device loss, the queue-lock callbacks, a precompiled Metal kernel for the
  un-premultiply pass, a build against the player's FFmpeg, the bundle and
  licensing.
- **Sound against picture**: the slow 24 fps playback comes from stalls in
  the render call and the fence wait, not from mpv's turnaround; the
  proposed fix did not help. Open: whether the stalls happen on an idle Mac,
  and PR #18845's newer drift controller. `audio-delay` from a running median
  of report → `shown_at` works on screen; light and sound are not measured.
- **A Windows switch between SDR and HDR output while playing.**
- **Feedback accuracy**: `Estimated` events on Windows without the opt-in
  (they follow the window's foreground state, see #20), the
  macOS residues after 0.5.2, a real-mouse resize, multi-display setups.
- **Other hardware**: AMD and Intel GPUs, Windows 10, HDR off, other displays
  and refresh rates on the current Skathi, older macOS, anything across
  processes.
- **The shipping player's power and latency** with the web view (its true
  fullscreen, CPU, GPU time and cadence were measured in the two matrices).
- **Whether VRR is enabled on a given Windows setup.** The monitor's EDID range
  was 48–240 Hz, with what looks like a FreeSync block; NVIDIA's setting is not
  readable without its API.
- **Driver frame-rate caps.** Our Windows machine caps presents at 224 a
  second. How the recipe behaves under such a cap at high frame rates, and
  Skathi's latch fix (`20733eea`), would need the cap off. Not tested by
  choice.

## Next steps

The two prototypes from the earlier list are done, and so are the port on
both systems, the two matrices against the shipping player, the native
players, the native-layer prototype, the port on Skathi's own `<sk-layer>`
and the lighter render set (see "The prototypes on Skathi, measured" and the
sections after them). **The priority now is releasing version 2 as a beta**
beside version 1; the release plan is
[v2-beta-release.md](v2-beta-release.md). The items below are ordered by
what the beta needs first.

1. **What the beta's build takes from this document**: the frame source
   `NextDrawable` on macOS (decided); the two port fixes of 2026-11-23 (a
   redraw when the `<sk-surface>` element changes size while paused, and
   our `<sk-layer>` pool capped at two with unused imports released); the
   macOS AOT image signed and run from a `.app` with a hardened-runtime
   signature (`025ff87c`, not verified by us).
2. **The lighter render set**: the owner's verdict on the macOS crops;
   a P1 with hermite `dscale` in the Windows mini player (zone plate and
   memory); real content on Windows through the prepared one-command
   script; then the decision, and with it the settings choice and its
   strings in all three languages.
3. **Decide between `<sk-surface>` and `<sk-layer>`.** Before deciding:
   the Windows resize re-measured after Skathi's commit-order change
   (`beab4bb5`), without our own `Commit`; the context menu over the
   picture; the layer route as an AOT build; on Windows a real SDR desktop
   (switched by hand) and the SDR ↔ HDR switch while playing; on macOS the
   frame source with the controls visible and in the mini player.
4. **HDR handling, against the other players**: what the player should do
   with PQ on each system (EDR with headroom like IINA and AVFoundation,
   HDR10 passed through like Media Player, MPC-HC and VLC, or mapped to a
   fixed target as mpv and today's player on Windows), with Frame Player's
   own macOS output read the same way, and the macOS layer's peak taken from
   the potential headroom compared with the `<sk-surface>` route's.
5. **The ports, continued**: the 8-bit SDR switches with HDR content and an
   SDR → HDR file sequence on macOS; an 8-bit path for SDR files on the
   Windows `<sk-surface>` route; pages without a framework mount through
   `mountDocument()` (#43, item 7); the console capture under AOT through
   `Runtime.isAot()` (#40); the remaining stubs and the macOS menu; the
   interface's per-second work, the one cost neither route touches.
6. **The Windows libmpv**: built, pinned and reproducible, with libxml2 for
   DASH (decided), now carrying the overlay target as well. Left: the host's
   two colour steps in the real path, and whether the overlay patch is kept
   (the native layer would not need it).
7. **The Vulkan render API for macOS**: the stale read behind the
   harness's intermittent `delay` failures (a variant without the gate and
   one with the copy on mpv's queue; the lock is ruled out), since
   Skathi's copy follows the same pattern; device loss, the queue-lock
   callbacks, a precompiled `.metallib` for the un-premultiply pass, a
   build against the player's FFmpeg, and the bundle and licensing.
8. **Upstream**: report the frame-source measurements on #49; re-measure
   #44's Windows commit order and report it there. #35–#49 are closed, and
   skathi-gpui pull request #1 is merged.
9. **The 24 fps audio on macOS**: find the render-call and fence stalls (an
   idle Mac, a trace of the render thread), try PR #18845's drift
   controller, and take `audio-delay` from the running median.
10. Compare with today's player through the e2e harness (on the `host-boundary`
    branch: `bun run e2e --mpv-conf <file>`, per-step frame metrics,
    `bun run e2e compare`).
11. **Previews and posters through a second libmpv core**, in the order given
    under "Previews and posters (planned)".
12. **Near the end, two checks deferred on purpose**:
    - the colour table on an SDR Windows desktop (Windows HDR off, switched
      by hand), for viewers without HDR;
    - the official container build of libmpv (WSL) against our native one:
      the same options, its extra dependencies, colour and decoding.
13. **Later, not now: version 1.** What this work found that the shipping
    player could use (the lighter set, `swapchain-depth=1` and
    `hwdec-threads=1` on Windows, the upscaler's memory, issue #81) is
    listed in [v1-backports.md](v1-backports.md), to be verified and ported
    together once the beta is out.

## Reproducing the probes

Throwaway programs, not kept. Enough to rebuild each of the earlier probes and
the Vulkan offscreen proof; the two prototypes are described by their sections,
not reconstructed here:

- **IOSurface + EDR (macOS, Objective-C, ~1 file):** load the bundled
  `src-tauri/lib/libMoltenVK.dylib` directly (`vkGetInstanceProcAddr` from it,
  no loader), headers from Homebrew `vulkan-headers`. Create a 1920×1080
  IOSurface `'RGhA'`, import it with `VkImportMetalIOSurfaceInfoEXT` into a
  dedicated-allocation image, clear bands 0.0625…4.0 with `vkCmdClearAttachments`,
  read back under `IOSurfaceLock(kIOSurfaceLockReadOnly)`. Show via a layer
  with the IOSurface as contents; log `NSScreen` EDR values while pumping AppKit
  events; capture with ScreenCaptureKit in HDR.
- **Vulkan → Metal ring (macOS):** as above plus a timeline semaphore exported as
  `MTLSharedEvent` (one per IOSurface), a Metal consumer doing
  `encodeWaitForEvent(2n+1)` → blit from `newTextureWithDescriptor:iosurface:plane:`
  into one of 4 own textures → compute kernel that verifies every pixel against
  the frame number encoded in it → `encodeSignalEvent(2n+2)`. Slow the consumer
  by spinning to expose the single-event bug.
- **Vulkan offscreen proof (macOS, Objective-C, two binaries):** link the
  public MoltenVK 1.4.2 release dylib directly (no loader, `VK_KHR_portability_subset`
  enabled), request Vulkan 1.2 with `timelineSemaphore` and `hostQueryReset`.
  Import a 64×32 `'RGhA'` IOSurface with `VkImportMetalIOSurfaceInfoEXT`, with
  and without an explicit `vkBindImageMemory`; make one timeline semaphore,
  either exported as `MTLSharedEvent` or imported from one made in Metal. One
  binary clears with a render pass, the other draws through libplacebo 7.398.0
  (`pl_vulkan_import`, `pl_vulkan_wrap`, `pl_vulkan_release_ex` /
  `pl_vulkan_hold_ex`, queue family `IGNORED`). Write (2.0, 0.5, −0.25, 1.0),
  signal 1; a Metal queue waits for 1, blits the IOSurface texture to a
  CPU-visible buffer and signals 2; a Vulkan submission waits on 2. Compare
  the buffer and the locked IOSurface bit for bit. No validation layers, one
  frame, no second write.
- **Vulkan/D3D11 → D3D12 (Windows, Rust with `windows` and `ash`):** D3D12
  consumer creates 3 shared RGBA16F resources and fences, exports NT handles;
  the producer imports them (see "Windows: Vulkan → D3D12"), renders values
  including 1.0, 2.5, 12.0, −0.1 and the frame number, signals `2n+1`; the
  consumer queue `Wait`s, copies, signals `2n+2`, and reads back every N frames.
- **mpv colour numbers (Windows):** standalone libmpv in its own window with
  `d3d11-output-format=rgba16f` and the target options above; capture the
  composed desktop in FP16 (Desktop Duplication's `DuplicateOutput1` with
  `DXGI_FORMAT_R16G16B16A16_FLOAT` is one way) and read flat regions of `testsrc2` and of
  a PQ clip made with libx265 (`master-display`, `max-cll=1000,400`).
- **D3D11 render API (Windows, Rust):** load `libmpv-2.dll`, create a D3D11
  device (not single-threaded), `mpv_render_context_create` with
  `MPV_RENDER_PARAM_API_TYPE` = `"d3d11"` and param 21 → `{device}`, then per
  frame param 22 → `{tex}` and param 12 (block) = 0; read the
  `R16G16B16A16_FLOAT` target back through a staging texture. For sharing,
  create it with `D3D11_RESOURCE_MISC_SHARED_NTHANDLE` plus one shared
  `ID3D11Fence` per ring texture, open both on a D3D12 device.
- **mpv backend lists and logs:** libmpv with `terminal=yes`, `msg-level=all=v`;
  `option-info/*/choices` for compiled choices; the swapchain lines come from
  `vo/gpu-next/libplacebo` and `vo/gpu-next/d3d11` (or `/mac`).
