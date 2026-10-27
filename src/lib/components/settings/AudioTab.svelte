<script lang="ts">
  /// Settings → Audio: mpv's output rows and the player's own leveling.
  import { t } from '$lib/i18n.svelte';
  import { applyNormalize, player } from '$lib/player.svelte';
  import type { MpvSettings } from '$lib/settings/mpv-settings.svelte';
  import MpvRows from '$lib/components/settings/MpvRows.svelte';

  interface Props {
    mpv: MpvSettings;
  }

  let { mpv }: Props = $props();

  const bitstream = $derived(Boolean(mpv.settingsValues['audio-spdif']));
</script>

<div class="set-panel">
  <MpvRows {mpv} group="audio" />
  <!-- Bitstream passthrough hands the receiver an undecoded stream, and a
       filter has nothing to attach to — so the switch says why it is
       unavailable rather than silently doing nothing. Kept by the player
       rather than written to mpv.conf (see the module), which is why it
       sits below the footer's claim about mpv's own settings. -->
  <div class="setting" class:muted={bitstream}>
    <div class="row-toggle">
      <div class="row-text">
        <div class="setting-label">{t('set.normalize')}</div>
        <div class="setting-hint">
          {t(bitstream ? 'set.normalize_spdif' : 'set.normalize_hint')}
        </div>
      </div>
      <button
        class="switch"
        class:on={player.normalize && !bitstream}
        role="switch"
        aria-checked={player.normalize && !bitstream}
        disabled={bitstream}
        aria-label={t('set.normalize')}
        onclick={() => applyNormalize(!player.normalize)}
      >
        <span class="switch-knob"></span>
      </button>
    </div>
  </div>
</div>
