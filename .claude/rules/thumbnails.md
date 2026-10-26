---
paths:
  - "src/lib/thumbs.svelte.ts"
  - "src-tauri/src/thumb_service.rs"
  - "src-tauri/src/color.rs"
  - "src-tauri/src/step_engine.rs"
  - "src-tauri/src/crop.rs"
---

# thumbnails — the short form

The short form of [`docs/rules/thumbnails.md`](../../docs/rules/thumbnails.md): the modules this area is made of,
its rules one paragraph each, and the commands that exercise it. Claude Code loads this file
by itself when a file it covers is opened with Read/Edit/Write — not when one is read through
Bash, which is why `CLAUDE.md` says to read it before changing the area.

Each line is the rule; the chapter it links to is the whole of it.

## Modules

| Path | What lives there |
|---|---|
| `src-tauri/src/thumb_service.rs` | Seekbar thumbnails: seek to keyframe → decode forward to the exact grid position *only when the keyframe would duplicate the neighbouring cell* (files whose GOP is longer than the grid step used to show one frame for seconds of seekbar) → JPEG, background storyboard, disk cache (`app_cache_dir/thumbs`, magic-versioned, LRU-pruned to 256 MB on every save). A hover maps to the **nearest** cell, and a *resting* cursor gets the frame at its exact position instead of the cell's (`exact` flag on `thumb_get`) — see "The preview must show the frame you land on" below |
| `src-tauri/src/color.rs` | Turning a decoded frame into something a person can look at: the PQ and HLG transfer functions, the IPT inverse Dolby Vision profile 5 needs, the exposure anchors and BT.2020 → BT.709. Pure arithmetic and unit-tested, because every one of its errors is a *plausible* picture in the wrong colour — see "A preview is a decoded frame, and a decoded frame has a colour space" below |

## Rules

- **Whether the sidecar shares mpv's FFmpeg depends on the platform, and the prefix on a log line tells you who is talking.** On macOS `src-tauri/ffmpeg-macos/lib/*.dylib` are **symlinks into `src-tauri/lib/`** (verified with … → [thumbnails](../../docs/rules/thumbnails.md)
- **Background decoding competes with playback — budget it explicitly.** The thumbnail storyboard decodes in *software*, so it degrades the player identically whether mpv's hwdec is on or off; that symmetry is the tell. → [thumbnails](../../docs/rules/thumbnails.md)
- **A preview is a decoded frame, and a decoded frame has a colour space — which the thumbnail service used to ignore entirely.** `scaling::Context::get` was given the two pixel formats and nothing else, so swscale fell back to its … → [thumbnails](../../docs/rules/thumbnails.md)
- **The preview must show the frame you land on, and both halves of that were wrong.** The hover thumbnail comes from a grid whose step is `clamp(duration/240, 0.25, 10)` — 10 s for anything longer than 40 minutes. → [thumbnails](../../docs/rules/thumbnails.md)
- **No storyboard means no space for one.** With thumbnails gated for streams the hover popup was still reserving its 180px frame and filling it gray, which reads as a preview that broke rather than one that was never coming. → [thumbnails](../../docs/rules/thumbnails.md)

## Commands

Sidecar smoke tests (both engines: stepping + thumbnails). Generate the fixture with the bundled ffmpeg:

```bash
src-tauri/ffmpeg/bin/ffmpeg.exe -f lavfi -i testsrc2=duration=60:size=1280x720:rate=30 -c:v mpeg4 -q:v 5 test.mp4
cd src-tauri && FP_TEST_VIDEO=<abs path to test.mp4> cargo test --lib -- --nocapture
```

(The LGPL ffmpeg build has **no GPL encoders** — use `mpeg4`, not libx264.)

A colour change cannot be checked by an assertion — a thumbnail of the right size in the wrong hue passes every one that can be written — so `thumb_smoke` takes the positions and writes the JPEGs out for looking at:

```bash
cd src-tauri && FP_TEST_VIDEO=<abs path> FP_TEST_POS=600,1200,2100 FP_TEST_DUMP=/tmp/thumbs \
  cargo test --lib thumb_smoke -- --nocapture
```
