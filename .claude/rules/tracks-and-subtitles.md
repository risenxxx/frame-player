---
paths:
  - "src/lib/tracks.svelte.ts"
  - "src/lib/resume.svelte.ts"
  - "src/lib/subs.svelte.ts"
  - "src/lib/sub-*.ts"
  - "src/lib/track-badge.ts"
  - "src/lib/languages.ts"
  - "src/lib/external-title.ts"
  - "src/lib/components/SubsDialog.svelte"
  - "src/lib/components/TrackMenu.svelte"
  - "src/lib/components/TrackTag.svelte"
  - "src-tauri/src/opensubtitles.rs"
  - "src-tauri/src/external_dirs.rs"
---

# tracks-and-subtitles — the short form

The short form of [`docs/rules/tracks-and-subtitles.md`](../../docs/rules/tracks-and-subtitles.md): the modules this area is made of,
its rules one paragraph each, and the commands that exercise it. Claude Code loads this file
by itself when a file it covers is opened with Read/Edit/Write — not when one is read through
Bash, which is why `CLAUDE.md` says to read it before changing the area.

Each line is the rule; the chapter it links to is the whole of it.

## Modules

| Path | What lives there |
|---|---|
| `src/lib/resume.svelte.ts`, `tracks.svelte.ts` | The two things a file needs before it can play the way the viewer left it. `resume` starts it at the remembered position — via mpv's **`start` option before `loadfile`**, not a seek afterwards, which is why the `beforeLoad` hook is awaited — and holds the seekbar honest meanwhile: `barDuration` borrows the remembered length until mpv reports its own (display only; seek arithmetic must keep using `player.duration`), and `seek.settling` pins the knob because `time-pos` reports zero for the whole of that window. `tracks` is the remembered audio/subtitle choice and the per-file delays, which share a module for the same reason they share a store — both answer "how does this source have to be played", and a manual pick has to cancel a restore still waiting for its track. Its restore is a **standing effect**, not a one-shot on `file-loaded`: external subtitles arrive a beat later |
| `src/lib/languages.ts` | The **content** languages (ROADMAP 25) — one table of 639-1 code + endonym + the 639-2 spellings, from which `LANG_ALIASES`, the `alang`/`slang` picker and the subtitle-search filter are all derived. Nothing to do with `i18n.svelte.ts`, which is the *interface* language |
| `src-tauri/src/opensubtitles.rs` | Subtitle search and download: the API key's lookup order, the OpenSubtitles file hash, `subs_search` (hash and/or title, privacy-gated) and `subs_download` (ticket → file → saved beside the video) |
| `src-tauri/src/external_dirs.rs` + `src/lib/external-title.ts` | External dubs and subtitles in a release's subfolders: which folders under the video hold them (handed to mpv's own search as `sub-file-paths`/`audio-file-paths` before the load, by `pointAtExternalTracks`), and what to call a track found there instead of mpv's "mka". See the line in [rules/tracks-and-subtitles](../../docs/rules/tracks-and-subtitles.md) |

## Rules

- **A subtitle found by the file's hash and one found by its title are different results, and the panel says which.** The hash (size + every 64-bit word of the first and last 64 KiB) identifies a *release*, so what comes back is … → [tracks-and-subtitles](../../docs/rules/tracks-and-subtitles.md)
- **Closed captions are a subtitle track that isn't in the container.** `eia_608`/`eia_708` captions ride inside the video stream (SEI), so `ffprobe -select_streams s` shows nothing while mpv lists `--sid=1 (eia_608)` — do not … → [tracks-and-subtitles](../../docs/rules/tracks-and-subtitles.md)
- **The subtitles get out of the control bar's way, and `sub-pos` is the only lever measured to work.** While the chrome is up the last line of dialogue sits behind the seekbar — exactly while it is being read, since what raised … → [tracks-and-subtitles](../../docs/rules/tracks-and-subtitles.md)
- **A remembered track is a description, not an id, and its scope is the folder.** Track ids are positions inside one file — the Russian dub that is #2 in episode 1 is routinely #3 in episode 2 — so a choice is stored as `{lang, … → [tracks-and-subtitles](../../docs/rules/tracks-and-subtitles.md)
- **Restoring on resume has two traps.** The chosen tracks live in `frameplayer.tracks`, separate from `frameplayer.positions` because those entries are *deleted* past 97 % and a dub choice should outlive finishing an episode; only … → [tracks-and-subtitles](../../docs/rules/tracks-and-subtitles.md)
- **The audio and subtitle buttons carry a tag, and it is only as fresh as `Track.selected`.** Language heard / count while off / language once on, decided in `track-badge.ts`; every write to `aid`/`sid` is followed by `loadTracks`, because the tag never leaves the screen. → [tracks-and-subtitles](../../docs/rules/tracks-and-subtitles.md)
- **A subtitle made for another frame rate is stretched, never delayed, and the stretch is offered, never applied.** `sub-speed` = subtitle fps / video fps (`sub-speed.ts`); a search result that knows its fps asks, the track menu covers the rest by hand. → [tracks-and-subtitles](../../docs/rules/tracks-and-subtitles.md)
- **The language of the interface and the language of the film are different questions, and conflating them was a bug with a name.** Every language choice in the player used to offer exactly two — `alang`/`slang` in the settings, … → [tracks-and-subtitles](../../docs/rules/tracks-and-subtitles.md)
- **"No subtitles in a language I am already hearing" is mpv's option, not ours, and its rule is not the obvious one.** `subs-with-matching-audio` — choices `no`/`forced`/`yes`, default **`yes`** on mpv 0.41 (read out of … → [tracks-and-subtitles](../../docs/rules/tracks-and-subtitles.md)
