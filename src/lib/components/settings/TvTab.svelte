<script lang="ts">
  /// Settings → TV: the cache casting keeps for files a television cannot play
  /// as they are.
  import { invoke } from '@tauri-apps/api/core';

  import { t } from '$lib/i18n.svelte';
  import { castCacheCapGb, setCastCacheCapGb } from '$lib/cast.svelte';
  import { showOsd } from '$lib/osd.svelte';
  import { fmtSize } from '$lib/units';

  interface Props {
    /// The cache's size on disk, as last read. The dialog's rather than this
    /// tab's, so a second visit shows the last answer while it asks again.
    castCacheBytes: number | null;
  }

  let { castCacheBytes = $bindable() }: Props = $props();

  // ---- The TV (casting) tab ----
  let castCapVal = $state(castCacheCapGb());
  const CAST_CAP_CHOICES = [0, 5, 20, 50];


  function setCastCapHere(gb: number) {
    castCapVal = gb;
    setCastCacheCapGb(gb);
  }

  async function refreshCastCacheSize() {
    castCacheBytes = await invoke<number>('cast_cache_size').catch(() => null);
  }

  // This tab is mounted only while it is showing, so mounting is the visit.
  $effect(() => {
    void refreshCastCacheSize();
  });

  async function clearCastCacheHere() {
    const freed = await invoke<number>('cast_clear_cache').catch(() => 0);
    showOsd(t('cast.cache_cleared', { size: fmtSize(freed) }));
    void refreshCastCacheSize();
  }
</script>

<div class="set-panel">
  <div class="setting">
    <div class="setting-label">{t('cast.set_cache')}</div>
    <div class="segmented">
      {#each CAST_CAP_CHOICES as cap (cap)}
        <button
          class="segopt"
          class:sel={castCapVal === cap}
          onclick={() => setCastCapHere(cap)}
        >
          {cap === 0 ? t('cast.cache_none') : t('cast.cap_gb', { n: cap })}
        </button>
      {/each}
    </div>
    <div class="setting-hint">{t('cast.cache_hint')}</div>
    {#if castCacheBytes !== null}
      <div class="setting-hint">{t('cast.cache_size', { size: fmtSize(castCacheBytes) })}</div>
    {/if}
    <button class="btn-danger" onclick={() => void clearCastCacheHere()}>
      {t('cast.cache_clear')}
    </button>
  </div>
</div>
