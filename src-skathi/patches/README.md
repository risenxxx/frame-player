# mpv patches for version 2

Version 2 draws through mpv's render API with the gpu-next renderer. Stock mpv has
neither half of that yet, so the libmpv version 2 loads is built from:

1. **mpv PR #18845** ("libmpv: use the gpu-next renderer by default in the render
   API"), the upstream pull request, not kept here. Both patches below were written
   against its head at the time: `aade20e8c` (macOS, `v0.42.0-110-gaade20e8c`) and
   `a98335f5` (Windows). Fetch it with `git fetch https://github.com/mpv-player/mpv
   pull/18845/head` and check out that commit.
2. One patch of ours per platform:

| Patch | Platform | What it adds |
|---|---|---|
| `mpv-vulkan-render-api.diff` | macOS | `MPV_RENDER_API_TYPE_VULKAN` (`include/mpv/render_vk.h`, `video/out/vulkan/libmpv_vk.c`): mpv renders with gpu-next into Vulkan images the caller owns, on MoltenVK. Adds an overlay target (subtitles and OSD apart from the picture) and `picture_layout` (the element's whole layout scaled to the picture image). 9 files |
| `mpv-d3d11-overlay.diff` | Windows | `MPV_RENDER_PARAM_D3D11_OVERLAY_FBO` (`render_d3d11.h`): subtitles and OSD drawn for a size the caller names into a second texture, left out of the picture. 7 files |

Without the patch a route still runs: macOS `FPUI_ROUTE=gl` uses version 1's
libmpv; on Windows the host sees that the overlay target is missing and draws
subtitles into the picture.

The macOS build links libplacebo (7.398) and MoltenVK. `scripts/build-macos-libs.sh --v2`
(`scripts/build-macos-libs-v2.sh`) builds this libmpv and its whole closure from pinned
sources into `src-skathi/lib`, applying `mpv-vulkan-render-api.diff` to the PR head
above; a development build points `MPV_LIB_DIR` at that directory. Measurements and the
decisions behind both routes: `docs/native-frames.md` on the [`docs/native-frames` branch](https://github.com/risenxxx/frame-player/tree/docs/native-frames).
