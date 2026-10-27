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
    WHEEL_STEPS,
    seekSteps,
    setSeekStep,
    setWheelMode,
    setWheelStep,
  } from '$lib/seek-steps.svelte';
  import { setExactThumbs, thumbs } from '$lib/thumbs.svelte';
</script>

<div class="set-group">{t('sec.seek')}</div>
<div class="set-panel">
  <!-- Pills rather than a slider: nobody wants a 7-second step, and five
       round numbers are quicker to hit than a thumb. The key names come from
       the binding table, because the keys can be moved. -->
  <div class="setting">
    <div class="setting-label">{t('set.seek_short')}</div>
    <div class="segmented">
      {#each SHORT_STEPS as v (v)}
        <button class="segopt" class:sel={seekSteps.short === v} onclick={() => setSeekStep('short', v)}>
          {t('set.seek_sec', { s: v })}
        </button>
      {/each}
    </div>
    <div class="setting-hint">
      {t('set.seek_short_hint', {
        keys: hintPair('seek_back', 'seek_fwd'),
        precise: hintPair('seek_back_precise', 'seek_fwd_precise'),
      })}
    </div>
  </div>

  <div class="setting">
    <div class="setting-label">{t('set.seek_long')}</div>
    <div class="segmented">
      {#each LONG_STEPS as v (v)}
        <button class="segopt" class:sel={seekSteps.long === v} onclick={() => setSeekStep('long', v)}>
          {t('set.seek_sec', { s: v })}
        </button>
      {/each}
    </div>
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
    <div class="segmented">
      {#each WHEEL_STEPS as v (v)}
        <button class="segopt" class:sel={seekSteps.wheelStep === v} onclick={() => setWheelStep(v)}>
          {t('set.seek_sec', { s: v })}
        </button>
      {/each}
    </div>
    <div class="setting-hint">{t('set.wheel_step_hint')}</div>
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
