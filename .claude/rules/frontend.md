---
paths:
  - "src/routes/**"
  - "src/lib/keys.svelte.ts"
  - "src/lib/i18n.svelte.ts"
  - "src/lib/i18n/**"
  - "src/lib/input.svelte.ts"
  - "src/lib/wheel.ts"
  - "src/lib/latest.ts"
  - "src/lib/dom.ts"
  - "src/lib/format.ts"
  - "src/lib/platform.ts"
  - "src/lib/units.ts"
  - "src/**/*.test.ts"
  - "src/lib/test-setup.ts"
  - "vitest.config.ts"
  - "scripts/check-*.mjs"
  - "scripts/css-orphans.mjs"
---

# frontend — the short form

The short form of [`docs/rules/frontend.md`](../../docs/rules/frontend.md): the modules this area is made of,
its rules one paragraph each, and the commands that exercise it. Claude Code loads this file
by itself when a file it covers is opened with Read/Edit/Write — not when one is read through
Bash, which is why `CLAUDE.md` says to read it before changing the area.

Each line is the rule; the chapter it links to is the whole of it.

## Modules

| Path | What lives there |
|---|---|
| `src/routes/+page.svelte` | What is left after the split: hotkeys, the fullscreen veil, the idle/cursor rules, the wheel dispatch, the StepEngine overlay, and the `onPlayerProperty`/`onFileLoaded` bridge that wires mpv state to the UI. It composes the components below rather than drawing them |
| `src/lib/input.svelte.ts`, `overlays.svelte.ts` | Everything the viewer does with a key, a wheel or a pointer, and what is on top of the video. `input` decides *which* thing was asked for; who answers it is `playback`'s (see the row below), which is why this file no longer spells the casting rule out — the one branch it keeps is the click, where the two transports want genuinely different gestures rather than the same gesture aimed elsewhere. `overlays` is the surface stack: at most one OSC menu, and Escape unwinding innermost first. Its existence is what lets `input` take **no hooks from the page at all** — everything that reacts to an overlay was reaching for a different variable before |
| `src/lib/thumbs.svelte.ts`, `zoom.svelte.ts`, `zoom-step.ts`, `screenshot.ts`, `osd.svelte.ts`, `format.ts`, `platform.ts` | Hover thumbnails; zoom/pan maths, and how far one Ctrl+wheel event zooms (a pinch by what the fingers did, a notch by a step — pure, tested); frame export; the OSD popup; pure formatting helpers; `IS_MAC` |
| `src/lib/latest.ts` | "Only the newest attempt may write." A counter behind `latest()` → `begin()` → `run.stale`, guarding the six places where a read outlives what it was about — the track and chapter lists, the queue read, the recents rail, the UPnP probe and the torrent poll. **Clearing a timer does nothing about this**: a promise already in flight resolves regardless, and a slower earlier attempt then lands last and overwrites the newer answer, permanently, since nothing is left to correct it. It had been hand-written in all six, and what a copy loses is never the counter — it is a check after the *second* await, which is why the shape makes each check one word |
| `src/lib/i18n.svelte.ts`, `src/lib/i18n/{ru,en,zh}.ts` | `t()`, `joinList()`, `locale()`/`setLocale()`, and the dictionaries one module per language beside it; `ru.ts` defines the key set and `en`/`zh` are `satisfies Record<MessageKey, string>`, so a missing English or Chinese string is a type error. TypeScript rather than JSON for exactly that. The site reads them as text (`site/src/i18n/app.ts`), one file per language. The Chinese follows [docs/zh-glossary.md](../../docs/zh-glossary.md) |
| `src/lib/keys.svelte.ts` | The hotkey table: `ACTIONS` (44 rows, per-platform defaults, `noRepeat`/`quiet`), chord normalization and labels, the localStorage overrides, and `hint()`/`hintPair()` that every tooltip and menu row reads |

## Rules

- **The player's wheel gestures stop at the edge of any floating surface.** Vertical is volume and horizontal is a scrub — or, with the wheel set to seek, vertical is a seek of the viewer's seconds per notch and Shift+wheel the volume (casting ignores the setting: the TV gets volume only); the decision is the pure `WheelGesture` in `wheel.ts`, tested — and **a mouse has no horizontal axis, so Shift+wheel is remapped to one**, because neither engine delivers it as one: both report Shift+wheel as `deltaY` with `shiftKey` set and do the swap further down their own scrolling code, which never runs over the video since nothing there scrolls. The direction is the browsers' own (down becomes right, hence forward) and therefore needs no platform branch, unlike a real `deltaX`, whose sign macOS has already flipped; the remap is skipped whenever the event brings a horizontal delta of its own, since a tilt wheel, a Windows precision touchpad and a mac trackpad are all already right and Shift gets held over those too. Pressing or releasing Shift mid-gesture re-decides the locked axis at once rather than waiting out `WHEEL_AXIS_RESET_MS`, dropping both accumulators so a part-collected second of scrub does not land as volume. A wheel over the video is right and over a list wrong — scrolling the queue was changing the volume. The rule lives once in `onWheel` (`WHEEL_SURFACES`, matched with `closest`), not as a `stopPropagation` per surface: that list is invisible until the one surface missing from it is found by hand, which is how the queue, the chapter list and the context menu all ended up missing. Ctrl+wheel still calls `preventDefault` everywhere, surface or not — without it the webview zooms the whole UI and there is no way back.
- **A `$derived` re-evaluates the moment it is read again, not at the end of the tick.** So an event handler must read the derived object it is acting on *once*, before it touches anything that derived depends on. → [frontend](../../docs/rules/frontend.md)
- **State lives in `.svelte.ts` modules, not in a store library.** Runes work outside components, so a module exporting a class instance with `$state` fields *is* the store — with per-field reactivity a subscription-based store … → [frontend](../../docs/rules/frontend.md)
- **Splitting further.** The page went from 11 727 lines to ~1 600 across twenty-four components and twenty-two `.svelte.ts` modules, and four silent traps came out of it: a CSS rule whose markup moved, scoped `@keyframes`, a state written as an ancestor, and the cascade order changing with the file. → [frontend](../../docs/rules/frontend.md)
- **A hotkey is a row in a table, and three things move with it.** `ACTIONS` in [keys.svelte.ts](../../src/lib/keys.svelte.ts) is the only place a default binding is written; `onKeydown` normalizes the event to a chord … → [frontend](../../docs/rules/frontend.md)
- **Tests cover the pure end of the codebase, and the choice of what to cover is the point.** `vitest.config.ts` is separate from the app's Vite config because `sveltekit()` brings routing and the `$app/*` virtuals with it; the … → [frontend](../../docs/rules/frontend.md)
