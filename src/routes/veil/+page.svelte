<!-- Shutter window: covers the monitor while the main window is being resized,
     hiding DWM composition artefacts (see architecture.md, the fullscreen lesson) -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';

  // The main window sends the colour right before showing the shutter (start
  // screen — #101016, video — black): releasing the shutter relies on an
  // invisible seam with the veil, so the colours must match. Both html and
  // body are painted — body sits on top.
  onMount(() => {
    const unlisten = listen<string>('veil-color', (e) => {
      document.documentElement.style.background = e.payload;
      document.body.style.background = e.payload;
    });
    return () => {
      void unlisten.then((f) => f());
    };
  });
</script>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    height: 100%;
    background: #000;
    cursor: none;
  }
</style>
