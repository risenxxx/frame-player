<script lang="ts">
  /// The glyph in front of a television row: what kind of thing it is, as far
  /// as the device says (`deviceKind`), and AirPlay's own mark for the one row
  /// that opens the system's list.
  import type { DeviceKind } from '$lib/cast.svelte';

  let { kind }: { kind: DeviceKind | 'airplay' } = $props();
</script>

<svg
  class="cast-icon"
  viewBox="0 0 24 24"
  width="18"
  height="18"
  fill="none"
  stroke="currentColor"
  stroke-width="1.6"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#if kind === 'tv'}
    <rect x="3" y="4.5" width="18" height="12" rx="1.8" />
    <path d="M8.5 20h7M12 16.5V20" />
  {:else if kind === 'box'}
    <rect x="3.5" y="8.5" width="17" height="7.5" rx="2" />
    <circle cx="16.6" cy="12.25" r="0.9" fill="currentColor" stroke="none" />
  {:else if kind === 'speaker'}
    <rect x="6.5" y="3" width="11" height="18" rx="2.6" />
    <circle cx="12" cy="14.4" r="3" />
    <circle cx="12" cy="7.6" r="1" fill="currentColor" stroke="none" />
  {:else}
    <path d="M7 17H5.6A2.6 2.6 0 0 1 3 14.4V6.6A2.6 2.6 0 0 1 5.6 4h12.8A2.6 2.6 0 0 1 21 6.6v7.8a2.6 2.6 0 0 1-2.6 2.6H17" />
    <path d="M12 14.6 16.4 20H7.6Z" fill="currentColor" />
  {/if}
</svg>

<style>
  .cast-icon {
    flex: none;
    /* The row's secondary strength, in colour rather than opacity: a thin
       glyph re-rasterises when it leaves full opacity (css.md). */
    color: rgba(232, 232, 236, 0.55);
  }
</style>
