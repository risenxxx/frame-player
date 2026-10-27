<script lang="ts">
  /// The mpv-backed rows of one section, drawn from their `SETTINGS` entries
  /// in `MpvSettings` — the Video, Audio and Subtitles tabs each lay several
  /// of these out between the player's own settings.
  import { tick } from 'svelte';

  import ScrollFade from '$lib/components/ScrollFade.svelte';
  import { IS_MAC } from '$lib/platform';
  import { t } from '$lib/i18n.svelte';
  import { languageName, searchLanguages } from '$lib/languages';
  import type { MpvSettings, SettingDef } from '$lib/settings/mpv-settings.svelte';

  interface Props {
    mpv: MpvSettings;
    group: SettingDef['group'];
  }

  let { mpv, group }: Props = $props();

  let langQueryEl = $state<HTMLInputElement | undefined>();

  async function openLangPicker(def: SettingDef) {
    mpv.langPickerFor = mpv.langPickerFor === def.key ? null : def.key;
    mpv.langQuery = '';
    if (!mpv.langPickerFor) return;
    await tick();
    langQueryEl?.focus();
  }
</script>

<!-- One mpv-backed setting, drawn from its `SETTINGS` entry. A snippet rather
     than a loop body because the tabs are now laid out in sections, and the
     player's own settings (SDR colour, the adjustments, leveling) sit between
     mpv's inside them. -->
{#each mpv.SETTINGS.filter((s) => s.group === group) as s (s.key)}
  {@render mpvSetting(s)}
{/each}

{#snippet mpvSetting(s: SettingDef)}
  <div class="setting">
    <div class="setting-label">{s.label}</div>
    {#if s.kind === 'devices'}
      <div class="segmented vertical">
        {#each mpv.audioDevices as device (device.name)}
          <!-- mpv's own list starts with `auto`, and that is also its
               default — mapped to null so choosing it CLEARS the
               mpv.conf line instead of pinning "auto" in writing, the
               same meaning "по умолчанию" has everywhere else here. -->
          {@const value = device.name === 'auto' ? null : device.name}
          <button
            class="segopt"
            class:sel={(mpv.settingsValues[s.key] ?? null) === value}
            onclick={() => mpv.setSetting(s, value)}
          >
            {device.description}
          </button>
        {/each}
      </div>
    {:else if s.kind === 'langs'}
      {@const codes = mpv.langsOf(s)}
      <!-- Chips in priority order, then the way to add one. The empty
           state says "авто" as a word rather than showing an empty
           strip, which reads as a control that failed to render. -->
      <div class="langs">
        {#if !codes.length}
          <span class="langs-auto">{t('vset.lang_auto')}</span>
        {/if}
        {#each codes as code, i (code)}
          <!-- Two buttons in a chip, never nested: a button inside a
               button is invalid and the inner one stops being clicked.
               The first chip is the preferred language and says so by
               being filled, which is also why clicking it does nothing
               — it is already what it would become. -->
          <span class="lang-chip" class:first={i === 0}>
            <button
              class="lang-name"
              data-tip={i === 0 ? t('vset.lang_primary') : t('vset.lang_promote')}
              disabled={i === 0}
              onclick={() => mpv.promoteLang(s, code)}
            >{languageName(code)}</button>
            <button
              class="lang-drop"
              aria-label={t('vset.lang_remove')}
              data-tip={t('vset.lang_remove')}
              onclick={() => mpv.removeLang(s, code)}
            >
              <svg viewBox="0 0 10 10" aria-hidden="true">
                <path stroke="currentColor" stroke-width="1.4" stroke-linecap="round" d="M1.2 1.2l7.6 7.6M8.8 1.2l-7.6 7.6" />
              </svg>
            </button>
          </span>
        {/each}
        <button
          class="lang-add"
          class:open={mpv.langPickerFor === s.key}
          data-tip={t('vset.lang_add')}
          aria-label={t('vset.lang_add')}
          onclick={() => void openLangPicker(s)}
        >
          <svg viewBox="0 0 16 16" aria-hidden="true">
            <path d="M8 3.5v9M3.5 8h9" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
          </svg>
        </button>
      </div>
      {#if mpv.langPickerFor === s.key}
        <!-- Sixty languages behind a search field, which is the whole
             shape of this control: the common case is two clicks and the
             long tail is one word of typing away, without either being
             in the other's way. -->
        <div class="lang-picker">
          <input
            class="lang-search"
            type="text"
            spellcheck="false"
            autocapitalize="off"
            autocorrect="off"
            placeholder={t('vset.lang_search')}
            bind:this={langQueryEl}
            bind:value={mpv.langQuery}
            onkeydown={(e) => {
              // Enter takes the first hit — with a filtered list of one,
              // reaching for the mouse is the whole cost of the feature.
              if (e.key === 'Enter') {
                e.preventDefault();
                const hit = searchLanguages(mpv.langQuery, codes)[0];
                if (hit) mpv.addLang(s, hit.code);
              }
              if (e.key === 'Escape') { e.preventDefault(); mpv.langPickerFor = null; }
              e.stopPropagation();
            }}
          />
          <div class="lang-list">
            {#each searchLanguages(mpv.langQuery, codes) as lang (lang.code)}
              <button class="lang-option" onclick={() => mpv.addLang(s, lang.code)}>
                <span class="lang-option-name">{lang.name}</span>
                <span class="lang-option-code">{lang.code}</span>
              </button>
            {:else}
              <div class="folders-empty">{t('vset.lang_none')}</div>
            {/each}
            <ScrollFade />
          </div>
        </div>
      {/if}
    {:else if s.kind === 'slider'}
      {@const value = mpv.sliderValue(s)}
      <div class="slider-row">
        <input
          type="range"
          class="setting-slider"
          min={s.min}
          max={s.max}
          step={s.step}
          {value}
          style="--progress: {((value - s.min) / (s.max - s.min)) * 100}%"
          aria-label={s.label}
          oninput={(e) => mpv.setSlider(s, Number(e.currentTarget.value))}
          onchange={(e) => e.currentTarget.blur()}
        />
        <span class="slider-value">{value.toFixed(s.decimals)}{s.suffix ?? ''}</span>
      </div>
    {:else}
      <div class="segmented">
        {#each s.options as o (o.label)}
          <button
            class="segopt"
            class:sel={mpv.settingsValues[s.key] === o.v}
            disabled={s.key === 'target-colorspace-hint' &&
              o.v !== null &&
              mpv.displayHdr !== null &&
              !mpv.displayHdr.supported}
            onclick={() => mpv.setSetting(s, o.v)}
          >
            {o.label}
          </button>
        {/each}
      </div>
    {/if}
    {#if s.key === 'target-colorspace-hint' && mpv.displayHdr}
      <div class="setting-hint">
        {#if !mpv.displayHdr.supported}
          {t('set.hdr_unsupported')}
        {:else if !mpv.displayHdr.enabled}
          {t(IS_MAC ? 'set.hdr_off_mac' : 'set.hdr_off_win')}
        {:else if mpv.settingsValues[s.key] === null}
          {t(IS_MAC ? 'set.hdr_forced_mac' : 'set.hdr_forced_win')}
        {:else}
          {t(IS_MAC ? 'set.hdr_on_mac' : 'set.hdr_on_win')}
        {/if}
      </div>
    {:else if s.hint}<div class="setting-hint">{s.hint}</div>{/if}
  </div>
{/snippet}

<style>
  .langs {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin-top: 8px;
  }

  .langs-auto {
    padding: 4px 2px;
    color: #6a6a74;
    font-size: 12.5px;
  }

  /* The chip is the container and both controls sit inside it, because a button
     inside a button is invalid markup and the inner one stops receiving clicks.
     Its own background carries the state; neither child paints one. */
  .lang-chip {
    display: inline-flex;
    align-items: center;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.1);
    transition: background 0.12s ease;
  }

  /* Indigo means one thing here as everywhere else — this is the selected one,
     the language mpv reaches for first. */
  .lang-chip.first {
    background: #6366f1;
  }

  .lang-name {
    padding: 5px 4px 5px 11px;
    border: none;
    background: transparent;
    color: #e0e0e6;
    font-size: 12.5px;
    cursor: pointer;
  }

  .lang-chip.first .lang-name {
    color: #fff;
  }

  /* The first chip is already what clicking it would make it, so it is inert —
     and says so by not changing under the pointer rather than by being dimmed,
     which would read as unavailable. */
  .lang-name:disabled {
    cursor: default;
  }

  .lang-drop {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    margin-right: 4px;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: transparent;
    /* Color, never opacity: this is a 1.4px stroke that is always on screen,
       and taking it off full opacity makes WKWebView re-rasterise and wobble it
       (the same finding as the torrent list's delete cross). */
    color: rgba(255, 255, 255, 0.4);
    cursor: pointer;
    transition: color 0.12s ease, background 0.12s ease;
  }

  .lang-chip.first .lang-drop {
    color: rgba(255, 255, 255, 0.62);
  }

  /* Written to win, not left to source order — measured in the built bundle,
     `.lang-chip.first .lang-drop` weighs (0,4,0) against a bare
     `.lang-drop:hover`'s (0,3,0), so on the preferred chip the cross would have
     stayed at its resting color under the pointer. The same arithmetic the
     link-history cross ran into. */
  .lang-chip .lang-drop:hover {
    background: rgba(0, 0, 0, 0.25);
    color: #fff;
  }

  .lang-drop svg {
    width: 9px;
    height: 9px;
  }

  /* 26 outer, border included — and it always was, because a <button> is
     border-box in both engines' own stylesheets, reset or no reset. */
  .lang-add {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    padding: 0;
    border: 1px dashed rgba(255, 255, 255, 0.18);
    border-radius: 50%;
    background: transparent;
    color: #b9b9c3;
    cursor: pointer;
    transition: border-color 0.12s ease, color 0.12s ease, background 0.12s ease;
  }

  .lang-add svg {
    width: 13px;
    height: 13px;
  }

  .lang-picker {
    margin-top: 8px;
    border: 1px solid rgba(255, 255, 255, 0.09);
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.03);
    overflow: hidden;
  }

  .lang-search {
    display: block;
    width: 100%;
    padding: 8px 10px;
    border: none;
    border-bottom: 1px solid rgba(255, 255, 255, 0.07);
    background: transparent;
    color: #e8e8ec;
    font-size: 12.5px;
  }

  .lang-search:focus {
    outline: none;
  }

  .lang-list {
    max-height: 176px;
    overflow-y: auto;
    /* Part of scrolling vertically rather than an extra: setting one axis
       computes the other from `visible` to `auto`. */
    overflow-x: hidden;
  }

  /* `width: 100%` is the row filling the list rather than overhanging it by its
     own 20px of padding, and that held before the global reset as well: this is
     a <button>, and both engines' stylesheets already make one border-box. The
     reset changes nothing here — worth knowing before "fixing" a row that was
     never broken. */
  .lang-option {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 10px;
    width: 100%;
    padding: 6px 10px;
    border: none;
    background: transparent;
    color: #d0d0d8;
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
  }

  .lang-option:hover {
    background: rgba(255, 255, 255, 0.08);
    color: #fff;
  }

  .lang-option-name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* The code is here for the person who knows their file is tagged `jpn` and
     cannot read 日本語 — the same reason the search matches on codes. */
  .lang-option-code {
    flex: none;
    color: #6a6a74;
    font-size: 11px;
  }

  .lang-add:hover,
  .lang-add.open {
    border-color: #818cf8;
    background: rgba(129, 140, 248, 0.1);
    color: #e8e8ec;
  }

  .setting-slider {
    flex: 1;
    min-width: 0;
  }
</style>
