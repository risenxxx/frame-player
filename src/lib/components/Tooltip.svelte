<script lang="ts">
  /// The custom tooltip. One delegated mechanism serves every `[data-tip]` in
  /// the app; the page owns the hover tracking and hands the placed result here.
  /// Laid out at the window origin and moved with a `transform` on purpose: a
  /// box laid out at `left: x` may only use the width to its right, so a tip
  /// near the edge wrapped to one word per line and the measure-then-clamp pass
  /// that followed read the *wrapped* width and called it a fit.

  interface Props {
    tooltip: { text: string; pos: { x: number; y: number } | null };
    el: HTMLDivElement | undefined;
  }

  let { tooltip, el = $bindable() }: Props = $props();
</script>

<div
  class="tip"
  bind:this={el}
  style={tooltip.pos
    ? `transform: translate3d(${tooltip.pos.x}px, ${tooltip.pos.y}px, 0)`
    : 'visibility: hidden'}
>
  {tooltip.text}
</div>

<style>
  .tip {
    position: fixed;
    /* Placed by a transform from the window's origin, never by left/top: a box
       laid out at `left: x` may only use the width remaining to its right, so
       near the right edge the text wrapped one word per line and the measure
       that followed saw the wrapped width as the natural one. See floating.ts. */
    left: 0;
    top: 0;
    /* A path is the one tip that can be longer than the window, and it is never
       truncated: the useful part is its tail, so an ellipsis would hide exactly
       what is being looked for. It wraps instead.
       360px rather than 520: at the wider size a long path made one enormous
       line that read as a paragraph rather than a label. `anywhere` rather than
       `break-word` because a path or a release name is frequently a single
       unbroken token with no space to break at — `break-word` would leave it
       overflowing the box it was given.
       380 is that 360 plus the tip's own 18px of padding and 2px of border:
       the same box on screen, written as its outer size. The viewport term
       becomes honest at the same time — it was meant to keep 16px clear of
       each edge and, as a content width, kept 6px. */
    max-width: min(380px, calc(100vw - 32px));
    overflow-wrap: anywhere;
    background: rgba(14, 14, 20, 0.95);
    border: 1px solid rgba(255, 255, 255, 0.09);
    border-radius: 7px;
    color: #d6d6de;
    font-size: 12px;
    padding: 5px 9px;
    pointer-events: none;
    z-index: 90;
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.4);
  }
</style>
