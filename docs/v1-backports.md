# Version 1 backports

Improvements found while building version 2 that the shipping player,
version 1, could also use. **This is a list, not a plan.** The priority is
the version 2 beta ([v2-beta-release.md](v2-beta-release.md)); nothing here
is scheduled. Until version 2 graduates, code does not go from the `v2`
branch back to `main` (project decision of 2026-11-23; the one exception
is version 2's release notes, which its release workflow copies to `main`
for the site, as v2-beta-release.md decides). This file is not a rule
about that; it is where an improvement that version 1 could take is
written down instead of being ported on the spot. The items are to be verified in version 1 and ported **together,
later**, in one piece of work.

Each item says what it is, where the evidence is, what it gained on each
system, what could go wrong, and what has to be checked in version 1
before it is ported. Labels as in [native-frames.md](native-frames.md):
**measured**, **from source**, **inferred**, **not checked**. Most of the
evidence was measured in the version 2 port, not in version 1; where an
item was measured in version 1 itself (an isolated copy of 1.48.1 on macOS
and the portable 1.49.1 on Windows), it says so. Machines as in
native-frames.md: an M3 Max with its built-in XDR display, and a Windows 11
desktop with an RTX 5080 and a 3840×2160 240 Hz HDR display.

## 1. A lighter mpv render set, under the viewer's own `mpv.conf`

**What.** Six options as defaults, applied *under* the viewer's
`mpv.conf` so that any key the viewer sets wins: `profile=fast`'s resolved
keys (scalers bilinear, `dither=fruit`), `linear-downscaling=yes`,
`correct-downscaling=yes`, `hwdec-threads=1` (item 3), and peak detection
as decided below. In our notes this set is "f2".

**Evidence.** native-frames.md, "A lighter render set in the port", the
part "The shipping player under the same set": f2 given to version 1 as
`mpv.conf` lines, 3 launches a cell, the installed player's data
checksummed before and after.

**Gain (measured in version 1, f2 against its defaults, 24 fps content,
screen-sized window, no drop in any launch):**

| | 1080p | 2160p PQ / HDR10 | GPU time |
|---|---|---|---|
| macOS 1.48.1 (main process and decoder service) | **−86 MiB** (655 → 569) | **−327 MiB** (1399 → 1072) | 9.3 → 3.3 % and 13.7 → 6.0 % |
| Windows 1.49.1, private bytes over all 13 processes | **−99 MiB** (827 → 728) | **−194 MiB** (1243 → 1049) | 2.30 → 1.85 % and 6.54 → 5.67 % |
| Windows 1.49.1, dedicated GPU memory | −66 MiB (491 → 425) | −219 MiB (1154 → 935) | |

On macOS most of the HDR saving is mpv's own chain at screen size, which
version 1 runs in full (graphics 526 → 322 MiB). The macOS figures leave
out the WebKit helpers, which were not attributed in those runs and do not
read mpv's options (inferred).

**Risk.**
- **The mini player's downscale (Windows, measured in the port).**
  `profile=fast` turns `dscale` from hermite into bilinear. In the port's
  mini player a 4K zone plate then shows faint ghost rings: SSIM 0.862
  against 0.934. Version 1's default `dscale` is hermite too (read back on
  Windows), so the same is expected there (inferred). On macOS the port
  ranked the two the other way (0.546 against 0.509); why is not checked.
  The remedy is to keep `dscale` out of the set, or set hermite
  explicitly. Without `correct-downscaling` the mini player is moiré over
  the whole picture on both systems, so that key must stay.
- **Chroma** (Windows, from source and measured in the port): an unset
  `cscale` follows `scale`, so chroma becomes bilinear too. Lanczos chroma
  overshoots in an unclipped HDR target (peak 2.84 × SDR white on a chroma
  zone plate against 1.87), so this may be a gain; the look on real content
  is not checked.
- **Peak detection.** On Windows the `hdr-compute-peak=auto` line read
  back `false`: version 1 applies `profile=fast` after it (inferred from the
  read-back), so the Windows numbers are without peak detection. On macOS
  it was not read back. Decide whether the set keeps peak detection, and
  measure that variant.
- **Upscaling sharpness**: bilinear against lanczos lost sharpness on a
  1080p zone plate enlarged 2× in the port (SSIM 0.997 → 0.945, Windows).
  The macOS real-content crops (P0 against P1, 40–62 dB) wait for the
  owner's verdict.
- **Delivery.** Send the resolved keys one by one, never `profile=fast`
  itself (a profile sets nine options at once and would override a
  viewer's `scale=` or `dither=` depending on order). Skip any key for which
  `isUserConfKey` is true, as `src/lib/picture-adjust.svelte.ts` does. Keep
  the set apart from the viewer's lines: one refused line makes the player
  drop all of the viewer's lines (measured in the port), and the defaults
  must not go with them.

**Check before porting.**
- The mini player with a 4K zone plate and real content, with `dscale`
  left at hermite, on both systems.
- Every key read back in version 1 after start, on both systems.
- Peak detection on and off, at 2160p PQ/HDR10.
- The real-content crops judged.
- Seeks, frame steps both ways, screenshots and the upscaler with the set
  on (the port showed no change; version 1 not run).
- A viewer's `mpv.conf` that sets some of the keys: theirs wins, the rest
  apply.

## 2. `swapchain-depth=1` on Windows

**What.** One option for version 1's own swap chain, which mpv owns under
`--wid` (`vo=gpu-next`, D3D11). It does nothing in version 2, whose render
API has no swap chain of mpv's (from source).

**Evidence.** native-frames.md, "A lighter render set in the port", the
shipping player's table (the row "f2 + `swapchain-depth=1`").

**Gain (measured in version 1 on Windows, on top of f2, 3 launches a
cell):** **−31 MiB** of dedicated GPU memory at 1080p (425 → 394) and −32
with HDR10 (935 → 903). GPU time unchanged; 24 distinct frames a second,
none skipped; the 99th-percentile frame gap, median of the three launches,
45.9 ms at 1080p and 45.8 ms with HDR10 against f2's 45.4 and 46.3. Three
launches a cell, so these gaps say only that nothing got visibly worse.
macOS: not measured (version 1 also presents through mpv's own swap chain
there, so the option applies; inferred).

**Risk.** One buffer less in flight: pacing under load, at high frame
rates and on slower GPUs may suffer; measured only on one fast GPU, at
24 fps, under the test machine's driver frame cap (224 a second), and only
together with f2.

**Check before porting.** Alone over version 1's defaults; 60 fps content;
a slower GPU and AMD or Intel; seeks and frame steps; on macOS whether it
applies and what it saves.

## 3. `hwdec-threads=1`

**What.** One decoder thread instead of mpv's default of 4 for hardware
decoding. Part of item 1, listed apart because its cost is not picture
quality but decode throughput.

**Evidence.** native-frames.md, "A lighter render set in the port": the
set without it (P2) against the set with it, and the decoder measured
alone outside the port.

**Gain.**
- **Windows** (measured): libmpv alone, `vo=null`, untimed, `d3d11va-copy`,
  3 interleaved rounds: 4K HEVC Main10 HDR10 holds **96 MiB less GPU
  memory** (721 → 625) and 60 MiB less private (448 → 388). In the port the
  same 96 MiB with HDR10, 48 at 4K SDR, 12 at 1080p.
- **macOS** (measured in the port): −114 to −121 MiB at 2160p PQ (malloc
  and the VideoToolbox decoder service), 26 at 1080p, 82 at 4K in the mini
  player; decode cadence unchanged.
- Version 1 itself: not measured apart from f2.

**Risk.** On Windows the 4K HEVC decode rate falls **from 512 to 450 frames
a second (−12 %)**, still about 19× real time on the test GPU; 4K H.264
and software decoding do not change. On a weaker GPU, at 8K or at high
frame rates the margin may be gone. Fewer frames in flight might lengthen
a seek or a step back; not measured. macOS throughput was not measured.

**Check before porting.** Decode throughput on an older or integrated GPU
(AMD, Intel) and on an older Mac; 8K and 4K at 60 fps; seek and step-back
latency against today; that the frame-step hand-over to the software
decoder is unchanged (the port showed it unchanged on Windows).

## 4. The upscaler's memory after it is turned off

**What.** Turning the upscaler (ArtCNN) off may not give its memory back
until mpv's video output is rebuilt. If version 1 shows the same, turning
it off should rebuild the video output, or the viewer keeps paying for a
shader they removed.

**Evidence.** native-frames.md, "A lighter render set in the port", "The
upscaler".

**Gain (measured in the port, not in version 1).**
- **Windows**: the upscaler costs +419 MiB of GPU memory (906 against 487);
  cleared at 14 s through `change-list glsl-shaders clr`, as the frontend
  does, only **36 MiB came back and 383 stayed** to the end of the launch,
  in both runs and in a third launch.
- **macOS**: a shader that drew only the first frames before the frontend
  cleared it left **+500 to +630 MiB** at 12 s on both of the port's routes.
- That libplacebo keeps what a removed hook allocated is inferred.

**Risk.** Rebuilding the video output may show a black frame or a hitch;
not measured. Version 1 may behave differently: it renders through
`vo=gpu-next` with its own swap chain, not the render API.

**Check before porting.** **First, whether version 1 has the problem at
all**: turn the upscaler on, play, turn it off, and read memory on both
systems. Only if it does: the cost and the visible effect of a rebuild at
that moment, and whether it can wait for a pause.

## 5. A portable copy's `veil` window uses the installed player's profile (Windows, issue #81)

**What.** `create_main_window` gives a portable copy its own web view data
directory, but `create_veil` (`src-tauri/src/lib.rs`) sets none, so the
hidden `veil` window opens `%LOCALAPPDATA%\app.frameplayer\EBWebView`, the
installed player's profile, on every portable launch, and creates it on a
machine without an installation (from source). Filed as Frame Player
issue #81.

**Evidence.** native-frames.md, "A lighter render set in the port", the
shipping player's control; [v2-beta-release.md](v2-beta-release.md), "The
pitfall issue #81 shows"; issue #81.

**Gain.** Correctness and privacy, not memory. Measured on Windows: one
control session with the portable 1.49.1 changed 76 files in the
installed player's `EBWebView` (logs, caches, `Preferences`, `Secure
Preferences`, the Session Storage log, GPU cache indexes), no Local
Storage data file and nothing under `%APPDATA%`. macOS: not affected (no
portable build).

**Risk.** Low: the `veil` window holds no state of its own (inferred).
Moving its profile beside the main window's adds a second web view data
folder inside the portable copy's `.data/`, or shares the main window's.

**Check before porting.** That a WebView2 environment can be shared
between the two windows with one data directory, or needs two; a hash of
the installed player's folders before and after a portable run (the test
v2-beta-release.md asks for); that an installed copy is unchanged.

## 6. Torrents: remove `.meta/<hash>` when the prune drops a torrent

Moved from the appendix of [v2-beta-release.md](v2-beta-release.md).

**What.** The prune removes librqbit's `<hash>.torrent` and `<hash>.bitv`
but leaves version 1's own metadata copy in `state`
(`src-tauri/src/torrent/session.rs:546-552`, from source). Remove it as
well.

**Evidence.** v2-beta-release.md, "Torrents", option (b): after
onboarding moves a torrent to version 2, that copy is an orphan, the only
thing the move leaves behind in version 1's cache.

**Gain.** Tidiness on both systems: the copy is small. Version 1 is
correct without it.

**Risk.** Low; the prune is the only writer that should take it away.

**Check before porting.** That nothing else reads `.meta/<hash>` after a
prune (the remembered list, the continue-watching card).

## 7. Torrents: say "moved to Frame Player 2" instead of downloading again

Moved from the appendix of [v2-beta-release.md](v2-beta-release.md).

**What.** A "continue watching" card whose torrent folder is gone re-adds
the torrent from the remembered magnet and downloads it again into version
1's folder (inferred from `src/lib/torrent/remembered.ts:36-38` and
`session.rs:505-532`). If the move leaves a marker in version 1's `state`
(for example `.moved-to-v2` listing the info hashes), version 1 can show
where the episode went and offer to open version 2 instead.

**Evidence.** v2-beta-release.md, "Torrents".

**Gain.** No second download of something the viewer already has, on both
systems. Version 1 is correct without it.

**Risk.** It ties version 1 to a file version 2 writes; the marker's
format becomes a contract between the two. New strings in all three
languages.

**Check before porting.** The marker's format agreed with version 2's
onboarding; what happens when version 2 is uninstalled after the move;
the strings.
