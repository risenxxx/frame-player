# Handing mpv's frames to a native UI framework

What was measured and agreed in November 2026 while evaluating a move from
today's shell (a transparent WebView with mpv drawing into a child window behind
it, see [architecture.md](architecture.md)) to [Skathi](https://github.com/argon-chat/skathi),
a native UI framework that draws the interface itself and takes video as a GPU
texture through its `<sk-surface>` element. Nothing here is built yet. The
point of this document is that the implementation starts from these findings
instead of re-measuring them.

Every number below comes from a probe that was run, and the conditions are given
next to it. The probes themselves were throwaway programs and are not in the
repository; "Reproducing the probes" at the end says enough to rebuild each one.
Two machines were used: an Apple M3 Max laptop (built-in XDR display, 120 Hz)
and a Windows desktop (build 28000) with an RTX 5080 (driver 610.62) and a Gigabyte MO27U2
monitor (3840×2160, 240 Hz, Windows HDR on, SDR white level 240 nits, reported
peak 1000 nits).

## The shape of the design

```
FFmpeg decode → mpv (gpu-next = libplacebo, on Vulkan)
             → VkImage that is really a shared texture (IOSurface / D3D12 resource)
             → Skathi surface: GPU wait, copy, map, draw into its window
```

- **mpv stays.** libplacebo only turns a decoded frame into pixels; mpv is the
  player around it — A/V sync, audio output, exact seeking and stepping, libass
  subtitles, the demuxer cache that torrent streaming leans on, `mpv.conf`,
  scripts, the property/event API the whole interface is built on. A player of
  our own on FFmpeg + libplacebo (libplacebo's `plplay` demo is the starting
  point people quote) would have to rewrite all of that. gpu-next already *is*
  libplacebo, so if Skathi uses libplacebo for its own HDR output later, the
  colour maths on both sides of the hand-off is the same library.
- **Vulkan on both platforms.** libplacebo has D3D11, Vulkan and OpenGL
  backends and no D3D12. Skathi's default Windows renderer is D3D12, and its
  documented Vulkan route is "create on D3D12, import into Vulkan". On macOS mpv
  already renders through Vulkan (MoltenVK, context `macvk`). One render-API
  backend for Vulkan therefore covers both platforms; a D3D11 producer works as
  a fallback but blocks its thread (see "Synchronisation").
- **Windows moves from D3D11 to Vulkan for video.** Measured on the RTX 5080
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

### What has to change in libmpv

Today's bundled libmpv cannot do any of this. Its render API runs the old
`vo=gpu` renderer, not gpu-next, and accepts only `MPV_RENDER_API_TYPE_OPENGL`
and `"sw"`; `"d3d11"` is refused with -4 (Windows, mpv v0.41.0-1303,
libplacebo v7.387). We need a patch, on top of the stack `docs/mpv-patches.md`
describes:

1. **A gpu-next render backend for Vulkan.** The client supplies a `VkImage`
   (plus its device, queue family and a timeline semaphore); libplacebo renders
   into it. Upstream state as of November 2026: PR #16818 (gpu-next render
   backend) is a draft limited to OpenGL with an sRGB-only target, an empty
   end-of-frame hook (no `report_swap`), and no `set_parameter`/`get_image`;
   maintainers called it a mess and asked for it to be done "like vo_gpu". The
   follow-up RFC #18607 is open, and the maintainer response to the IINA
   developer's proposal — a separate `MPV_RENDER_PARAM_API_TYPE` per backend
   rather than one switch — was "looks sane". A Vulkan API type written that way
   is something upstream may take. The OpenGL route is closed for us anyway:
   our macOS libmpv refuses `gpu-api=opengl` and `gpu-context=cocoa` (-7).
2. **`report_swap` with a timestamp.** `mpv_render_context_report_swap()` takes
   no time: mpv assumes the frame was shown at the moment of the call. Skathi
   reports presentation after the fact (below), so the patch must accept
   `shown_at` and the refresh interval, and know about dropped frames, or
   display-resample timing will be wrong.
3. **A Windows build of libmpv of our own.** Today Windows uses a prebuilt
   libmpv and only macOS applies patches. This is the same prerequisite every
   patch in `docs/mpv-patches.md` has.

Size estimate for (1)+(2): on the order of PR #16818 itself, 1–2 thousand lines.

## Skathi's frame contract

From Skathi's documentation (`/embedding/native-frames/`, `/embedding/gpu-frames/`,
`/embedding/gpu-frames-macos/`) and its source at commit `8ab3a3d` with the
`vendor/gpui` submodule at `6b741ca`. MIT OR Apache-2.0. Paths below are in
those repositories.

- **API.** `skathi::surface(name)` → `SurfaceHandle` (`Send + Sync`).
  `surface.set_format(SurfaceFormat::Rgba16Float)` once, before presenting.
  `SharedTexture::open(handle)` / `from_iosurface(IOSurfaceRef)`, then
  `.with_fence(handle)` (Windows) or `.with_shared_event(event)` (macOS), and
  `present_shared_fenced(&shared, wait, release)`. `info()` gives the element's
  size in physical pixels, `visible` and `display`; `on_change` reports
  `Changed`, `DeviceLost` and `DisplayChanged`.
- **Formats.** Windows: `R16G16B16A16_FLOAT` (or `_TYPELESS`) on every GPU path.
  macOS: `RGBA16Float` textures, IOSurface `'RGhA'`, CVPixelBuffer
  `64RGBAHalf`. Alpha is straight, not premultiplied.
- **Colour.** Linear, BT.709 primaries, **1.0 = SDR white** on every platform;
  values may exceed 1.0 or go below 0. Skathi converts to scRGB itself
  (scRGB = value × `sdr_white_nits` / 80).
- **What happens on present** (`crates/sk-tree/src/surface/d3d12.rs` `copy_in`
  754–850, `metal.rs` 528–639): on Skathi's copy queue, GPU-wait the producer's
  fence for `wait`, copy into one of up to 4 surface textures (`MAX_SLOTS`),
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
- **A `wait` value that never arrives stalls Skathi's shared copy queue for
  every surface** on D3D12 and Metal. The producer must signal `wait` even when
  mpv failed to render the frame.
- **Slot reuse is lazy on D3D12:** a finished slot is reclaimed inside
  `commands()`, which runs after `free_slot`, so it frees one present late.
  Whether a ring of three is enough against the real thing has to be checked on
  the prototype.
- **Today's window is SDR.** Swapchains are `B8G8R8A8_UNORM` (Windows, 3
  buffers, `FLIP_SEQUENTIAL`, `Present(0)`, paced by `DwmFlush`) and
  `BGRA8Unorm` (`CAMetalLayer`, 3 drawables, `CAMetalDisplayLink` on macOS 14+).
  No `SetColorSpace1`, no EDR. RGBA16F frames are clamped to 0…1 and
  sRGB-encoded (`surface/color.rs`); `output_headroom` is always 1.0.
- **Display info** (`surface/display.rs`): Windows reads SDR white and HDR state
  from `QueryDisplayConfig` once a second and `IDXGIOutput6::GetDesc1`; macOS
  reads `NSScreen` EDR values and `maximumFramesPerSecond`. `headroom` =
  peak / SDR white. Windows reports no variable refresh.
- **Linux:** surfaces take CPU frames only (no dmabuf). Moving to Skathi would
  cost the `feature/linux` branch its zero-copy render-API path.

### Frame events (agreed with Skathi's maintainers, November 2026)

Not in commit `8ab3a3d`; announced as the design to implement. Check the shipped
version against this list.

- Every `present_*` returns the frame number (`u64`).
- Every present that returned `Ok` ends in **exactly one** event:
  - `SurfaceEvent::FramePresented { frame, shown_at, refresh_interval, precision }`.
    Sent once, at the frame's first refresh; how many refreshes it stayed is
    the next frame's `shown_at` minus this one's.
  - `SurfaceEvent::FrameDropped { frame, reason }` with `Superseded` (a newer
    frame arrived before it was shown — sent immediately), `Hidden` (window
    minimised or `info().visible` false — pending frame dropped then, and every
    present while hidden is dropped at once), `DeviceLost` (before
    `surfacelost`), `Closed`.
- `shown_at` is in mpv's own clock: QPC on Windows, `mach_absolute_time` on
  macOS. `refresh_interval` is the measured interval from the previous vblank to
  the one the frame was shown on, so it is real under VRR; the nominal period
  stays in `info().display`.
- `precision: Exact | Estimated`. A normal window (`CreateSwapChainForHwnd`)
  reads `SyncQPCTime` from `GetFrameStatistics`: exact. A transparent window
  (`CreateSwapChainForComposition`) has no frame statistics; Skathi falls back
  to the first compositor vblank after the GPU finished the present (±1
  refresh), unless `IPresentationManager` works out. macOS uses
  `addPresentedHandler`; `presentedTime == 0` becomes a drop.
- The callback is a `Send` closure on the surface handle, called from Skathi's
  vsync thread, in frame order, at most one vblank after the show. A hung UI
  thread is the only thing that delays it.
- One thing to confirm: gpui presents with `Present(0)`, and with the flip model
  DWM discards a present that a newer one overtakes before the vblank. A
  surface frame inside a discarded window present must come out as
  `Superseded`, which holds if `Presented` is derived from `PresentCount`.

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
4. Always signal `wait`, even when rendering failed (see the copy-queue stall
   above).
5. A step or a pause is complete when `FramePresented` arrives for that frame
   number. `Superseded` means present it again.
6. On `Hidden`, stop calling mpv's render; audio continues. Render the current
   frame once when visible again.
7. On `DeviceLost`, reopen the shared textures and rebuild the ring. mpv's own
   Vulkan device is not affected.
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
- **The D3D11 fallback** (`ID3D11Device5::OpenSharedResource1` +
  `OpenSharedFence`, `ID3D11DeviceContext4::Wait/Signal`) is also bit-exact
  (203 µs per frame, round trip 358 µs), but `Signal` **blocks the CPU thread
  for about 4.2 ms** while a queued GPU wait is unsatisfied. A D3D11 producer
  runs at the consumer's pace.
- A D3D11 ↔ D3D11 keyed-mutex share also works (values above 1.0 survive, 200
  hand-offs at ≤ 400 µs each including a 16 MB readback), but keyed mutexes do
  not fit D3D12; it is not the path.

### macOS: Vulkan → Metal (MoltenVK 1.4.2, Vulkan 1.3.357)

- **Import the IOSurface with `VkImportMetalIOSurfaceInfoEXT`**
  (`VK_EXT_metal_objects`). Exporting the image back returns the same
  IOSurface-backed `MTLTexture` (RGBA16Float, 15 360 bytes per row). Readback
  matched exactly: 0 mismatches in 8 294 400 half-floats, before and after 200
  more draws.
- **Do not use the `VK_EXT_external_memory_metal` MTLTexture import.** Every
  call succeeds and the draw succeeds, but MoltenVK 1.4.2 renders into a
  texture of its own; the IOSurface reads back all zeros. Nothing reports it.
- **The shared event works in both directions:** a timeline `VkSemaphore`
  exported as `MTLSharedEvent` (`VkExportMetalSharedEventInfoEXT`), or an event
  made in Metal imported with `VkImportMetalSharedEventInfoEXT`. Needs only
  `VK_EXT_metal_objects` and the `timelineSemaphore` feature. Latencies: Vulkan
  signal → Metal wait 0.28–0.33 ms; Metal signal → Vulkan GPU wait 0.13–0.25
  ms; → `vkWaitSemaphores` 0.06–0.22 ms. Lower values are ignored.
- **No barriers are required** for coherence (none, layout transitions only, and
  a queue-family release to `VK_QUEUE_FAMILY_EXTERNAL` all gave 0 bad pixels).
  Keep the release to external in `GENERAL` anyway, for portability.
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

### What mpv does with the target options (Windows, measured)

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

What to configure:

- **While Skathi's window is SDR (output headroom 1):**
  `target-trc=linear target-prim=bt.709 hdr-reference-white=203`. Skathi's
  documentation suggests `target-peak = 203 × output_headroom` with `linear`;
  that does nothing, and without `hdr-reference-white` white lands at 1.18.
- **Once Skathi outputs HDR (headroom > 1):** `target-trc=scrgb target-prim=bt.709
  target-peak=<display peak nits> hdr-reference-white=<sdr_white_nits>`, then
  scale by 80 / `sdr_white_nits` (÷3 at 240 nits) to get Skathi's 1.0 = SDR
  white: SDR white → 0.999, 1000 nits → 4.05. mpv cannot apply that scale
  itself (a user shader was not tried), so either the producer does it, our
  mpv patch makes `linear` honour the peak, or Skathi accepts absolute scRGB as
  a second colour space.
- **Always set `hdr-reference-white` explicitly.** `auto` depends on mpv having
  a window on the display, which it will not.
- `DISPLAYCONFIG_SDR_WHITE_LEVEL` (via `DisplayConfigGetDeviceInfo`) read 3000 →
  240 nits; `MaxLuminance` 1000 → headroom 4.17.
- These numbers come from mpv rendering into its own window. The texture path
  needs re-measuring once the patch exists, and on macOS they were not taken.

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

- **The display clock needs nothing from Skathi:** on Windows, `DwmFlush` in our
  own thread plus QPC, or `DwmGetCompositionTimingInfo`; on macOS a
  `CVDisplayLink`/`CADisplayLink` for the window's screen (`info().display`
  says which).
- **Which of our frames landed on which refresh** only Skathi knows — that is
  the frame events above. With `precision: Exact`, mpv can run
  `video-sync=display-resample` via the timestamped `report_swap`. With
  `Estimated`, stay on `video-sync=audio` and use the events only to confirm
  steps and pauses.
- **Today's numbers to compare against** (macOS, e2e, 120 Hz): with the default
  `video-sync=audio`, 0 dropped and 0 delayed frames; gpu-next with
  `display-resample`: vsync-jitter 0.112–0.120, estimated display fps
  119.0–119.5, 2 mistimed frames, 5 delayed during steady playback. `vo=gpu`
  with `display-resample`: jitter 0.150–0.174, 117.4–118.4 fps, 9 mistimed, 15
  delayed. Windows, D3D11, `display-resample`: jitter 0.266, estimated 224.09
  against 240, 235 delayed (not investigated).
- `stats().shown` in today's Skathi counts a frame when its atlas copy is
  queued, before draw and present; it is not a "shown" signal.

### The window has to be opaque on Windows

Exact timestamps come only from `CreateSwapChainForHwnd`, i.e. an opaque window.
Ours is transparent only so that mpv's child window shows through the WebView;
once Skathi draws the video, that reason goes. Before asking for an opaque
window, check what else assumes transparency: the mini player, rounded corners,
and the "nothing painted means desktop visible" rule in
[rules/window.md](rules/window.md).

## Copies and the zero-copy question

Today a GPU frame takes three passes inside Skathi: copy into the surface
texture (`copy_in`), half-float → 8-bit mapping (`Ready::for_atlas` →
`draw_mapped`, lazy: only when first shown), copy into the window's atlas
(`node.rs` → `live.rs` → gpui atlas). For a 3840×2160 FP16 frame that is about
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
  `FramePresented` for the frame number is the proper wait.
- **SDR, gpu-next against `vo=gpu` on macOS:** flat colours match within 1/255,
  but window screenshots differ at SSIM ~0.92 across the whole video area
  (dithering or scaling; not investigated).

## Still open

- Windows with HDR off; the composition-window (`Estimated`) timestamps; AMD
  and Intel GPUs; anything across processes.
- The real Skathi — every synchronisation number above is against an emulated
  consumer that follows its code.
- Colour numbers on macOS, and through the texture path on either platform.
- Whether VRR is enabled on a given Windows setup (the monitor's EDID range was
  48–240 Hz with what looks like a FreeSync block; NVIDIA's setting is not
  readable without its API).

## Next steps

1. **A prototype without mpv:** a Skathi window with an `<sk-surface>` and the
   Vulkan producer from the probes, on both platforms. It checks the protocol,
   the ring size against lazy slot reuse, and the frame events against the real
   framework.
2. The mpv patch: the Vulkan render API type, then the timestamped
   `report_swap`; the Windows libmpv build.
3. Compare with today's player through the e2e harness (on the `host-boundary`
   branch: `bun run e2e --mpv-conf <file>`, per-step frame metrics,
   `bun run e2e compare`).

## Reproducing the probes

Throwaway programs, not kept. Enough to rebuild each:

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
- **mpv backend lists and logs:** libmpv with `terminal=yes`, `msg-level=all=v`;
  `option-info/*/choices` for compiled choices; the swapchain lines come from
  `vo/gpu-next/libplacebo` and `vo/gpu-next/d3d11` (or `/mac`).
