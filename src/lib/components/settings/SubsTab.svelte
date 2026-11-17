<script lang="ts">
  /// Settings → Subtitles: which track mpv picks, and how it draws it.
  import { invoke } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import { command } from 'tauri-plugin-libmpv-api';

  import { joinList, t } from '$lib/i18n.svelte';
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
  /// A file in the player's font folder and the families it declares — what
  /// `sub-font` can be set to (subtitle_fonts.rs). Empty for a file that was
  /// put there by hand and cannot be read as a font.
  interface ImportedFont {
    name: string;
    families: string[];
  }

  let fonts = $state<ImportedFont[]>([]);
  let fontBusy = $state(false);

  const fontDef = $derived(mpv.SETTINGS.find((s) => s.key === 'sub-font'));
  /// The family in force, compared as libass compares it — without case.
  const currentFamily = $derived((mpv.settingsValues['sub-font'] ?? '').trim().toLowerCase());

  function isCurrent(font: ImportedFont): boolean {
    return !!currentFamily && font.families.some((f) => f.toLowerCase() === currentFamily);
  }

  $effect(() => {
    void refreshFonts().then((ok) => {
      if (!ok) showOsd(t('osd.font_failed'));
    });
  });

  async function refreshFonts(): Promise<boolean> {
    try {
      fonts = await invoke<ImportedFont[]>('subtitle_fonts_list');
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
    // sublines.lua follows it into `osd-fonts-dir` and rebuilds its overlays.
    await command('set', ['sub-fonts-dir', '']);
    await command('set', ['sub-fonts-dir', directory]);
    return true;
  }

  /// What the backend's refusal means to the viewer; anything unnamed is a
  /// general failure, and its text goes to the console.
  function fontError(e: unknown): string {
    switch (String(e)) {
      case 'font_exists':
        return t('osd.font_exists');
      case 'font_invalid':
        return t('osd.font_invalid');
      case 'font_size':
        return t('osd.font_size');
      default:
        return t('osd.font_failed');
    }
  }

  /// Clicking the current font again goes back to mpv's default.
  function useFont(font: ImportedFont) {
    if (!fontDef || !font.families.length) return;
    mpv.setSetting(fontDef, isCurrent(font) ? null : font.families[0]);
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
      await invoke<ImportedFont>('subtitle_font_add', { path });
      const listed = await refreshFonts();
      const applied = await applyFontDirectory();
      if (!listed) showOsd(t('osd.font_failed'));
      else showOsd(t(applied ? 'osd.font_added' : 'osd.font_override'));
    } catch (e) {
      showOsd(fontError(e));
      console.warn('subtitle_font_add failed:', e);
    } finally {
      fontBusy = false;
    }
  }

  async function removeFont(font: ImportedFont) {
    if (fontBusy) return;
    fontBusy = true;
    try {
      await invoke('subtitle_font_remove', { name: font.name });
      // A family nothing provides any more would leave the field naming a
      // font libass silently replaces — so the choice goes back to default,
      // unless another file (another weight, say) still declares it.
      const wasCurrent = isCurrent(font);
      const listed = await refreshFonts();
      if (wasCurrent && fontDef && !fonts.some(isCurrent)) mpv.setSetting(fontDef, null);
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
<div class="set-panel">
  <MpvRows {mpv} group="fonts" />
  <!-- The same list as the excluded folders in General: a row per file, a
       cross to take it out, the dashed button to add one under the list. A
       row is also the way to pick its family — typing a name read off a file
       is what the field alone asked for. -->
  <div class="setting">
    <div class="setting-label">{t('vset.fonts')}</div>
    <div class="setting-hint">{t('vset.fonts_hint')}</div>
    <div class="fonts">
      {#each fonts as font (font.name)}
        {@const current = isCurrent(font)}
        <div class="font-row" class:current>
          <button
            class="font-pick"
            aria-pressed={current}
            disabled={!font.families.length}
            onclick={() => useFont(font)}
          >
            <svg class="font-check" viewBox="0 0 16 16" aria-hidden="true">
              {#if current}
                <path
                  d="M3.5 8.5l3 3 6-7"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.6"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                />
              {/if}
            </svg>
            <span class="font-text">
              <span class="font-family">{font.families[0] ?? font.name}</span>
              <span class="font-file" title={font.name}>
                {#if !font.families.length}
                  {t('vset.font_unreadable')}
                {:else if font.families.length > 1}
                  {t('vset.font_also', { families: joinList(font.families.slice(1)) })}
                {:else}
                  {font.name}
                {/if}
              </span>
            </span>
          </button>
          <button
            class="font-remove"
            data-tip={t('vset.font_remove')}
            aria-label={t('vset.font_remove')}
            disabled={fontBusy}
            onclick={() => void removeFont(font)}
          >
            <svg viewBox="0 0 10 10" aria-hidden="true">
              <path stroke="currentColor" stroke-width="1.4" stroke-linecap="round" d="M1.2 1.2l7.6 7.6M8.8 1.2l-7.6 7.6" />
            </svg>
          </button>
        </div>
      {:else}
        <div class="fonts-empty">{t('vset.fonts_empty')}</div>
      {/each}
      <button class="font-add" disabled={fontBusy} onclick={() => void addFont()}>
        <svg viewBox="0 0 16 16" aria-hidden="true">
          <path stroke="currentColor" stroke-width="1.5" stroke-linecap="round" d="M8 3.5v9M3.5 8h9" />
        </svg>
        {t('vset.font_add')}
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

  /* ---- Imported fonts: the excluded-folder list of General, with the row
     itself a button that picks the family. ---- */
  .fonts {
    margin-top: 10px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .font-row {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 3px 8px 3px 4px;
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.05);
  }

  .font-row.current {
    background: rgba(129, 140, 248, 0.12);
  }

  .font-pick {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 4px 6px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: inherit;
    text-align: left;
    cursor: pointer;
  }

  .font-pick:disabled {
    cursor: default;
  }

  .font-check {
    flex: none;
    width: 15px;
    height: 15px;
    color: #a5b4fc;
  }

  .font-text {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .font-family,
  .font-file {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .font-family {
    color: #d6d6de;
    font-size: 12.5px;
  }

  .font-row.current .font-family {
    color: #fff;
  }

  .font-file {
    color: #77777f;
    font-size: 11px;
  }

  .font-remove {
    flex: none;
    width: 22px;
    height: 22px;
    display: grid;
    place-items: center;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: #77777f;
    cursor: pointer;
    transition: background 0.12s ease, color 0.12s ease;
  }

  .font-remove svg {
    width: 9px;
    height: 9px;
    fill: none;
    display: block;
  }

  .font-row:hover .font-remove {
    color: #d6d6de;
  }

  .font-remove:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.1);
    color: #fff;
  }

  .fonts-empty {
    padding: 4px 2px;
    color: #6a6a74;
    font-size: 12px;
  }

  .font-add {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
    padding: 8px 10px;
    border: 1px dashed rgba(255, 255, 255, 0.16);
    border-radius: 8px;
    background: transparent;
    color: #b9b9c3;
    font-size: 12.5px;
    cursor: pointer;
    transition: border-color 0.12s ease, color 0.12s ease, background 0.12s ease;
  }

  .font-add svg {
    width: 14px;
    height: 14px;
    fill: none;
  }

  .font-add:hover:not(:disabled) {
    border-color: #818cf8;
    background: rgba(129, 140, 248, 0.08);
    color: #e8e8ec;
  }

  .font-add:disabled,
  .font-remove:disabled {
    cursor: default;
  }
</style>

