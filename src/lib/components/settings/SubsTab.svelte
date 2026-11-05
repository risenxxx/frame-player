<script lang="ts">
  /// Settings → Subtitles: which track mpv picks, and how it draws it.
  import { t } from '$lib/i18n.svelte';
  import type { MpvSettings } from '$lib/settings/mpv-settings.svelte';
  import MpvRows from '$lib/components/settings/MpvRows.svelte';
  import { setSubLift, subShift } from '$lib/sub-shift.svelte';

  interface Props {
    mpv: MpvSettings;
  }

  let { mpv }: Props = $props();
</script>

<div class="set-group">{t('sec.sub_choice')}</div>
<div class="set-panel">
  <MpvRows {mpv} group="choice" />
</div>
<div class="set-group">{t('sec.sub_look')}</div>
<div class="set-panel">
  <MpvRows {mpv} group="look" />
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
