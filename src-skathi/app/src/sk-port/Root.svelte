<script lang="ts">
  import Layout from '../../../../src/routes/+layout.svelte';
  import Page from '../../../../src/routes/+page.svelte';
  import Overrides from './Overrides.svelte';
</script>

<!-- mpv's picture, under the whole interface: what Tauri's transparent window showed through.
     The host fills one of these, by route:
     - `<sk-surface name="video">`: the element's whole layout at about the video's pixel count,
       stretched over the element (Skathi's upscale, `image-rendering: auto`);
     - `<sk-surface name="overlay">`: subtitles and mpv's OSD drawn for the element's size, straight
       alpha. On Windows it is the bottom third of the video until the host reports text above
       that (`data-sk-overlay-band`, `data-sk-overlay-full`, set by tauri-stub.ts);
     - `<sk-layer name="video">`: a native layer of the host's under this element (the macOS child
       route, the Windows `sklayer` route). It paints nothing on the other routes. -->
<sk-layer name="video" class="sk-video"></sk-layer>
<sk-surface name="video" class="sk-video"></sk-surface>
<sk-surface name="overlay" class="sk-video sk-overlay"></sk-surface>
<div class="sk-ui"><Layout><Page /></Layout></div>
<Overrides />

<style>
  .sk-video {
    position: fixed;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: fill;
    image-rendering: auto;
  }
  .sk-overlay {
    pointer-events: none;
  }
  :global(html[data-sk-overlay-band]:not([data-sk-overlay-full])) .sk-overlay {
    top: auto;
    bottom: 0;
    height: calc(100% / 3);
  }
  .sk-ui {
    position: fixed;
    inset: 0;
  }
  /* FPUI_FS_ANIM (tauri-stub.ts `setFullscreen`): the picture moves on its own, so the interface is
     away for the transition, and the page's black veil for as long as the flag is on (it comes down
     before the stub hears of the transition). The veil by name: `visibility: hidden` on `.sk-ui` did
     not take its fixed-position veil with it (measured: the veil's fade over the picture). */
  :global(html[data-fs-anim]) .sk-ui {
    opacity: 0;
  }
  :global(html[data-fs-anim-on] .veil) {
    display: none !important;
  }
  /* Tauri's drag attribute (TopBar, the start screen) as Skathi's drag region (titleBarStyle "hidden"). */
  :global([data-tauri-drag-region]) {
    app-region: drag;
  }
  :global([data-tauri-drag-region] button), :global([data-tauri-drag-region] input), :global([data-tauri-drag-region] a) {
    app-region: no-drag;
  }
</style>
