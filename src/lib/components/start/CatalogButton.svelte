<script lang="ts">
  /// The start screen's catalog button: the third way in, marked out by a
  /// gradient hairline. Drawn only when the catalog is switched on — the panel
  /// decides that, this only draws the button.
  import { t } from '$lib/i18n.svelte';

  interface Props {
    onOpenCatalog: () => void;
  }

  let { onOpenCatalog }: Props = $props();
</script>

<button class="btn-outline btn-catalog" onclick={onOpenCatalog}>
  <!-- The gradient hairline, drawn as a stroke rather than painted as
       the difference between two rectangles — see the rule below for
       what that difference cost on Windows. The rect is deliberately
       bare: its geometry has to be written in `calc()`, which an
       attribute cannot carry, so all of it lives in the stylesheet.
       The stops take their colours from custom properties, which is
       what lets `:hover` brighten the ring without a second gradient to
       keep in step. -->
  <svg class="ring" aria-hidden="true">
    <defs>
      <!-- Horizontal, where the ring used to be written at 115°, and
           the tilt is dropped rather than approximated. An
           objectBoundingBox gradient is skewed by the box's aspect and
           a CSS one is not, so the angle cannot simply be carried
           over — and what it is worth here is small: measured on the
           118 × 38 box this renders at, the 115° line is 123px long of
           which the vertical term is 16, so the colour differs by 0.13
           of the ramp between the top edge and the bottom one. Those
           two edges are 38px apart and never seen against each other,
           which is what makes an eighth of the ramp invisible. -->
      <linearGradient id="fp-catalog-ring" x1="0" y1="0" x2="1" y2="0">
        <stop offset="0" style="stop-color: var(--ring-a)" />
        <stop offset="0.45" style="stop-color: var(--ring-b)" />
        <stop offset="1" style="stop-color: var(--ring-c)" />
      </linearGradient>
    </defs>
    <rect />
  </svg>
  <!-- A magnifier at the same 24-unit box and 2px stroke as the chain
       beside it. The handle stops short of the corner on purpose: run
       to 20,20 it reads as a longer glyph than the chain and the two
       stop looking like one set. -->
  <svg viewBox="0 0 24 24" aria-hidden="true">
    <g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
      <circle cx="10.5" cy="10.5" r="6.5" />
      <path d="M15.4 15.4 19.5 19.5" />
    </g>
  </svg>
  {t('start.catalog')}
</button>

<style>
  /* The catalog, marked out by a gradient hairline.
     ───────────────────────────────────────────────────────────────────────
     **Why not simply colour it in.** Indigo means one thing in this player —
     selected / on / primary / focused — so a second indigo *fill* beside
     "Открыть файл" would be two primaries, i.e. none. A 1px gradient ring is a
     different token from a flat fill and can therefore carry a different
     meaning: *this one is worth knowing about*. It is the one place a second
     hue appears in the palette, and the gradient is what makes it read as a
     gradient at all — indigo shading into indigo is invisible at a hairline,
     so the violet end is doing the work rather than decorating.

     **Drawn as an SVG stroke, and the two shapes it is not are both worth
     knowing.** The usual `padding-box`/`border-box` double-gradient trick needs
     an opaque fill behind the interior, and this button's fill is transparent
     and *changes on hover* — so it would have to hardcode the start screen's
     own colour and then track the hover state as well.

     The shape that shipped first was a masked `::before`: a gradient rectangle
     with its middle punched out by `mask-composite: exclude`, i.e. the ring was
     the **difference between two rectangles** rather than a drawn line. Its
     thickness was therefore never painted anywhere — it was whatever survived
     after the outer (`border-box`) and inner (`content-box`) mask rects had
     each been snapped to the device-pixel grid, independently. That is exact on
     macOS, where the device ratio is 2 and a 1px ring is two whole device
     pixels, and it falls apart on Windows: the button sits at a fractional
     offset by construction (`.start-actions` centres a wrapping flex row, so
     any odd difference of widths puts it on a half pixel), and Windows adds a
     fractional device ratio at 125 %/150 % on top. Reported from a Windows
     build: the ring **thinner along the bottom and clipped at every corner but
     the top-left** — which is exactly the signature of that snapping, since the
     box origin rounds outward and keeps its pixel while the far edges round
     inward and lose theirs, and the corners, where the ring is drawn by the
     rounded clip alone against a square inner rect, lose the most.

     A stroke has no such arithmetic: it is one line with a width, antialiased
     evenly on all four sides whatever fraction its box lands on. Measured
     against the built stylesheet in both engines — headless Chrome at ratios 1,
     1.25 and 1.5, a WKWebView harness at 2 — with the paint integrated over
     each 8×8 corner quadrant and normalised to one fully covered pixel of the
     same line, so a stroke, a mask and an ordinary CSS border are directly
     comparable (a 1px line around a quarter arc of radius 7.5 is π/2 · 7.5 ≈
     11.8 whatever draws it):

       ratio 1.25, Blink   mask 5.7–8.9  stroke 10.1–11.1  border 7.1–7.8
       ratio 1,    Blink   mask 10.4–11.2  stroke 10.3–10.4  border 10.3–10.5
       ratio 2,    WebKit  mask 11.1–11.3  stroke 11.7–11.8  border 11.8

     The middle row is the bug: at 1.25 the mask keeps 8.9 at the top-left
     corner and 5.7 at the bottom-right, i.e. it loses over a third of the
     corner, and only there. The sides say the same thing — at 1.25 the mask
     paints its left edge over two full device pixels and each of the other
     three over one, which is the "bottom is a pixel thinner" half of the
     report. What the stroke buys is evenness rather than perfection: its four
     sides land within 3 % of each other at every ratio measured and its corners
     within 16 % at the worst of them, against the mask's 36 % — and at ratios 1
     and 2 its corners are an ordinary border's to the tenth.

     Not animated, deliberately. A slowly rotating gradient is the obvious next
     step and it is wrong here: this is the screen the player sits on whenever
     nothing is playing, and permanent motion in the corner of the eye is
     fatiguing rather than inviting. */
  .btn-outline.btn-catalog {
    position: relative;
    /* The ring replaces the plain border rather than sitting inside it. The
       geometry does not move: the border box is still 1px larger than the
       padding box, so this button is exactly the height of the one beside it. */
    border-color: transparent;
    color: #e8e8ec;
    --ring-a: #818cf8;
    --ring-b: #6366f1;
    --ring-c: #a855f7;
  }

  /* Brighter on hover, in step with the fill `.btn-outline` already lightens.
     Written as three classes so it beats `.btn-outline:hover` rather than
     relying on which stylesheet the bundler emitted last — the fight this
     project has lost twice already. The gradient itself is untouched: the stops
     read these properties, so there is one ramp rather than two to keep in
     step. */
  .btn-outline.btn-catalog:hover:not(:disabled) {
    border-color: transparent;
    --ring-a: #a5b4fc;
    --ring-b: #818cf8;
    --ring-c: #c084fc;
  }

  /* Three classes, because this has to beat both `button.btn-outline svg`
     (0,1,2) and `.btn-outline.btn-catalog svg` (0,2,1) below — the ring is an
     `<svg>` inside a `.btn-outline`, so every rule written for the glyphs finds
     it too. */
  .btn-outline.btn-catalog svg.ring {
    position: absolute;
    /* **`-1px`, not `0`.** An absolutely positioned box resolves its insets
       against the containing block's *padding* box, so `inset: 0` would land
       inside the button's own 1px border — a full pixel further in than the
       plain border of the button standing next to it (measured in a WKWebView
       harness against the built stylesheet back when this was a `::before`: the
       gradient sat at +1.0px from the left edge). `-1px` puts this box on the
       border box, where the sibling's border is. */
    inset: -1px;
    /* **The size is written out, and `auto` is not the same thing here.** An
       `<svg>` is a *replaced* element, so `width: auto` means its intrinsic
       size — the SVG default of 300 × 150 — rather than "fill the insets", and
       an over-constrained absolute box then drops `right`/`bottom` instead of
       the width. Measured in headless Chrome against the built stylesheet: with
       the insets alone the ring's left and top edges landed on the button and
       its right and bottom edges were 300px and 150px away, i.e. off the button
       entirely — a 6px sweep inward from either found nothing painted at all.
       (app.css's `svg { width: 14px }` had been hiding that, which is why it
       also has to be overridden rather than deleted.) Percentages resolve
       against the containing block, which for an absolute box is the *padding*
       box — 2px narrower than the border box — hence `+2px`. */
    width: calc(100% + 2px);
    height: calc(100% + 2px);
    /* The glyph rule dims its siblings to 0.8; a ring is not decoration beside
       the words, it is the mark. */
    opacity: 1;
    /* An absolutely positioned child paints above the in-flow ones — the lesson
       the bars' scrim gradients already carry. Only the hairline is painted, so
       nothing covers the label; this is belt and braces for the click. */
    pointer-events: none;
    /* **`visible`, or the right and bottom edges are half a line.** An `<svg>`
       clips its content to its own box, and that clip is snapped to device
       pixels while the stroke is not — so whenever the button lands on a
       fraction of a device pixel, the far edge of the clip rounds inward and
       cuts into the stroke's outer half. The button lands on one routinely:
       `.start-actions` centres a wrapping row of text-sized buttons, and in
       Russian the catalog sits at x = 490.66 — screenshot at ratio 2, the
       right side drawn at half the width of the left. The stroke is already
       inside the box by construction, so nothing is lost by not clipping. */
    overflow: visible;
  }

  /* **The half pixel lives here, in SVG user space, and not in the element's
     own position — that is the difference between a crisp hairline and a soft
     one.** A 1px stroke is centred on its path, so its centreline has to sit
     half a pixel inside the box for the line to cover exactly one device pixel;
     writing that half into the *layout* (`left: -0.5px`) does not survive,
     because Blink snaps a box's paint offset to whole pixels — measured in
     headless Chrome, the stroke then straddled the padding-box edge and came
     out as two pixels at 44 % each, i.e. dimmer than the border beside it,
     which is what this whole change exists to avoid. Inside the SVG nothing
     snaps: the geometry is user units on a box whose own origin is already
     integral. `rx` is 7.5 for the same reason — plus the stroke's half-width it
     makes the button's own 8. Geometry as CSS rather than as attributes because
     `calc()` is needed and attributes do not take it. */
  .btn-outline.btn-catalog svg.ring rect {
    x: 0.5px;
    y: 0.5px;
    width: calc(100% - 1px);
    height: calc(100% - 1px);
    rx: 7.5px;
    fill: none;
    stroke: url(#fp-catalog-ring);
    stroke-width: 1px;
  }

  /* The glyph carries the accent too, at full strength rather than the 0.8 its
     siblings get: on this button the icon is part of the mark, not decoration
     beside the words. Two classes, so it beats `.btn-outline svg`. */
  .btn-outline.btn-catalog svg {
    color: #a5b4fc;
    opacity: 1;
  }
</style>
