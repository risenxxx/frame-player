---
paths:
  - "src/lib/components/**"
  - "src/lib/settings/**"
  - "src/lib/osd.svelte.ts"
  - "src/lib/overlays.svelte.ts"
  - "src/lib/floating.ts"
  - "src/lib/tooltip.svelte.ts"
  - "src/lib/faded.svelte.ts"
  - "src/lib/links.ts"
---

# ui-surfaces — the short form

The short form of [`docs/rules/ui-surfaces.md`](../../docs/rules/ui-surfaces.md): the modules this area is made of,
its rules one paragraph each, and the commands that exercise it. Claude Code loads this file
by itself when a file it covers is opened with Read/Edit/Write — not when one is read through
Bash, which is why `CLAUDE.md` says to read it before changing the area.

Each line is the rule; the chapter it links to is the whole of it.

## Modules

| Path | What lives there |
|---|---|
| `src/lib/components/` | The UI, one file per surface: `Dialog` (the shell all seven dialogs share), `SettingsDialog` (the sheet and its tab row; each tab is its own component under `settings/` — `GeneralTab`, `VideoTab`, `PlaybackTab`, `AudioTab`, `SubsTab`, `TorrentsTab`, `TvTab`, `KeysTab` — and the three mpv-backed ones share `MpvRows` and `ConfFoot`, with the mpv.conf values in `src/lib/settings/mpv-settings.svelte.ts`, one instance per opening), `SubsDialog`, `LinkDialog`, `MediaInfoDialog`, `DiagnosisDialog`, `TorrentPickDialog`, `TorrentUpdateDialog`, `ReportDialog`, `StartScreen` (the frame and the torrent rows' delete menu, which is drawn outside the scrolling overlay; its parts are under `start/` — `OpenPanel` with `CatalogButton`, `RecentRail`, `TorrentList`), `EndScreen`, `ContextMenu`, `CastMenu` with `CastIcon` (the row's device kind, from `deviceKind` — only what the device announces), `TopBar`, `Osd`, `Tooltip`, `SkipButton`, `LoadingOverlay`, `CastScreen`, `StepOverlay`, `Curtain`. **A rule and the markup it styles must live in the same file** — Svelte scopes `<style>`, and the compiler's `unused CSS selector` warning does *not* catch a rule left behind, because a `<Child />` tag is opaque to that analysis. `bun run css-orphans` is what catches it, in both directions — a rule left behind, and the mirror image, a class whose **base** rule stayed in another component while one of its users moved out. The second half is the one a diff of the built stylesheet structurally cannot see, because such a comparison has to normalize the scope hash away to be readable at all, and to it `.speedopt.svelte-a` and `.speedopt.svelte-b` are the same rule — which is how five controls (the delay stepper's −/+, three dialog footers, the external-subtitle rows) came to render as unstyled native widgets under a clean report |

## Rules

- **mpv option surface**: defaults are set in `initialOptions` in `initPlayer` (player/session.ts); user overrides come from `%APPDATA%/<identifier>/mpv.conf` (command `user_mpv_conf`, template in lib.rs, `wid` filtered out). → [ui-surfaces](../../docs/rules/ui-surfaces.md)
- **Returning to the start screen is a destination, not a blank to wait out.** `showEmpty` is debounced by 300 ms because a playlist transition blanks `filename` for a moment and the picker must not flash through it. → [ui-surfaces](../../docs/rules/ui-surfaces.md)
- **A control's actions belong to the control, not to the list around it.** The delay stepper's reset used to be a `.menu-item` under the −/+ pill, which is correct for a list row and wrong beside a control: a menu row is … → [ui-surfaces](../../docs/rules/ui-surfaces.md)
- **The settings dialog has two control shapes, and the choice between them is about the value, not the layout.** A pill (`.segmented`) picks one of several *named* values; a slider covers a continuous range. → [ui-surfaces](../../docs/rules/ui-surfaces.md)
- **A slow operation must say so before it starts, not after it finishes.** Opening a URL and encoding a screenshot both used to pass in silence; `showOsd`'s `sticky` is what raises a popup that cannot fade before the work ends, and anything raising one owes it a replacement on every exit path. → [ui-surfaces](../../docs/rules/ui-surfaces.md)
- **Start screen recents**: `RECENT_LIMIT` cards, candidates drawn from a pool 3× that size — a dead file (renamed, deleted, unplugged drive) is replaced by the next candidate rather than leaving a gap, and is *not* purged from the … → [ui-surfaces](../../docs/rules/ui-surfaces.md)
- **The start screen must be complete in its first frame.** `loadRecent` and `refreshTorrents` used to be fired un-awaited before `show()`, so the window appeared holding the panel and its two buttons and then grew a rail and a … → [ui-surfaces](../../docs/rules/ui-surfaces.md)
- **Start-screen layout must not depend on its own content.** `.overlay` is a `grid` with `place-items: center`, which sizes the column from content — so the recents grid was measured from the cards, and the cards changed as … → [ui-surfaces](../../docs/rules/ui-surfaces.md)
- **Placing a floating box is three rules, and they belong in one place.** Flip to the other side of the anchor when the preferred one has no room, shift along the other axis to stay inside, cap the size when the box is simply … → [ui-surfaces](../../docs/rules/ui-surfaces.md)
- **The OSC panels are anchored to the bar, not to a number somebody added up.** `.menu`'s `bottom` is `calc(100% + 12px)` — `.osc`'s top edge plus the same gap the timestamps already keep above the seekbar … → [ui-surfaces](../../docs/rules/ui-surfaces.md)
- **Each side of the window has one edge, 20px in, measured to the glyph.** The seekbar, the end glyphs of the control row (hence `.controls { margin-inline: -11px }`), the logo, the track menus, the skip button, the readouts and the OSD all end on it, because the traffic lights and the Windows close glyph already do and cannot move. → [ui-surfaces](../../docs/rules/ui-surfaces.md)
- **The control bar's right-hand cluster folds into one button, and what decides it is arithmetic rather than a breakpoint.** How many buttons that cluster carries is a property of the file, so the width where it pushes play off centre is too — `Controls` measures the row and folds when … → [ui-surfaces](../../docs/rules/ui-surfaces.md)
- **A link promises navigation; an action gets a button.** `.settings-link` is for "show mpv.conf in Finder" and nothing else; the website and the repository in Settings → About are real `<a href>`s handed to the system browser (`src/lib/links.ts`), because they do go somewhere. Destructive actions are `.btn-danger`, and secondary actions next to a `.primary` — "open a link", "install yt-dlp" — are `.btn-outline`: they open a dialog or download a binary, which is not navigating anywhere.
- **The hover preview keeps the video's own aspect, not 16:9.** mpv reports `dwidth`/`dheight` — the size with the aspect already applied — the moment a file opens, so the box can be shaped correctly before any frame exists. → [ui-surfaces](../../docs/rules/ui-surfaces.md)
- **A scrolling box says it has more below it, and the shape of that is forced rather than chosen.** `ScrollFade` is a bottom gradient with a chevron, rendered as the **last child of the container that scrolls** — one line per list … → [ui-surfaces](../../docs/rules/ui-surfaces.md)
- **An OSC panel says it at both ends, and with a mask where the dialogs paint.** `MenuBody` is the list inside every panel over the control bar; `.menu` is only the surface. Paint over a see-through panel is a darker band (16 against 30 over white), so the rows fade under a mask instead — which is why the list is a box of its own — with the scrollbar kept out of the mask, the chevrons outside the list, and `scroll-padding` keeping the entry being played clear of the fade. Only while the panel stands at its full height (`--menu-cap`): in a window that cuts it short, and in the context menu, there are no edges. → [ui-surfaces](../../docs/rules/ui-surfaces.md)
- **Tooltips**: put `data-tip="..."` (+ matching `aria-label`) on any element — a delegated handler renders the custom tooltip. → [ui-surfaces](../../docs/rules/ui-surfaces.md)
- **OSD**: `showOsd(text, { sub, progress, sticky })` — a label, an optional bar and an optional smaller second line, up for 1.2 s. No icons, and it sits at `z-index: 80`, above the dialog backdrop, which is safe only because it takes no clicks. → [ui-surfaces](../../docs/rules/ui-surfaces.md)
