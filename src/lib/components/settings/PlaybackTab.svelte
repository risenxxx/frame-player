<script lang="ts">
  /// Settings → Playback: seek steps, the wheel, hover thumbnails, the queue.
  import { IS_MAC } from '$lib/platform';
  import { t } from '$lib/i18n.svelte';
  import { hintPair } from '$lib/keys.svelte';
  import { playlist, setPlaylistPref } from '$lib/playlist.svelte';
  import {
    LONG_STEPS,
    SHORT_STEPS,
    WHEEL_MODES,
    SPEED_MODS,
    STEP_MAX,
    STEP_MIN,
    WHEEL_STEPS,
    seekSteps,
    setSeekStep,
    setSpeedMod,
    setWheelMode,
    type StepKind,
  } from '$lib/seek-steps.svelte';
  import { setExactThumbs, thumbs } from '$lib/thumbs.svelte';

  function seekStepOf(kind: StepKind): number {
    return kind === 'short' ? seekSteps.short : kind === 'long' ? seekSteps.long : seekSteps.wheelStep;
  }
</script>

<!-- The pills are the common answers; the field beside them takes any other
     whole number of seconds, and shows the value when it is not a pill. It
     commits on Enter or on leaving it, so typing "45" does not pass through
     a 4-second step on the way. -->
{#snippet stepRow(kind: StepKind, choices: readonly number[], value: number)}
  <div class="step-row">
    <div class="segmented">
      {#each choices as v (v)}
        <button class="segopt" class:sel={value === v} onclick={() => setSeekStep(kind, v)}>
          {t('set.seek_sec', { s: v })}
        </button>
      {/each}
    </div>
    <label class="step-custom" class:sel={!choices.includes(value)}>
      <input
        type="number"
        min={STEP_MIN}
        max={STEP_MAX}
        step="1"
        placeholder={t('set.seek_other')}
        aria-label={t('set.seek_other')}
        value={choices.includes(value) ? '' : value}
        onchange={(e) => {
          const raw = e.currentTarget.value;
          if (raw !== '') setSeekStep(kind, Number(raw));
          e.currentTarget.value = choices.includes(seekStepOf(kind)) ? '' : String(seekStepOf(kind));
        }}
        onkeydown={(e) => {
          if (e.key === 'Enter') e.currentTarget.blur();
        }}
      />
      <span>{t('set.seek_unit')}</span>
    </label>
  </div>
{/snippet}

<div class="set-group">{t('sec.seek')}</div>
<div class="set-panel">
  <!-- Pills rather than a slider: nobody wants a 7-second step, and five
       round numbers are quicker to hit than a thumb. The key names come from
       the binding table, because the keys can be moved. -->
  <div class="setting">
    <div class="setting-label">{t('set.seek_short')}</div>
    {@render stepRow('short', SHORT_STEPS, seekSteps.short)}
    <div class="setting-hint">
      {t('set.seek_short_hint', {
        keys: hintPair('seek_back', 'seek_fwd'),
        precise: hintPair('seek_back_precise', 'seek_fwd_precise'),
      })}
    </div>
  </div>

  <div class="setting">
    <div class="setting-label">{t('set.seek_long')}</div>
    {@render stepRow('long', LONG_STEPS, seekSteps.long)}
    <div class="setting-hint">{t('set.seek_long_hint', { keys: hintPair('seek_back_10', 'seek_fwd_10') })}</div>
  </div>

  <div class="setting">
    <div class="setting-label">{t('set.wheel')}</div>
    <div class="segmented">
      {#each WHEEL_MODES as m (m)}
        <button class="segopt" class:sel={seekSteps.wheel === m} onclick={() => setWheelMode(m)}>
          {t(m === 'seek' ? 'set.wheel_seek' : 'set.wheel_volume')}
        </button>
      {/each}
    </div>
    <div class="setting-hint">{t(IS_MAC ? 'set.wheel_hint_mac' : 'set.wheel_hint_win')}</div>
  </div>

  <div class="setting">
    <div class="setting-label">{t('set.wheel_step')}</div>
    {@render stepRow('wheel', WHEEL_STEPS, seekSteps.wheelStep)}
    <div class="setting-hint">{t('set.wheel_step_hint')}</div>
  </div>

  <!-- Key names, not words, so they are not in the dictionaries; the "off"
       pill is. Written the way each platform prints them. -->
  <div class="setting">
    <div class="setting-label">{t('set.speed_wheel')}</div>
    <div class="segmented">
      {#each SPEED_MODS as m (m)}
        <button class="segopt" class:sel={seekSteps.speedMod === m} onclick={() => setSpeedMod(m)}>
          {m === 'off' ? t('set.speed_wheel_off') : m === 'alt' ? (IS_MAC ? '⌥' : 'Alt') : IS_MAC ? '⌃⇧' : 'Ctrl+Shift'}
        </button>
      {/each}
    </div>
    <div class="setting-hint">{t('set.speed_wheel_hint')}</div>
  </div>

  <div class="setting">
    <div class="row-toggle">
      <div class="row-text">
        <div class="setting-label">{t('set.thumb_exact')}</div>
        <div class="setting-hint">{t('set.thumb_exact_hint')}</div>
      </div>
      <button
        class="switch"
        class:on={thumbs.exact}
        role="switch"
        aria-checked={thumbs.exact}
        aria-label={t('set.thumb_exact')}
        onclick={() => setExactThumbs(!thumbs.exact)}
      >
        <span class="switch-knob"></span>
      </button>
    </div>
  </div>
</div>

<div class="set-group">{t('sec.queue')}</div>
<div class="set-panel">
  <div class="setting">
    <div class="row-toggle">
      <div class="row-text">
        <div class="setting-label">{t('set.queue')}</div>
        <div class="setting-hint">{t('set.queue_hint')}</div>
      </div>
      <button
        class="switch"
        class:on={playlist.queueFolder}
        role="switch"
        aria-checked={playlist.queueFolder}
        aria-label={t('set.queue')}
        onclick={() => setPlaylistPref('queueFolder', !playlist.queueFolder)}
      >
        <span class="switch-knob"></span>
      </button>
    </div>
  </div>

  <div class="setting">
    <div class="row-toggle">
      <div class="row-text">
        <div class="setting-label">{t('set.autoadvance')}</div>
        <div class="setting-hint">{t('set.autoadvance_hint')}</div>
      </div>
      <button
        class="switch"
        class:on={playlist.autoAdvance}
        role="switch"
        aria-checked={playlist.autoAdvance}
        aria-label={t('set.autoadvance')}
        onclick={() => setPlaylistPref('autoAdvance', !playlist.autoAdvance)}
      >
        <span class="switch-knob"></span>
      </button>
    </div>
  </div>
</div>

<style>
  .step-row {
    display: flex;
    flex-wrap: wrap;
    align-items: stretch;
    gap: 6px;
  }

  .step-row .segmented {
    flex: 1;
  }

  /* The pill row's own track, holding a field: indigo when the value in force
     is the one typed here, as a selected pill is. */
  .step-custom {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 0 10px;
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.07);
    color: #9a9aa5;
    font-size: 12px;
    cursor: text;
  }

  .step-custom.sel {
    background: #6366f1;
    color: #ffffff;
  }

  .step-custom input {
    width: 44px;
    border: none;
    background: transparent;
    color: inherit;
    font-size: 12px;
    text-align: right;
    -moz-appearance: textfield;
    appearance: textfield;
  }

  .step-custom input::-webkit-inner-spin-button,
  .step-custom input::-webkit-outer-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }

  .step-custom input:focus {
    outline: none;
  }

  .step-custom:focus-within {
    box-shadow: 0 0 0 1px #818cf8;
  }
</style>
