---
paths:
  - "**/*.css"
  - "src/lib/components/**"
  - "src/routes/**/*.svelte"
---

# css — the short form

The short form of [`docs/rules/css.md`](../../docs/rules/css.md): the modules this area is made of,
its rules one paragraph each, and the commands that exercise it. Claude Code loads this file
by itself when a file it covers is opened with Read/Edit/Write — not when one is read through
Bash, which is why `CLAUDE.md` says to read it before changing the area.

Each line is the rule; the chapter it links to is the whole of it.

## Modules

| Path | What lives there |
|---|---|
| `src/app.css` + `src/routes/+layout.svelte` | Document-level styles: the `box-sizing: border-box` reset, fonts, focus rings, the `button, input` font reset, and the `.scrollable` scrollbar skin. They live outside a component because Svelte scopes `<style>` — a rule written in the page stops applying the moment its markup moves into a child component |

## Critical gotchas

(Numbered as in `CLAUDE.md`, which keeps the headline of each.)

9. **A bar's gradient tail must not be part of its hit area.** `.topbar` and `.osc` extend well past their controls to fade the scrim, and while that padding belonged to the element it swallowed clicks on the video. The gradients live in `::before` with `pointer-events: none` (`.osc::before` also needs `z-index: -1` plus `isolation: isolate`), and the elements are sized to their content. → [css](../../docs/rules/css.md)

## Rules

- **Focusing an input scrolls it into view, and that silently undoes whatever scrolling you just arranged.** The subtitle panel's sign-in form unfolds at the bottom of a box that already scrolls (`.settings` is `max-height: 100%; … → [css](../../docs/rules/css.md)
- **Every box is its border box, and every size written down is an outer size.** The reset (`*, *::before, *::after`) lives in app.css. → [css](../../docs/rules/css.md)
- **A flex item does not shrink below its content unless told to.** `min-width: auto` is the default, so `overflow: hidden; text-overflow: ellipsis` on a label inside a flex row never gets the chance to fire: the row grows instead, … → [css](../../docs/rules/css.md)
- **A box sized by its content has two widths in WebKit once it scrolls, and the pointer is what switches between them.** An `overflow: auto` bar is left out of a shrink-to-fit width on the first layout and counted on the next, so a panel opened 10px narrow with its names cut and grew at the first hover. `MenuBody` makes it `overflow-y: scroll` while there is something to scroll. → [css](../../docs/rules/css.md)
- **A paragraph inside a panel sized by its content asks for its whole length on one line.** A shrink-to-fit panel is as wide as its widest max-content row, and a wrapping hint's max-content is the sentence unbroken — the cast menu went from 314px to its 520px cap when AirPlay's options opened. `.tv-settings` is `contain: inline-size`, so it takes the panel's width instead of proposing one; `.cast-hint` caps itself with `max-width`. → [css](../../docs/rules/css.md)
- **Reusing a class means beating it, and equal weight is decided by source order — this has now cost two bugs.** The queue rows lost `display: flex` to `.menu-item`'s `display: block` that way; the torrent list's delete button … → [css](../../docs/rules/css.md)
- **A surface that has faded out must leave the page, not stay transparent.** WebKit paints a layer at `opacity: 0` like a visible one, and the hidden control bar's seekbar was written on every video frame — ~200 MB in WebKit's GPU process for the whole of playback, on macOS. `faded()` adds a `gone` class (`visibility: hidden`) a timer after the fade; a delayed `visibility` *transition* hangs while the app is not frontmost. → [css](../../docs/rules/css.md)
- **Animating `opacity` on a small glyph makes it twitch, and no amount of measuring geometry will find it.** The torrent list's delete cross was reported "jumping and coming back" on hover. → [css](../../docs/rules/css.md)
- **Accent colors**: indigo `#6366f1` (fills) / `#818cf8` (hover/active/focus); white text on accent surfaces. → [css](../../docs/rules/css.md)
