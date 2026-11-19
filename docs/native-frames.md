# Handing mpv's frames to a native UI framework

What was measured and agreed in November 2026 while evaluating a move from
today's shell (a transparent WebView with mpv drawing into a child window behind
it, see [architecture.md](architecture.md)) to [Skathi](https://github.com/argon-chat/skathi),
a native UI framework that draws the interface itself and takes video as a GPU
texture through its `<sk-surface>` element. The player is not built yet; two
throwaway prototypes are, one per platform (see "The prototypes on Skathi,
measured"). The point of this document is that the implementation starts from
these findings instead of re-measuring them.

Every number below comes from a probe that was run. The prototypes' sections
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
  CPU wait for the slot), now with the gpu-next renderer selected. Skathi stays
  on its D3D12 renderer and opens the textures there. This needs a Windows
  libmpv build of our own (the prebuilt one cannot carry the PR); the PR's CI
  artifacts are for testing only.
- **macOS: a Vulkan render API, by a patch of ours.** On top of PR #18845, a
  `vulkan` backend for the render API: the client supplies a `VkImage`
  imported from an IOSurface (`VkImportMetalIOSurfaceInfoEXT`) and a timeline
  semaphore exported as the `MTLSharedEvent` Skathi waits on; libplacebo
  renders through MoltenVK, i.e. Metal underneath. Offscreen direct-IOSurface
  image and timeline-event interop passed on MoltenVK 1.4.2 for one frame
  (below); the libmpv backend itself is future work.
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

## The prototypes on Skathi, measured

Built and measured in November 2026, on Skathi 0.4.0 and then on its master
up to 0.5.0 (with `skathi-gpui` 0.5.2), as each fix landed. Each platform got
a standalone prototype: an `<sk-surface>` fed by today's libmpv through its
render API, with a small test page over it. Neither prototype is the player.
Their interface is a minimal page, and the real interface's cost is not
measured. The prototypes are throwaway programs and are not in the repository.

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
| Frame step, paused | lower medians on the new path (96–99 vs 46–49 ms), cause untested | run medians 99–101 vs 83–95 ms; single commands overlap (86–106 vs 50–117 ms) |
| Back step, paused | overlapping cell medians (83–94 vs 88–95 ms) | run medians 161–198 vs 216–243 ms (+19…+83 ms by run order) |
| CPU, 24 fps SDR | 2.5 → 4.5 % of a core | 7.9–10.0 → 15.1–29.2 % (old `gpu` renderer, capture running) |
| GPU, 24 fps SDR | 0.4 → 1.0 % | 3.8–4.4 → 9.1–10.6 % (capture running; not comparable across OSes) |
| Memory | private bytes against the shipping player, sizes unmatched: in a window 76–182 MB lower at 1080p, −230…+45 MB at 2160p; screen-sized +164…+213 MB at 1080p, −54…+225 MB at 2160p | `phys_footprint`: 221–225 MB above the bare video path at 1080p, same session; against an unmatched earlier whole-player observation (~845 MB), about 90–140 MB lower |
| GPU memory against the shipping player | +81…+172 MB in a window, about +500 MB screen-sized | not separated |
| Audio | no drift over 60 s by mpv's counters; about +6 ms `audio-delay` inferred, not tested | audio clock minus captured picture +51.8…+53.0 ms (old path +9.1…+11.1); `audio-delay=+0.050` tested in this setup; 24 fps plays 0.1–0.125 % slow |
| gpu-next through PR #18845 | works on D3D11 (CI build); colour differs from `gpu` with the same options | initialises, decodes and renders on GL 4.1; a scaler shader error once per file |

On Windows the new path put more frames on their intended refresh in every
cell. On macOS the clear difference is 23.976 fps, and the old path's
run-to-run spread at 60 fps; at 24 fps SDR and HDR10 the ranges overlap. Paused
latency: on Windows the seek and back-step medians overlap and the forward
step's medians are lower on the new path, for an untested reason; on macOS the
new path's run medians are higher for seeks and back steps. Stage MM reported
44.9–48.0 ms median from present to `shown_at`; it may explain part of that.
The new path costs more CPU and GPU. Memory depends on the window size: on
Windows the prototype's private bytes rise by about 320–370 MB and its GPU
memory by 580–610 MB from the 1280×720 window to screen-sized, whatever plays.
Which allocations make that step is the largest open question about cost.
None of these comparisons supports a production memory saving.

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
- **The frame-step difference is unexplained.** It is close to two frame
  durations against one. A guess, not measured: the step follows mpv's sync
  mode (`audio` on the old path, `display-resample` on the new). It must not
  be read as a gain of the Skathi path; a matched sync-mode comparison is
  listed under "Not measured".
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
  was resized from outside, and its true fullscreen is not measured.
- **The prototype's interface is a minimal test page.** The real interface
  drawn by Skathi will add memory that is not measured.
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
  - Which allocations make this step, Skathi's or others, is not measured.
    Most of the screen-sized difference against the shipping player is this
    step.
- A ring of 2 instead of 3 saves one texture: 16 MB of GPU memory in the
  window and 64 MB screen-sized, nothing in private bytes (n = 3).

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
- Which options would bring `gpu-next` to the `gpu` values was not tried.
- A reading, not established: libplacebo models an SDR display's black level
  and maps HDR10 with its own curve.

#### Not measured

- The shipping player:
  - its true fullscreen (there is no way in without synthetic input);
  - its CPU, GPU, power, cadence and latency with its web view.
- The real interface drawn by Skathi: not built, so neither its memory nor its
  cost is known.
- `exact_frame_feedback`: its CPU, GPU and power cost; the option at other
  refresh rates; the option with a ring of 2.
- The frame-step control: the old path with `video-sync=display-resample`, or
  the new path with audio sync.
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
- Which allocations make Skathi's window-size memory step.
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
  The cause is a hypothesis: Skathi's maintainers think macOS "most likely"
  (#18); the capture cannot separate it from Skathi's switch.

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
minimal: **the real interface's cost is not measured**.

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
- The recipe plays **24 fps 0.1–0.125 % slow** (audio resampled). mpv reads
  119.3–119.6 Hz from our swap reports despite `display-fps-override=120`;
  that our report timing is the cause is an inference.

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

#### The Vulkan render API (research, not built)

**Verdict: proceed** with a downstream gpu-next-only Vulkan backend on MoltenVK:
app-owned device, RGBA16F IOSurface pool, one timeline semaphore/event per slot.

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
- **Size, inferred**: 350–600 lines in 6–9 mpv files, 250–450 app-side, on top
  of PR #18845. **Risks**: cross-queue teardown (MoltenVK #2697 and #2859 are
  open reports of related risks, not diagnoses of this prototype), colour at
  the compositor, VideoToolbox NV12/P010, footprint (a 2160p slot is ~63 MiB).
- Queue-family ownership and image layouts are provisional until the ring
  runs under validation.
- **Skathi caveat** (source, `aa232d0e`): the atlas path clamps RGBA16F to BGRA8;
  an HDR branch keeps extended values. Confirm which runs in the target build.

#### Not measured

Light on the panel; other displays, rates, brightness; a real-mouse resize; a
second surface; device loss; acoustic output; 4K and 60 fps HDR cadence; steps on
0.5.x; the shipping player itself in these runs; power; macOS before 26.

### Skathi defects found, and their state

Filed in github.com/argon-chat/skathi while building the prototypes. #1–#20
are closed, most of them within hours; #21 and #22 are open. "Re-measured"
means our own runs confirmed the fix, on the build named; "landed" names the
upstream commit, which is not always the build we tested.

- **#1** `<sk-surface>` was inert without the cargo feature `video`. Fixed (`ae8dca2c`), re-measured on `1eacd896`.
- **#2** `sk types --exe` refused an optimised executable. Fixed (`d9f84387`).
- **#3** All surfaces shared one process-wide copy queue, so a fence that never arrived froze every surface. Fixed by the maintainer (`43da7b21`, per-surface queue and release fence, with their own regression test); not re-measured by us (the prototypes have one surface).
- **#4** The documentation said half-float frames were clipped to 8 bits. The documentation was fixed (`711649e2`). A separate follow-up (`aad939ae`) sets `preferredDynamicRange`/`contentsHeadroom` and decides the HDR switch by the screen's potential headroom, so EDR can start on macOS without a primer; re-measured on `1eacd896`, macOS 26.5.2.
- **#5** Svelte threw `customElements is not defined` on `<sk-surface>`. Fixed; the shim is gone from `1eacd896`.
- **#6** Nothing said whether a window draws HDR. `output_hdr` was added; on macOS it equalled "the layer is `RGBA16Float`" in 102 063 of 102 063 probes over 48 runs (MM).
- **#7** A host built with devtools opened the devtools port although the payload was published with `publishDevtools: false`. Fixed (`bc43e91e`): such a host now starts with devtools off; a build with the `no-devtools` feature contains none. Re-measured on macOS (MM).
- **#8** The macOS EDR layer was tagged with the display's colour space. Fixed (`8a1f1728`), re-measured 3 of 3 (MM): the layer is now tagged extended sRGB.
- **#9** A surface showed nothing while the newest frame was in flight. Fixed (`c68618ad`). Before the fix, at exactly one present per refresh, the loss on Windows depended on the phase (61 % of frames just after the vblank, 8 % at 0.3–0.9 of the refresh). A post-fix figure of 6–10 % lost at one present per refresh was noted on Windows without its build, phase or run count; treat it as unverified. The recipe's at most one present per refresh, started by the formula above, comes from the pacing work of #15.
- **#10** The documented mpv recipe (`report_swap` from `on_frame`) could not work. The page was rewritten along the recipe above.
- **#11** The macOS EDR switch produced a glitch frame and a flood of `DisplayChanged`. Fixed, re-measured (MM): no glitch frame captured in 21 switches, 10 of them with full capture coverage near the switch.
- **#12** A ring of 4 or more gave permanent `Busy` in an HDR window on D3D12. Fixed, re-measured on `1eacd896`.
- **#13** `last_vblank` was the coming vblank and went stale. Fixed (`0a158b12`, then `9f24eb0c`), re-measured on `74e63fad`: p95 14.5–19.0 µs off the grid over 5 runs. `9f24eb0c` also carries the last known vblank forward after a refresh-rate change; we did not re-test a rate change on the current build. Before any vblank is known the value can be `None`; the producer then waits one `refresh_period`.
- **#14** Covered, minimised or display-off windows reported frames as shown. Fixed (`2706f963`, `e373486f`). Still open, not filed: `Hidden` starts 30–169 ms late under a cover that does not take the foreground.
- **#15** At 24 fps frames came a refresh late and were labelled `Estimated`. The vblank clock (`9f24eb0c`) and the recipe changes (docs `74e63fad`) fixed the drift. Our patch landed upstream as the opt-in `exact_frame_feedback(true)` (`810843bd`, documented in `02dfa33d`) and was re-measured on `c7a524d5` above, on one display. It is off by default because it costs about one refresh of latency.
- **#16** A removed D3D12 device was never replaced or reported. Fixed (`3f948c3e`), re-measured on `6cd67304` with 24 injected `RemoveDevice` removals and the producer's own D3D11 device intact: `SurfaceEvent::DeviceLost` came 16.6–33.6 ms after the removal and again 471–506 ms after it, when the replacement device was ready; video was back 690–708 ms after the removal. On this machine the old shared fences read `UINT64_MAX` for good and gave `Busy` when reused, so the producer makes new fences after a device loss; the maintainer's machine kept the last value, and Skathi's documentation (`02dfa33d`) now says the fence breaks "on some systems". During the ~0.7 s the window shows the desktop behind it. A real driver reset was not tested.
- **#17** A refused GPU frame's reason was promised in the log but not printed. Fixed (`71f66833`): each surface logs a warning once per reason. Verified in source only: in our device-loss runs presents were refused before paint, so the line never appeared.
- **#18** On macOS the HDR clamp climbed the EDR ramp in 200 ms steps. Fixed (`12f7f88e`) for the clamp Skathi paints with, re-measured on `c602b5a4`. `content_headroom()` and `DisplayChanged` still move in coalesced steps, so a producer that tone-maps to it still steps. The ~110 ms hold when EDR starts from 1.0 is "most likely" macOS by the maintainer's reading; our capture cannot tell the two apart.
- **#19** On macOS, frames shown at sparse presents or during a resize were reported `Superseded` or late. Fixed through `skathi-gpui` 0.5.2 (`c602b5a4`, `810843bd`, `77030d59`, `f2a79987`), re-measured on each in the scripted cases, with the residues listed above.
- **#20** Closing a window logged `[ERROR]` lines. The maintainer reports them gone for both `close()` and a posted `WM_CLOSE` (`skathi-gpui` `186ed842`). Our last run predates that and still logged one line at every `Handle::close()`; posting `WM_CLOSE` then logged 5–6 `Invalid window handle` lines. Not re-measured, and a click on the close button was never measured.
- **#21, #22** (open) `sk check` misses CSS that the runtime drops or flags. Filed from the check of moving the real interface to Skathi, which is a separate piece of work.

Also not filed: 1–2 frames still in flight at close get no event (10 of 13 closes on Windows); the git dependency on Skathi needs the devtools bundle built from a clone.

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

1. **A gpu-next render backend for Vulkan.** The client supplies a `VkImage`
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
The current estimate, for a Vulkan backend on top of the PR, is 350–600 lines in
mpv plus 250–450 app-side; see "The Vulkan render API".)

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
  contract: queue ownership and image layout stay provisional until the ring
  runs under validation. The offscreen proof used `VK_QUEUE_FAMILY_IGNORED` with
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
  to `R10G10B10A2` + **G2084/P2020 (HDR10)** — for SDR content too — and passes
  an HDR10 clip through at 1000 nits. `target-colorspace-hint=no` tone-maps to
  SDR at the SDR white level (target bt.709 / srgb / 240). `vo=gpu` does not
  follow the hint and stays 8-bit SDR (bt.709 / gamma2.2 / 203).
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

### The window has to be opaque on Windows

Exact timestamps come only from `CreateSwapChainForHwnd`, i.e. an opaque window.
(Superseded in part: with `exact_frame_feedback(true)` every event was `Exact`
in the Windows prototype's window on `c7a524d5`, at the cost of about one
refresh. The prototype's window flags are not recorded here, so check them
before making opacity a requirement.)
Ours is transparent only so that mpv's child window shows through the WebView;
once Skathi draws the video, that reason goes. Before asking for an opaque
window, check what else assumes transparency: the mini player, rounded corners,
and the "nothing painted means desktop visible" rule in
[rules/window.md](rules/window.md).

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
- macOS: until the Vulkan render API patch exists the preview core runs on the
  OpenGL interim path, with gpu-next's compute shaders missing there.
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

- **The real interface on Skathi.** Its memory, its cost, and what of today's
  Svelte interface runs unchanged. A separate check has started; `sk check`
  already misses CSS the runtime drops or renders differently (Skathi #21,
  #22).
- **The prototype's memory step with the window size** (about 320–370 MB
  private and 580–610 MB GPU from 1280×720 to screen-sized on Windows): which
  allocations, whose they are, and whether they can shrink.
- **gpu-next colour against `gpu`** with our options (the table above): which
  options give the intended values, on both platforms.
- **The Vulkan render API on macOS**: researched and proven offscreen for one
  frame, not built. See "The Vulkan render API" above for its risks.
- **Sound against picture** measured with instruments, and whether a fixed
  macOS `audio-delay` (+50 ms in the tested configuration) becomes a default.
- **The frame-step difference on Windows** (46–49 against 96–99 ms), which is
  unexplained; a matched sync-mode and renderer comparison would tell.
- **A Windows switch between SDR and HDR output while playing.**
- **Feedback accuracy**: `Estimated` events on Windows without the opt-in, the
  macOS residues after 0.5.2, a real-mouse resize, multi-display setups.
- **Other hardware**: AMD and Intel GPUs, Windows 10, HDR off, other displays
  and refresh rates on the current Skathi, older macOS, anything across
  processes.
- **The shipping player's true fullscreen**, and its CPU, GPU and power with
  the web view.
- **Whether VRR is enabled on a given Windows setup.** The monitor's EDID range
  was 48–240 Hz, with what looks like a FreeSync block; NVIDIA's setting is not
  readable without its API.

## Next steps

The two prototypes from the earlier list are done (see "The prototypes on
Skathi, measured").

1. **The real interface on Skathi**: `sk check` over today's frontend, a
   payload built from it with the Tauri calls stubbed, and its memory and
   start-up cost against the prototypes' minimal page.
2. **A Windows libmpv build of our own** carrying PR #18845, with the colour
   options matched so that gpu-next gives the intended values.
3. **The Vulkan render API patch for macOS**, in the order the research gives:
   - a ring of slots under Vulkan and Metal validation layers, comparing the
     two image-import paths (direct IOSurface and `VK_EXT_external_memory_metal`,
     each on a pinned build), with a second write after each Metal release,
     delayed completion, a failed submission, and queue drain before a
     generation is retired;
   - the backend on the PR, with a minimal offscreen client;
   - VideoToolbox input;
   - the connection to Skathi's fenced path, with paired producer and
     compositor readbacks, HDR patches and an SDR control, so that the HDR
     branch is known to be the one tested;
   - the bundle and licensing.
4. **Skathi's window-size memory step**: find the allocations, and report it
   upstream if it is theirs.
5. Compare with today's player through the e2e harness (on the `host-boundary`
   branch: `bun run e2e --mpv-conf <file>`, per-step frame metrics,
   `bun run e2e compare`).
6. **Previews and posters through a second libmpv core**, in the order given
   under "Previews and posters (planned)".

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
