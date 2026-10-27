<script lang="ts">
  /// Settings → Video: the picture (mpv's HDR rows, the SDR colour, the global
  /// picture adjustments), decoding and online quality.
  import { IS_MAC } from '$lib/platform';
  import { t } from '$lib/i18n.svelte';
  import { frameStep, setFastBackStep } from '$lib/frame-step.svelte';
  import { applySdrColor, player } from '$lib/player.svelte';
  import {
    ADJUST_PARAMS,
    adjust,
    adjustLabel,
    bipolarFill,
    isZero,
    resetGlobalAdjust,
    setGlobalAdjust,
    signed,
    type AdjustParam,
  } from '$lib/picture-adjust.svelte';
  import { chrome } from '$lib/chrome.svelte';
  import { holdSlider } from '$lib/dom';
  import type { MpvSettings } from '$lib/settings/mpv-settings.svelte';
  import MpvRows from '$lib/components/settings/MpvRows.svelte';

  interface Props {
    mpv: MpvSettings;
    /// The decoder mpv actually picked, read by the dialog when it opens.
    hwdecCurrent: string | null;
    /// The picture slider being dragged. The dialog's, because it is what
    /// turns the whole sheet see-through (`peek` on Dialog).
    peeking: AdjustParam | null;
  }

  let { mpv, hwdecCurrent, peeking = $bindable() }: Props = $props();

  function startPeek(k: AdjustParam, e: PointerEvent) {
    if (e.button !== 0) return;
    peeking = k;
    chrome.tuning = true;
    holdSlider(e, () => (peeking = null));
  }
</script>

<div class="set-group">{t('sec.picture')}</div>
<div class="set-panel peek-keep" class:peeking={!!peeking}>
  <MpvRows {mpv} group="picture" />
  <!-- macOS only: the difference is between mpv and AVFoundation, and on
       Windows there is no system player whose curve anyone expects. A pill,
       not a switch — both positions are named ways of drawing, neither is
       "off". Kept by the player rather than in mpv.conf (see
       `applySdrColor`). -->
  {#if IS_MAC}
    <div class="setting">
      <div class="setting-label">{t('vset.sdr_color')}</div>
      <div class="segmented">
        <button
          class="segopt"
          class:sel={player.sdrColor === 'mpv'}
          onclick={() => applySdrColor('mpv')}
        >
          {t('vset.sdr_color_mpv')}
        </button>
        <button
          class="segopt"
          class:sel={player.sdrColor === 'system'}
          onclick={() => applySdrColor('system')}
        >
          {t('vset.sdr_color_system')}
        </button>
      </div>
      <div class="setting-hint">{t('vset.sdr_color_hint')}</div>
    </div>
  {/if}
  <!-- The global end of the picture adjustments; the file and playlist ends
       are in the context menu, next to the picture they are judged against.
       Dragging a slider here turns the sheet see-through for the same
       reason it does there. -->
  <div class="setting peek-keep" class:peeking={!!peeking}>
    <div class="setting-label">{t('vset.adjust')}</div>
    {#each ADJUST_PARAMS as k (k)}
      {@const v = adjust.global?.[k] ?? 0}
      <div class="slider-row adj-set-row" class:live={peeking === k}>
        <label class="adj-set-label" for="set-adj-{k}">{adjustLabel(k)}</label>
        <input
          id="set-adj-{k}"
          type="range"
          class="bipolar"
          min="-100"
          max="100"
          step="1"
          value={v}
          style={bipolarFill(v)}
          oninput={(e) => {
            chrome.tuning = true;
            setGlobalAdjust(k, +e.currentTarget.value);
          }}
          onpointerdown={(e) => startPeek(k, e)}
          ondblclick={() => setGlobalAdjust(k, 0)}
        />
        <span class="slider-value">{signed(v)}</span>
      </div>
    {/each}
    <div class="setting-hint">
      {#if player.hasFile && !adjust.followsGlobal}
        {t(adjust.scope === 'folder' ? 'vset.adjust_own_folder' : 'vset.adjust_own_file')}
      {:else}
        {t('vset.adjust_hint')}
      {/if}
    </div>
    <button class="btn-outline adj-reset" disabled={isZero(adjust.global)} onclick={resetGlobalAdjust}>
      {t('adj.reset')}
    </button>
  </div>
</div>

<div class="set-group">{t('sec.decode')}</div>
<div class="set-panel">
  <MpvRows {mpv} group="decode" />
  <!-- Kept by the player, not in mpv.conf: it is not an mpv option but a
       decision the frame stepper makes per file (frame-step.svelte.ts). -->
  <div class="setting">
    <div class="row-toggle">
      <div class="row-text">
        <div class="setting-label">{t('vset.fast_backstep')}</div>
        <div class="setting-hint">{t('vset.fast_backstep_hint')}</div>
      </div>
      <button
        class="switch"
        class:on={frameStep.enabled}
        role="switch"
        aria-checked={frameStep.enabled}
        aria-label={t('vset.fast_backstep')}
        onclick={() => setFastBackStep(!frameStep.enabled)}
      >
        <span class="switch-knob"></span>
      </button>
    </div>
  </div>
  {#if hwdecCurrent}
    <div class="settings-foot">
      {t('set.hwdec_foot', {
        value:
          hwdecCurrent === 'no' ? t('set.hwdec_sw') : t('set.hwdec_hw', { name: hwdecCurrent }),
      })}
    </div>
  {/if}
</div>

<div class="set-group">{t('sec.online')}</div>
<div class="set-panel">
  <MpvRows {mpv} group="online" />
</div>

<style>
  .adj-set-row {
    padding: 4px 8px;
    margin: 0 -8px;
    border-radius: 8px;
    transition: opacity 0.15s ease, background-color 0.15s ease;
  }

  .adj-set-row input {
    flex: 1;
  }

  .adj-set-label {
    flex: 0 0 104px;
    color: #b9b9c3;
    font-size: 12.5px;
  }

  /* A control on the sheet, not the action of a dialog: `.btn-outline`'s own
     9/20 padding and 15px type are sized for a footer, where there is one of
     them and it is what you came to press. Here it sat under four sliders
     labelled at 12.5px and outweighed the section it resets. The numbers are
     `.btn-danger`'s, which is the size every other button on this sheet
     already is. Two classes, so it beats the shared `button.btn-outline`
     wherever the two end up in the built stylesheet. */
  .btn-outline.adj-reset {
    margin-top: 10px;
    padding: 8px 14px;
    font-size: 12.5px;
  }

  /* While a slider is dragged, only its own row is left, on a backing of its
     own: the sheet around it has gone transparent (Dialog's `peek`). */
  .setting.peek-keep.peeking > :not(.adj-set-row),
  .setting.peek-keep.peeking .adj-set-row:not(.live) {
    opacity: 0;
  }

  .peek-keep.peeking .adj-set-row.live {
    background: rgba(16, 16, 22, 0.8);
  }

  /* `:global`, because the picture panel's own rows are drawn by `MpvRows`
     and carry its scope rather than this file's. */
  .peek-keep > :global(*) {
    transition: opacity 0.15s ease;
  }

  /* The panel the sliders sit in is the sheet's direct child, which is what
     Dialog's `peek` keeps or hides — so it is kept, and does the hiding of its
     other rows itself, its own backing going with the sheet's. */
  .set-panel.peek-keep.peeking {
    background: transparent;
    border-color: transparent;
  }

  /* `:global` for the same reason as `.peek-keep > *` above: the rows it hides
     include `MpvRows`' own. */
  .set-panel.peek-keep.peeking > :global(:not(.peek-keep)) {
    opacity: 0;
  }
</style>
