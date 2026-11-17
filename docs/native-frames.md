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
FFmpeg decode → mpv render API → a texture we own, shared with Skathi
             → Skathi surface: GPU wait, copy, map, draw into its window

Windows, now:   mpv's D3D11 render API (old vo_gpu renderer) → shared D3D11 texture → Skathi (D3D12)
Both, later:    mpv gpu-next (libplacebo) on Vulkan → IOSurface / D3D12 resource → Skathi
```

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
  and our libmpv refuses OpenGL. libplacebo has D3D11, Vulkan and OpenGL
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
  renderer and should appear in the texture), Windows HDR off.

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
   libmpv refuses `gpu-api=opengl` and `gpu-context=cocoa` (-7).
2. **`report_swap` with a timestamp** (needed by every backend, D3D11
   included). `mpv_render_context_report_swap()` takes no time: mpv assumes the
   frame was shown at the moment of the call. Skathi reports presentation after
   the fact (below), so the patch must accept `shown_at` and the refresh
   interval, and know about dropped frames, or display-resample timing will be
   wrong.
3. **A Windows build of libmpv of our own** for anything patched. Today Windows
   uses a prebuilt libmpv and only macOS applies patches. mpv now publishes
   official libmpv builds for Windows (x86_64 and arm64, LGPL and GPL) from its
   CI; that workflow is the starting point for ours.

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
  hdr-compute-peak=no target-peak=<203 × headroom>`. With Skathi's window SDR
  today (`output_headroom` 1) that is `target-peak=203`; once it outputs HDR,
  `203 × output_headroom` (846 on this display: 1000 / 240 × 203). This is the
  recipe Skathi's documentation gives, plus `hdr-compute-peak=no`.
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
  was measured on macOS.

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
- Colour numbers on macOS, and through a gpu-next render-API texture (none
  exists yet); OSD and subtitles through the D3D11 render API.
- Whether VRR is enabled on a given Windows setup (the monitor's EDID range was
  48–240 Hz with what looks like a FreeSync block; NVIDIA's setting is not
  readable without its API).

## Next steps

1. **A Windows prototype with real mpv:** a Skathi window with an `<sk-surface>`
   fed by the D3D11 render API (shared textures created in D3D11, one fence per
   texture, CPU wait for the slot). It needs no mpv patch and checks the
   protocol, the ring size against lazy slot reuse, and the frame events
   against the real framework.
2. **A macOS prototype without mpv:** the same with the Vulkan → IOSurface
   producer from the probes.
3. The mpv patch: the Vulkan render API type for gpu-next (after checking what
   upstream's renderer split has delivered by then), the timestamped
   `report_swap`, a Windows libmpv build of our own.
4. Compare with today's player through the e2e harness (on the `host-boundary`
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
