<script lang="ts">
  /// The tag on the audio and subtitle buttons, and on their rows in the
  /// folded panel — one component so the two cannot drift apart, and so the
  /// rule lives with the markup it styles. What it says is `track-badge.ts`.
  import type { TrackBadge } from '$lib/track-badge';

  interface Props {
    badge: TrackBadge;
    /// Pinned to the lower-right corner of the button it sits in, rather than
    /// in the flow of a menu row.
    corner?: boolean;
    /// The tag's own width, for the glyph that has to make room for it.
    width?: number;
  }

  let { badge, corner = false, width = $bindable(0) }: Props = $props();
</script>

<span class="tag" class:on={badge.on} class:corner bind:offsetWidth={width} aria-hidden="true">{badge.text}</span>

<style>
  /* Dark rather than white when off: two white chips side by side pulled the
     eye off the picture, and the dark one still holds over a bright frame
     because the bar's scrim is under it. A shade lighter than the scrim at the
     bottom of the bar, so the plate is a shape and not only its letters. */
  .tag {
    display: inline-block;
    height: 12px;
    padding: 0 3px;
    border-radius: 3px;
    font-size: 8.5px;
    font-weight: 600;
    line-height: 12px;
    letter-spacing: 0.03em;
    font-variant-numeric: tabular-nums;
    background: #3a3a46;
    color: #e8e8ec;
    pointer-events: none;
  }

  .tag.on {
    background: #6366f1;
    color: #fff;
  }

  /* Over the glyph's lower-right corner. Absolutely positioned, so it adds
     nothing to the button's width — the arithmetic that folds the right cluster
     never sees it. What keeps it apart from the glyph is not a rim drawn round
     it (a dark ring read as a frame, worst over a bright picture) but a hole
     the glyph leaves for it, so the gap shows the picture itself: `tagCut` in
     Controls.svelte, which is written against these three numbers. */
  .tag.corner {
    position: absolute;
    right: 0;
    bottom: 2px;
  }
</style>
