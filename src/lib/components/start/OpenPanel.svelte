<script lang="ts">
  /// The start screen's panel: the title, the hint and the ways in — a file, a
  /// link and, when it is switched on, the catalog.
  import { t } from '$lib/i18n.svelte';
  import { withKey } from '$lib/keys.svelte';

  import CatalogButton from './CatalogButton.svelte';

  interface Props {
    onOpenFile: () => void;
    onOpenLink: () => void;
    /// Null while the catalog is switched off — see StartScreen's props.
    onOpenCatalog: (() => void) | null;
  }

  let { onOpenFile, onOpenLink, onOpenCatalog }: Props = $props();
</script>

<div class="panel">
  <h1>Frame Player</h1>
  <p>{t('start.hint')}</p>
  <div class="start-actions">
    <button class="primary" onclick={onOpenFile}>{withKey(t('start.open'), 'open_file')}</button>
    <!-- Secondary on purpose: a local file is what the player is for, and
         a link is the other way in rather than an equal one. -->
    <button class="btn-outline" onclick={onOpenLink}>
      <!-- The chain drawn DIAGONALLY, which is not decoration: the flat
           horizontal version occupies 10 of the 24 units vertically, so
           at any box small enough to sit beside a 15px label it renders
           about 8px tall against the label's ~10.7px cap height and reads
           as undersized. The diagonal one nearly fills its box, so 14px
           of box gives ~11px of glyph — level with the words. -->
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
          <path d="M10 13.5a4.6 4.6 0 0 0 6.9.5l2.7-2.7a4.6 4.6 0 0 0-6.5-6.5L11.5 6.4"/>
          <path d="M14 10.5a4.6 4.6 0 0 0-6.9-.5l-2.7 2.7a4.6 4.6 0 0 0 6.5 6.5l1.6-1.6"/>
        </g>
      </svg>
      {t('start.link')}
    </button>
    <!-- The third way in, and deliberately the same weight as the link rather
         than a lesser one: all three answer "how do I get something playing",
         and only the first is the player's own reason to exist. Drawn only
         when the catalog is switched on, so the default install still has the
         two buttons it always had. -->
    {#if onOpenCatalog}
      <CatalogButton {onOpenCatalog} />
    {/if}
  </div>
</div>

<style>
  /* The ways in, side by side: the file picker is the primary action and the
     other two sit next to it as plain ones.
     Wrapping is not decoration — with the catalog on, three Russian labels are
     ~400px of buttons plus their gaps, against a window whose minimum width is
     480 and which the mini player takes well below the start screen's own
     comfortable size. Unwrapped, the third button leaves the panel. */
  .start-actions {
    display: flex;
    align-items: center;
    justify-content: center;
    flex-wrap: wrap;
    gap: 12px 16px;
  }

  .panel h1 {
    font-size: 42px;
    font-weight: 600;
    letter-spacing: 0.06em;
    margin: 0 0 8px;
  }
</style>
