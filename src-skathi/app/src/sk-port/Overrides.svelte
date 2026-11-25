<!-- Version 1's stylesheet as Skathi needs it written. Each rule makes Skathi draw what WebKit and
     WebView2 draw from `src/` unchanged; none of them reaches version 1, which never loads this file.
     Measured against the same build in Playwright WebKit and Chromium. -->
<style>

  /* Windows text. sk.config.json keeps `"textRendering": "optimized"` (DirectWrite, grayscale here:
     the window is not opaque, and WebView2 draws version 1's text grayscale too). Measured at
     Skathi a88f6683 against Edge 158 drawing the same `src/`,
     126 text elements over the title bar, the control panel, the context menu, the settings sheet
     and the start screen in en/ru/zh:
     - optimized alone: 0.97 of Edge's ink, light text on dark lightest (control panel 0.91, title 0.94);
     - with `font-dilation: luminance`: 1.03 (panel 0.99, title 1.03), the same pixel error (9.7
       against 9.5 levels) and the same widths;
     - skathi-mode: 1.14 of Edge's ink and 60 % more pixel error (heavier, unhinted); with
       `font-smoothing: antialiased` 0.90, still 15 levels;
     - `font-smoothing: subpixel-antialiased`: ClearType fringes on 57-68 % of text pixels, none in Edge;
     - `line-metrics: snapped`: every line box a pixel taller, context menu rows 3.6-5.2 px off.
     macOS draws this weight itself (CoreText), so the rule is Windows' only (tauri-stub.ts). */
  :global(html[data-sk-windows]) {
    font-dilation: luminance;
  }

  /* Gone with Skathi 0.8.5 (a88f6683): the root `line-height: normal` (Skathi starts at `normal`
     since skathi#98) and the Chinese `line-height: 1.4` with its Latin exceptions (a fallback font
     grows a normal line since skathi#106). Measured in the settings sheet against Edge 158: with
     the root rule or without it, en and ru rows are the same to the pixel; the zh rules made a
     label 17.33px against Edge's 16 and the tab row 34 against 32, and without them it is 16 and 32. */

  /* An auto-width button in block flow is shrink-to-fit in CSS; Skathi gives an inline-level box
     among blocks a line of its own at the container's width (its documented INLINE_DISPLAY_NOTE),
     and `width: fit-content` / `max-content` are ignored. The settings sheet's "Reset brightness
     and colour" button was 540px wide against 189. In an explicit flex column it can align to the
     start, which Skathi honours. */
  /* The cast panel's "looking for devices" spinner sits beside its label by `vertical-align`, which
     needs a line box: Skathi does not have one. A flex row centres it instead; the row also drops the
     space after the spinner that an inline row kept, so the margin takes that space in (7px + one
     space of 12.5px text = 10px). Version 1 keeps its inline row: in a browser the flex row draws the
     spinner 0.5-1px higher (measured in Chromium and WebKit), so the rule lives here. */
  :global(.cast-empty) {
    display: flex;
    align-items: center;
  }
  :global(.cast-empty > .cast-spin) {
    flex: none;
    margin-right: 10px !important;
  }

  :global(.setting.peek-keep) {
    display: flex;
    flex-direction: column;
  }
  :global(.setting.peek-keep > .adj-reset) {
    align-self: flex-start;
  }

  /* A browser's own stylesheet centres a button's text (`button { text-align: center }`); Skathi
     has no such default, so every pill of a `.segmented` row, the keys tab's "+" and every other
     button whose text is not in a flex box started at its left padding (measured at Skathi a88f6683 and
     8a9b3acb: "Never" 10 device px from the left, Chromium 109 of 109+108). An element selector,
     as the UA's is, so every class rule in `src/` that sets its own alignment still wins
     (`.segmented.vertical .segopt` is left-aligned on purpose). */
  :global(button) {
    text-align: center;
  }

  /* The playback tab's "other" field (PlaybackTab.svelte `.step-custom`). Skathi does not paint a
     `:focus-within` rule (focus-within.ts): the ring showed at rest and not on focus. It also kept
     the `:focus-visible` outline the field's `input:focus { outline: none }` removes, so a focused
     field had a rectangle inside it (measured at Skathi a88f6683, a minimal page and the tab). Without the
     pseudo-classes, as a class and a plain `outline: none`, both are as WebView2 draws them
     (measured). `!important` because the scoped rules weigh three classes. */
  :global(.step-custom) {
    box-shadow: none !important;
  }
  :global(.step-custom.sk-focus-within) {
    box-shadow: 0 0 0 1px #818cf8 !important;
  }
  :global(.step-custom input) {
    outline: none !important;
  }

  /* `--ui-shadow-drop` (the logo and the window buttons over the video, +page.svelte) is two
     drop-shadows; Skathi keeps the first and drops the second, the 6px halo ("dropped a second
     drop-shadow()"), and the logo's shadow came out at 62% of WebView2's darkness. One shadow,
     a little wider and darker, is 90% (measured on a white strip at a88f6683, Edge 158). */
  :global(.player) {
    --ui-shadow-drop: drop-shadow(0 1px 2.5px rgba(0, 0, 0, 0.75)) !important;
  }
</style>
