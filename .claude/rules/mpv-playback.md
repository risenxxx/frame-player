---
paths:
  - "src/routes/+page.svelte"
  - "src/lib/player.svelte.ts"
  - "src/lib/player/**"
  - "src/lib/playback.svelte.ts"
  - "src/lib/seek.svelte.ts"
  - "src/lib/seek-steps.svelte.ts"
  - "src/lib/playlist.svelte.ts"
  - "src/lib/frame-step*.ts"
  - "src/lib/step-engine.svelte.ts"
  - "src/lib/endscreen.svelte.ts"
  - "src/lib/crop*.ts"
  - "src/lib/picture-adjust.svelte.ts"
  - "src/lib/upscale*.ts"
  - "src/lib/now-playing.svelte.ts"
  - "src/lib/audio-output.svelte.ts"
  - "src/lib/mediainfo.ts"
  - "src/lib/screenshot.ts"
  - "src/lib/zoom*.ts"
  - "src/lib/components/MediaInfoDialog.svelte"
  - "src/lib/components/SkipButton.svelte"
  - "src/lib/components/ChapterMenu.svelte"
  - "src/lib/components/QueueMenu.svelte"
  - "src/lib/components/EndScreen.svelte"
  - "src/lib/components/SeekBar.svelte"
  - "src-tauri/src/frame_step.rs"
  - "src-tauri/src/step_engine.rs"
  - "src-tauri/src/crop.rs"
  - "src-tauri/src/now_playing.rs"
  - "src-tauri/src/audio_output.rs"
  - "src-tauri/src/screenshot.rs"
  - "src-tauri/lua/**"
  - "src-tauri/shaders/**"
---

# mpv-playback — the short form

The short form of [`docs/rules/mpv-playback.md`](../../docs/rules/mpv-playback.md): the modules this area is made of,
its rules one paragraph each, and the commands that exercise it. Claude Code loads this file
by itself when a file it covers is opened with Read/Edit/Write — not when one is read through
Bash, which is why `CLAUDE.md` says to read it before changing the area.

Each line is the rule; the chapter it links to is the whole of it.

## Modules

| Path | What lives there |
|---|---|
| `src/lib/playback.svelte.ts` | **Playback as one thing, whichever machine is decoding it.** Position, duration, paused, volume, the current chapter and a `can` record of what the session supports, plus the verbs (`togglePause`, `seekBy`, `seekFraction`, `jumpToChapter`, `stepChapter`, `advance`, `openEntry`, `setVolume`, `nudgeVolume`, `toggleMute`) that route themselves. It exists because the rule "while `cast.remote`, read `cast.time` rather than mpv's" is one that has to be *remembered*, and forgetting it is silent: mpv is parked **paused** on the frame it handed over, so its mirrors do not look wrong, they look frozen at a plausible value. Five places had been written without it and each was found by a viewer — the chapter list, the skip button, its guard, the Windows taskbar and the digit jumps. Two things it deliberately does not do. It does not cover the **seekbar gesture**: a local drag is a stream of preview seeks with the exact/keyframe probe behind it and a remote one is a single command on release, so hiding that would leave the caller unable to see what a gesture costs — those branches stay written out in `seek.svelte.ts`, which therefore stays *below* this module (importing it there is a cycle through `step-engine`). And it does not route hotkeys case by case: `CAST_BEHAVIOR` is a `Record<ActionId, …>`, so **a new action is a compile error until somebody classifies it** — the same device as `SKIP_LABEL`, and the answer to a bug class that was entirely omissions. Everything routable is a verb rather than a table entry, because a verb serves the buttons and the menus as well as the key; `refusedBySession` only says no |
| `src/lib/seek.svelte.ts` | Everything that moves the playhead — seekbar drag, wheel scrub, arrows, digit jumps — and the per-file exact/keyframe probe they share. Moved as one unit on purpose: this is the most invariant-dense code in the player and the least covered by tests. It reaches the frame-stepper through `initSeek` hooks so it need not know the sidecar exists |
| `src/lib/player.svelte.ts` + `src/lib/player/` | mpv state mirrors, `initPlayer` (options + observers + events), commands, tracks, file loading. Knows nothing about the UI; the page subscribes through hooks. `player.svelte.ts` is the public face and only re-exports — every importer goes through it — and the code is one module per seam under `player/`, dependencies running one way: `state.svelte.ts` (`player` and its types) at the bottom; `files.ts` (extensions, network sources, external-track folders), `filters.ts` (`@fpnorm`, the SDR colour mode, the viewer's mpv.conf keys), `tracks.ts` (`readList`, tracks, `matchTrack`), `chapters.ts` (chapters, `skipKind`); `mirrors.ts` (`OBSERVED`, `RESYNC`, the one `applyProperty`, `resyncState`, `positionNow` and the waits the event path releases); `commands.ts` (playback, repeat, speed, delays, A–B, picture geometry); `ytdlp.svelte.ts`; and `session.ts` (`initPlayer`, its hooks, `loadFiles` — together because both read the hooks) at the top. Nothing under `player/` imports the public face |
| `src/lib/frame-step.svelte.ts`, `frame-step-plan.ts` + `src-tauri/src/frame_step.rs` | Frame stepping as it ships: the session (software decoding while stepping, backward decoding on slow files, handing mpv back before any seek or play), the budget and landing arithmetic (pure, tested), and the probe that measures the groups around a position and the machine's memory. See the frame-stepping line in [rules/mpv-playback](../../docs/rules/mpv-playback.md) |
| `src-tauri/src/step_engine.rs` | FFmpeg sidecar frame-stepper (GOP ring). **Disabled** via `USE_STEP_ENGINE=false` in step-engine.svelte.ts, which hands every step to `frame-step.svelte.ts`; kept working + smoke-tested |
| `src-tauri/src/crop.rs` + `src/lib/crop.svelte.ts` | Removing baked-in black bars (ROADMAP 34): `crop.rs` measures the luma plane of keyframes from across the file (pure, unit-tested), `crop_detect` in thumb_service.rs feeds it, and the frontend turns the answer — or a named shape, via `crop-geometry.ts` — into `video-crop`; the mode is kept for the playlist and each file measures or computes its own rectangle. See the picture-geometry line in [rules/mpv-playback](../../docs/rules/mpv-playback.md) |
| `src/lib/picture-adjust.svelte.ts` | Brightness, contrast, saturation and gamma (ROADMAP 36): four sliders in the context menu's picture panel that turn both menus see-through while dragged (the global ones again under Settings → Video), three scopes (this file, its playlist, every file; the narrowest one with a record wins, and moving onto a scope holding other values asks which set survives), the bars kept away while a slider is in use (`chrome.tuning`), and eight unbound hotkeys plus `C` for the panel. See the line in [rules/mpv-playback](../../docs/rules/mpv-playback.md) |
| `src/lib/upscale.svelte.ts`, `upscale-plan.ts` | The upscaler (ROADMAP 38): a neural-network shader from `src-tauri/shaders/` chosen per file or per playlist in the picture panel, plus one unbound hotkey. `upscale-plan.ts` is the pure half — the modes, each shader's threshold, the ratio the shaders themselves test (from the `osd-dimensions/*` mirrors) and the scope resolution — and is tested. See the upscaler line in [rules/mpv-playback](../../docs/rules/mpv-playback.md) |
| `src-tauri/src/now_playing.rs` + `src/lib/now-playing.svelte.ts` | Now Playing on macOS: the Control Center entry, the keyboard's play/pause key, the AirPods' pause. The frontend reports what plays, the native side hands it to MediaPlayer and sends the system's commands back as an event. See the Now Playing line in [rules/mpv-playback](../../docs/rules/mpv-playback.md) |
| `src-tauri/src/audio_output.rs` + `src/lib/audio-output.svelte.ts` | A sound offset per output device, for the lag of a Bluetooth speaker: which device the system plays through (polled natively, since mpv on `auto` does not say), the offsets kept per device, and the one rule that `audio-delay` holds file delay + device offset with `tracks.svelte.ts` keeping the halves apart. See the line in [rules/mpv-playback](../../docs/rules/mpv-playback.md) |
| `src-tauri/src/screenshot.rs` | Frame export: destination dir (created on demand), temp path + clipboard hand-off. mpv writes the PNG; this only supplies what mpv cannot do |
| `src-tauri/lua/zoompan.lua` | Atomic zoom+pan on mpv's core thread (copied next to the dev exe by build.rs; bundled via resources) |

## Rules

- **The end of a file is a moment the player owns, and that is a deliberate inversion.** `keep-open=always` (not mpv's `yes`, which stops only on the LAST playlist entry and advances silently everywhere else) parks mpv on the final … → [mpv-playback](../../docs/rules/mpv-playback.md)
- **A file's name in the queue comes from three places, and only the third costs anything.** mpv fills `playlist/N/title` from `media-title`, but **only for entries it has actually opened** — so everything ahead of the playhead was … → [mpv-playback](../../docs/rules/mpv-playback.md)
- **A queue is built from one opened file, never from several.** Dropping five files is an explicit selection and stays exactly what was given; opening a single episode is the case where the rest of the folder is wanted. → [mpv-playback](../../docs/rules/mpv-playback.md)
- **Seek flags are a performance contract, not a detail**: `hr-seek=yes` makes any seek without an explicit flag *exact*, and that cost is a property of the **file** — so it is probed once per file (`issueSeek` times the first one, `fileSlowSeek`/`wantExact()` decide from then on, `resetSeekProbe()` on `path`), never switched mid-gesture. → [mpv-playback](../../docs/rules/mpv-playback.md)
- **Frame stepping goes through mpv, made fast in two moves that mpv itself undoes at the next file.** A step back takes the decoder to software for the session (hardware is 2–10× slower at a one-frame seek) and gives it back only while paused, so our play controls wait for it before unpausing (during playback a hardware AV1/VP9 decoder fails on mid-group packets and mpv stays in software, at 3–4× the power); from the second step back in a row on a slow file on this disk (local, or a torrent — one still downloading only where its map of arrived bytes covers the groups being decoded, since a hole reads back as zeros), mpv decodes backward (`play-direction=backward`) with the audio track off, inside a memory budget, with every landing checked because an overflow is silent; the track comes back only after the turn forward, or it lands on the next keyframe. All of it through `file-local-options/`, and every seek goes through `frameStepBeforeSeek` first, because turning the direction queues a seek that would replace the viewer's. → [mpv-playback](../../docs/rules/mpv-playback.md)
- **Frame export and external tracks go through mpv, not around it.** `screenshot-to-file` keeps exactly what the VO decoded, HDR tone mapping included — which is precisely what the StepEngine canvas path got wrong, so do not … → [mpv-playback](../../docs/rules/mpv-playback.md)
- **Every list property is read through sub-properties, and they do not all behave alike.** `track-list`, `chapter-list`, `playlist` and `audio-device-list` are all implemented on mpv's side by the same helper, which exposes … → [mpv-playback](../../docs/rules/mpv-playback.md)
- **"Skip intro" matches the whole chapter title, never a substring.** `skipKind()` offers a skip button for the first `SKIP_WINDOW` seconds of a chapter that names itself an opening, a recap, a preview, a trailer, the credits or … → [mpv-playback](../../docs/rules/mpv-playback.md)
- **The media info panel is a live readout, not a snapshot, and half of it is legitimately missing.** Every field comes from a plain `getProperty` — measured on a 4K HEVC file, `video-codec` gives the readable name ("H.265 / … → [mpv-playback](../../docs/rules/mpv-playback.md)
- **Picture geometry is per file, and reset only if we touched it; the crop is the one part remembered — its mode per playlist, its rectangle per file.** `video-rotate`, `video-aspect-override` and `panscan` are global mpv options: a phone clip turned upright stays turned for the next episode, so they are reset on … → [mpv-playback](../../docs/rules/mpv-playback.md)
- **The A–B loop is mpv's to run; ours only to mark.** Both points set means mpv loops the segment by itself — measured, playback stays inside the range and wraps without anything watching the position. → [mpv-playback](../../docs/rules/mpv-playback.md)
- **Volume leveling is a labeled `af` filter, and its state is ours.** `af add @fpnorm:lavfi=[dynaudnorm=…]` / `af remove @fpnorm` — the label is what lets it be removed again without touching an `af` line from the user's own … → [mpv-playback](../../docs/rules/mpv-playback.md)
- **SDR color on macOS is a choice between two curves, and ICC is not part of either.** "As in macOS" is `@fpsdr:format=gamma=gamma2.2` + `gamma-factor` + `target-trc=srgb` + `treat-srgb-as-power22=no`, only while `video-params/gamma` is `bt.1886`; `icc-profile-auto` would colour-manage the picture a second time. → [mpv-playback](../../docs/rules/mpv-playback.md)
- **`hwdec-current` is shown in the settings footer** — a silent fallback to software decoding is otherwise invisible and is the first thing to check when seeking feels slow.
- **An `end-file` error is not a failed attempt.** mpv's ytdl hook raises one on the way to succeeding — it opens the URL, gives up on it and loads what yt-dlp resolved instead — and playlist transitions do the same. Reporting on the event threw an error dialog over videos that were about to play, which is worse than saying nothing. The verdict now waits `LOAD_FAIL_GRACE` and `onFileLoaded` cancels it: if anything opened, whatever failed on the way was a step and not an outcome.
- **Picture adjustments have three scopes and the narrowest one with a record wins.** File, then playlist (`folderScopeKey`), then every file; widening a choice deletes the narrower records for this source. The four equalizer options leak across `loadfile` like the rest, so each file gets all four explicitly. → [mpv-playback](../../docs/rules/mpv-playback.md)
- **The upscaler runs only when the picture is drawn larger than it was decoded, and says so when it is not.** Three shaders in `src-tauri/shaders/` — ArtCNN C4F16 DS for video, Anime4K CNN M for animation, Anime4K GAN x2 M for `strong`, the only one that invents texture and therefore only for sources up to 720p (`STRONG_MAX`; above it, `video`); each pass's `//!WHEN` compares the picture's part inside the window with the whole decoded plane — crop and zoom do not count — and `upscaleRatio` repeats the test for the panel. File and playlist scopes, the file winning, no global one; `glsl-shaders` leaks across `loadfile`, so every file gets its list, and an mpv.conf list is left alone. → [mpv-playback](../../docs/rules/mpv-playback.md)
- **Now Playing on macOS is the player's own, and mpv's is switched off.** mpv's never learns what plays under libmpv (its event helper wants mpv.app), so `input-media-keys=no` and `now_playing.rs` + `now-playing.svelte.ts`: facts down through `now_playing_set`, commands up as `frameplayer://remote` into `playback`'s verbs. → [mpv-playback](../../docs/rules/mpv-playback.md)
- **Anything mpv keeps across a file change is a leak waiting to be noticed.** `video-rotate`, `video-aspect-override`, `panscan`, the A–B loop points and — measured — **`sub-delay`, `audio-delay` and `sub-speed`** all survive a `loadfile`, so … → [mpv-playback](../../docs/rules/mpv-playback.md)
