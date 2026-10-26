---
paths:
  - "src/lib/history.svelte.ts"
  - "src/lib/source.ts"
  - "src/lib/report*.ts"
  - "src/lib/redact.ts"
  - "src/lib/console-log.ts"
  - "src/lib/trash.ts"
  - "src/lib/components/ReportDialog.svelte"
  - "src/lib/components/StartScreen.svelte"
  - "src-tauri/src/logs.rs"
  - "src-tauri/src/install_count.rs"
  - "src-tauri/src/shell_handler.rs"
  - "src-tauri/src/trash.rs"
  - "src-tauri/src/lib.rs"
  - "src-tauri/tauri.conf.json"
  - "site/installs/**"
---

# sources-and-privacy — the short form

The short form of [`docs/rules/sources-and-privacy.md`](../../docs/rules/sources-and-privacy.md): the modules this area is made of,
its rules one paragraph each, and the commands that exercise it. Claude Code loads this file
by itself when a file it covers is opened with Read/Edit/Write — not when one is read through
Bash, which is why `CLAUDE.md` says to read it before changing the area.

Each line is the rule; the chapter it links to is the whole of it.

## Modules

| Path | What lives there |
|---|---|
| `src/lib/history.svelte.ts` | Watch positions, "continue watching", privacy roots, the update-resume snapshot |
| `src-tauri/src/logs.rs` + `src/lib/report.svelte.ts`, `report-text.ts`, `redact.ts`, `console-log.ts` | The issue report. `logs.rs` is the verbose flag (`logs::verbose()`, every protocol log reads it), stderr captured into a ring and passed through, the mpv log's path and tail, the OS, and the names this process has learned that a report must not carry (`logs::remember`). `console-log.ts` is the frontend's ring, installed by `+layout.ts` before anything logs — a leaf. `report.svelte.ts` gathers, `report-text.ts` lays out (pure), and `redact.ts` takes out paths, links, addresses, identifiers and names from the **finished** text (pure, tested against real wire-log lines). See the ninth enforcement point in [rules/sources-and-privacy](../../docs/rules/sources-and-privacy.md) |
| `src-tauri/src/install_count.rs` + `site/installs/` | The installation count on the landing page: the player's one report of its first launch (platform only, no identifier, before the window so an older installation can tell itself apart by `mpv.conf`), and the counter worker — a Durable Object behind the site's worker, separate because a worker with one gets no preview addresses. See the line in [rules/sources-and-privacy](../../docs/rules/sources-and-privacy.md) |
| `src-tauri/src/shell_handler.rs` | The `frameplayer://` protocol and the video associations, registered by the player rather than by an installer — because Windows has no folder-local form of either. Keeps what it displaces (`<ProgId>_backup`, `fp-previous-command`) and gives it back, so a portable copy taking the association does not cost an installed one its own |

## Rules

- **A network source is not a file, and a surprising amount of the player assumes otherwise.** `isNetworkSource()` gates the three that matter: the thumbnail storyboard (it decodes the *whole* file in the background — over a stream … → [sources-and-privacy](../../docs/rules/sources-and-privacy.md)
- **File associations on macOS need UTIs, not extensions**: `bundle.fileAssociations[].contentTypes` → `LSItemContentTypes`. Without it only mp4/mov/avi worked, because modern LaunchServices matches by UTI and `CFBundleTypeExtensions` is a legacy fallback. `public.movie` is the supertype that covers formats declared by other apps (mkv); the explicit UTIs next to it are belt-and-braces. Tauri's association schema is `additionalProperties: false` — no comment keys.
- **Opening files ("Open with", associations)**: two delivery paths that must both work — `argv`/single-instance on Windows, an Apple Event (`RunEvent::Opened`) on macOS — and both funnel into `deliver_files()`, a buffer the frontend drains with `take_pending_files` on the event *and* on window focus. → [sources-and-privacy](../../docs/rules/sources-and-privacy.md)
- **What a source *is* is not how the player reaches it.** Everything remembered about a video — position, chosen tracks, title — is filed under `sourceId()` (source.ts), not under the path. → [sources-and-privacy](../../docs/rules/sources-and-privacy.md)
- **A video's name is asked for only when it is not already free, and only from the site serving it.** mpv supplies `media-title` for anything that plays (yt-dlp resolves the page), so that is recorded into one cache keyed by … → [sources-and-privacy](../../docs/rules/sources-and-privacy.md)
- **The start screen shows the title the player showed, not the file name.** A path is written for sorting, not for reading, and for a link it is a video id with a tracking query attached — so `frameplayer.positions` carries the … → [sources-and-privacy](../../docs/rules/sources-and-privacy.md)
- **The issue report is the ninth privacy enforcement point, and redaction runs over the assembled text, never over known fields.** So no source of lines can reach the output around it; names come in as literals (`logs::remember` where discovery reads them), a device model is kept and its owner's name is not, and nothing leaves until the viewer has seen the exact string — the issue link carries only the version and the OS. The verbose log is one `AtomicBool`, read at every call site, switched at runtime and forgotten at restart; stderr is captured with a passthrough, and the capture's reader must never write to stderr. → [sources-and-privacy](../../docs/rules/sources-and-privacy.md)
- **Watch history & privacy**: a file enters the history only once playback passes `min(15 s, 5 % of duration)` and leaves it again past 97 % — the relative half of that floor is not optional, a flat 15 s made anything shorter than … → [sources-and-privacy](../../docs/rules/sources-and-privacy.md)
- **The installation count is one request in the life of an installation, carrying the platform and nothing else.** `install_count.rs` runs before the main window, because an installation older than the counter is in the site's starting number and knows itself by the `mpv.conf` the frontend writes on every first start. → [sources-and-privacy](../../docs/rules/sources-and-privacy.md)
- **Sending the playing file to the trash acts only on a local file, only to the trash, and only after mpv has let go of it.** Unbound by default; refused for streams, torrents, casting and rooms out loud; `playlist-remove current` → wait for `path` to move → `trash_file` → `forgetRecent`; nothing falls back to a permanent delete (Windows refuses non-fixed drives, where the shell would). → [sources-and-privacy](../../docs/rules/sources-and-privacy.md)
