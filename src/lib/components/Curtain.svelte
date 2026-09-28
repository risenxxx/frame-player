<script lang="ts">
  /// The dark between two pictures, where the web view is what draws it — see
  /// `curtain.svelte.ts` for when it comes down, what lifts it, and why macOS
  /// fades the picture itself instead.
  ///
  /// The first child of the player on purpose: every other surface comes later
  /// in DOM order and therefore paints over it, so the bars, the loading plate
  /// and the dialogs stay lit with no `z-index` anywhere. It takes no clicks —
  /// a click on the video during a change is still a click on the video.
  import { curtain } from '$lib/curtain.svelte';
</script>

{#if curtain.drawn}
  <div class="curtain" class:on={curtain.on} style:--curtain-ms="{curtain.ms}ms" aria-hidden="true"></div>
{/if}

<style>
  /* The transition in force is the one of the state being *entered*, so the
     two directions get a curve each. Coming off, it starts slowly: the bars
     beside the new picture are widest at the start of the window's morph, and
     that is the part of it to keep dark. */
  .curtain {
    position: absolute;
    inset: 0;
    background: #000;
    opacity: 0;
    pointer-events: none;
    transition: opacity var(--curtain-ms, 0ms) cubic-bezier(0.4, 0, 1, 1);
  }

  .curtain.on {
    opacity: 1;
    transition-timing-function: ease-out;
  }
</style>
