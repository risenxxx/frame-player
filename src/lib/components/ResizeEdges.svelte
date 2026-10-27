<script lang="ts">
  /// The window's resize strips — Windows only (see the note in the markup).
  /// The page's root says which platform it is on (`.player.mac`), so the
  /// rules that read it are written against that ancestor with `:global`,
  /// which keeps the markup exactly what it was.
  import { chrome, startResize } from '$lib/chrome.svelte';
  import { IS_MAC } from '$lib/platform';
</script>

<!-- Windows only, and that is not a tidy-up: `drag_resize_window` is
     **unimplemented on macOS** — tao returns `NotSupported` and Tauri
     discards the error (`let _ = ...`), so these strips caught the press and
     then did nothing at all. Worse than nothing, in fact: they are 5px wide,
     sit inside the window above everything (`z-index: 50`) and draw their own
     resize cursors, so they took over the band AppKit uses for its own edge
     resizing and turned it into a dead zone that still looked live. The
     symptom was a resize that worked only when the pointer happened to land
     in the system's band outside the frame. macOS needs none of this: the
     window is `Titled | Resizable`, and AppKit resizes it from edges and
     corners the DOM cannot reach anyway. -->
{#if !IS_MAC && !chrome.fullscreen && !chrome.isMaximized}
  <div class="rz rz-n" role="presentation" onpointerdown={() => startResize('North')}></div>
  <div class="rz rz-s" role="presentation" onpointerdown={() => startResize('South')}></div>
  <div class="rz rz-e" role="presentation" onpointerdown={() => startResize('East')}></div>
  <div class="rz rz-w" role="presentation" onpointerdown={() => startResize('West')}></div>
  <div class="rz rz-ne" role="presentation" onpointerdown={() => startResize('NorthEast')}></div>
  <div class="rz rz-nw" role="presentation" onpointerdown={() => startResize('NorthWest')}></div>
  <div class="rz rz-se" role="presentation" onpointerdown={() => startResize('SouthEast')}></div>
  <div class="rz rz-sw" role="presentation" onpointerdown={() => startResize('SouthWest')}></div>
{/if}

<style>
  .rz {
    position: fixed;
    z-index: 50;
  }

  .rz-n { top: 0; left: 10px; right: 10px; height: 5px; cursor: n-resize; }
  .rz-s { bottom: 0; left: 10px; right: 10px; height: 5px; cursor: s-resize; }
  .rz-e { right: 0; top: 10px; bottom: 10px; width: 5px; cursor: e-resize; }
  .rz-w { left: 0; top: 10px; bottom: 10px; width: 5px; cursor: w-resize; }
  .rz-ne { top: 0; right: 0; width: 10px; height: 10px; cursor: ne-resize; }
  .rz-nw { top: 0; left: 0; width: 10px; height: 10px; cursor: nw-resize; }
  .rz-se { bottom: 0; right: 0; width: 10px; height: 10px; cursor: se-resize; }
  .rz-sw { bottom: 0; left: 0; width: 10px; height: 10px; cursor: sw-resize; }

  /* Windows: the top-right corner belongs entirely to the window buttons — a
     flick of the cursor into the screen corner of an edge-snapped window must
     hit the close button, not a resize zone. Corner resizing is not lost: it
     stays on the invisible native rim (~7px) just outside the window edge.
     144px = three 48px buttons. */
  :global(.player:not(.mac)) .rz-n { right: 144px; }
  :global(.player:not(.mac)) .rz-e { top: 48px; }
  :global(.player:not(.mac)) .rz-ne { display: none; }
</style>
