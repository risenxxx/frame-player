<script lang="ts">
  /// Settings → Subtitles: which track mpv picks, and how it draws it.
  import { t } from '$lib/i18n.svelte';
  import type { MpvSettings } from '$lib/settings/mpv-settings.svelte';
  import MpvRows from '$lib/components/settings/MpvRows.svelte';

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
</div>
