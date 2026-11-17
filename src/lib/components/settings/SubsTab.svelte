<script lang="ts">
  /// Settings → Subtitles: which track mpv picks, and how it draws it.
  import { invoke } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import { command } from 'tauri-plugin-libmpv-api';

  import { t } from '$lib/i18n.svelte';
  import type { MpvSettings } from '$lib/settings/mpv-settings.svelte';
  import MpvRows from '$lib/components/settings/MpvRows.svelte';
  import { setSubLift, subShift } from '$lib/sub-shift.svelte';
  import { chrome, withFileDialog } from '$lib/chrome.svelte';
  import { holdSlider } from '$lib/dom';
  import { showOsd } from '$lib/osd.svelte';
  import { isUserConfKey, player } from '$lib/player.svelte';

  interface Props {
    mpv: MpvSettings;
    /// The subtitle slider being dragged — see VideoTab's.
    peeking: string | null;
  }

  let { mpv, peeking = $bindable() }: Props = $props();
  let fonts = $state<{ name: string }[]>([]);
  let fontBusy = $state(false);

  $effect(() => {
    void refreshFonts().then((ok) => {
      if (!ok) showOsd(t('osd.font_failed'));
    });
  });

  async function refreshFonts(): Promise<boolean> {
    try {
      fonts = await invoke<{ name: string }[]>('subtitle_fonts_list');
      return true;
    } catch (e) {
      console.warn('subtitle_fonts_list failed:', e);
      return false;
    }
  }

  async function applyFontDirectory(): Promise<boolean> {
    if (!player.ready) return true;
    if (isUserConfKey('sub-fonts-dir')) return false;
    const directory = await invoke<string>('subtitle_fonts_dir');
    // mpv compares option values before notifying its subtitle decoder. Set a
    // different value first so reloading the same directory sees file changes.
    await command('set', ['sub-fonts-dir', '']);
    await command('set', ['sub-fonts-dir', directory]);
    return true;
  }

  async function addFont() {
    if (fontBusy) return;
    fontBusy = true;
    try {
      const path = await withFileDialog(() =>
        open({
          multiple: false,
          filters: [{ name: t('vset.font_files'), extensions: ['otc', 'otf', 'ttc', 'ttf'] }],
        }),
      );
      if (!path) return;
      await invoke<string>('subtitle_font_add', { path });
      const listed = await refreshFonts();
      const applied = await applyFontDirectory();
      if (!listed) showOsd(t('osd.font_failed'));
      else showOsd(t(applied ? 'osd.font_added' : 'osd.font_override'));
    } catch (e) {
      showOsd(t('osd.font_failed'));
      console.warn('subtitle_font_add failed:', e);
    } finally {
      fontBusy = false;
    }
  }

  async function removeFont(name: string) {
    if (fontBusy) return;
    fontBusy = true;
    try {
      await invoke('subtitle_font_remove', { name });
      const listed = await refreshFonts();
      const applied = await applyFontDirectory();
      if (!listed) showOsd(t('osd.font_failed'));
      else showOsd(t(applied ? 'osd.font_removed' : 'osd.font_override'));
    } catch (e) {
      showOsd(t('osd.font_failed'));
      console.warn('subtitle_font_remove failed:', e);
    } finally {
      fontBusy = false;
    }
  }

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
<div class="set-group">{t('sec.sub_fonts')}</div>
<div class="set-panel fonts-panel">
  <div class="setting">
    <div class="setting-label">{t('vset.fonts')}</div>
    <div class="setting-hint">{t('vset.fonts_hint')}</div>
    <button class="btn-outline font-add" disabled={fontBusy} onclick={() => void addFont()}>
      {t('vset.font_add')}
    </button>
    {#if fonts.length}
      <div class="font-list">
        {#each fonts as font (font.name)}
          <div class="font-row">
            <span class="font-name" title={font.name}>{font.name}</span>
            <button
              class="font-remove"
              disabled={fontBusy}
              aria-label={t('vset.font_remove')}
              onclick={() => void removeFont(font.name)}
            >{t('vset.font_remove')}</button>
          </div>
        {/each}
      </div>
    {:else}
      <div class="font-empty">{t('vset.fonts_empty')}</div>
    {/if}
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

  /* The backing gets room inside it, as the context menu's `.adj-row` has, or
     the label, the value and the hint sit against its edges. The negative
     margin takes the padding back out, so the row's content does not move
     when the drag starts; the bottom one is the row's own 14px less the 8px
     padding, which only the hidden rows below would feel anyway. */
  .set-panel.peek-keep.peeking > :global(.live) {
    padding: 8px 10px;
    margin: -8px -10px 6px;
    background: rgba(16, 16, 22, 0.8);
    border-radius: 8px;
  }

  .font-add {
    margin-top: 10px;
    padding: 6px 10px;
    font-size: 12.5px;
  }

  .font-list {
    display: grid;
    gap: 6px;
    margin-top: 10px;
  }

  .font-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    min-width: 0;
  }

  .font-name {
    overflow: hidden;
    color: #c4c4ce;
    font-size: 12.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .font-remove {
    flex: none;
    padding: 4px 8px;
    border: 0;
    border-radius: 5px;
    background: transparent;
    color: #a7a7b2;
    font-size: 11.5px;
    cursor: pointer;
  }

  .font-remove:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.08);
    color: #fff;
  }

  .font-remove:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .font-empty {
    margin-top: 10px;
    color: #7d7d88;
    font-size: 12px;
  }
</style>

