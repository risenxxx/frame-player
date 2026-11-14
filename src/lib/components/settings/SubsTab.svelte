<script lang="ts">
  /// Settings → Subtitles: which track mpv picks, and how it draws it.
  import { t } from '$lib/i18n.svelte';
  import type { MpvSettings } from '$lib/settings/mpv-settings.svelte';
  import MpvRows from '$lib/components/settings/MpvRows.svelte';
  import { setSubLift, subShift } from '$lib/sub-shift.svelte';
  import { chrome } from '$lib/chrome.svelte';
  import { holdSlider } from '$lib/dom';

  interface Props {
    mpv: MpvSettings;
    /// The subtitle slider being dragged — see VideoTab's.
    peeking: string | null;
  }

  let { mpv, peeking = $bindable() }: Props = $props();

  /// The sheet goes see-through while a look slider is dragged, as it does for
  /// the picture's: under the dimmed backdrop a subtitle's size, place or
  /// opacity cannot be judged at all. The bars go too (`tuning`), which also
  /// takes the subtitles' lift off, so what is seen is where they really sit.
  function startPeek(key: string, e: PointerEvent) {
    if (e.button !== 0) return;
    peeking = key;
    chrome.tuning = true;
    holdSlider(e, () => (peeking = null));
  }
</script>

<div class="set-group">{t('sec.sub_choice')}</div>
<div class="set-panel">
  <MpvRows {mpv} group="choice" />
</div>
<div class="set-group">{t('sec.sub_look')}</div>
<div class="set-panel peek-keep" class:peeking={!!peeking}>
  <MpvRows {mpv} group="look" onpeek={startPeek} {peeking} />
  <!-- Not a line of mpv.conf: the lift is the player's own, so the switch
       sits after mpv's rows rather than among them. -->
  <div class="setting">
    <div class="row-toggle">
      <div class="row-text">
        <div class="setting-label">{t('set.sub_lift')}</div>
        <div class="setting-hint">{t('set.sub_lift_hint')}</div>
      </div>
      <button
        class="switch"
        class:on={subShift.enabled}
        role="switch"
        aria-checked={subShift.enabled}
        aria-label={t('set.sub_lift')}
        onclick={() => setSubLift(!subShift.enabled)}
      >
        <span class="switch-knob"></span>
      </button>
    </div>
  </div>
</div>

<style>
  /* While a slider is dragged only its own row is left, on a backing of its
     own: the sheet around it has gone transparent (Dialog's `peek`) and this
     panel, its direct child, is kept and hides its other rows itself. The
     rows are `MpvRows`', hence `:global`. */
  .set-panel.peek-keep.peeking {
    background: transparent;
    border-color: transparent;
  }

  .peek-keep > :global(*) {
    transition: opacity 0.15s ease;
  }

  .set-panel.peek-keep.peeking > :global(:not(.live)) {
    opacity: 0;
  }

  .set-panel.peek-keep.peeking > :global(.live) {
    background: rgba(16, 16, 22, 0.8);
    border-radius: 8px;
  }
</style>

