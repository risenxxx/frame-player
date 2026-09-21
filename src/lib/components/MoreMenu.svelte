<script lang="ts">
  /// The control bar's right-hand tools, folded into one panel.
  ///
  /// It exists because that cluster grows with the *file*: a release with
  /// chapters, several dubs and subtitles carries seven buttons where a plain
  /// stream carries three. The row is a `1fr auto 1fr` grid and a `1fr` column
  /// cannot shrink below its content, so past a certain width the right cluster
  /// takes the space the centre column needs and play/prev/next visibly slide
  /// left of centre. `Controls` decides when that would happen; this is what it
  /// folds them into.
  ///
  /// Every row here is the same control as the button it replaces, so none of
  /// them is written twice: which tools exist is decided once in `Controls` and
  /// arrives as props, and picking a row calls the same `toggleMenu` the button
  /// would have called — the panel is *replaced* by the one it names rather
  /// than nesting it, which is the OSC's one-menu-at-a-time rule.
  import { t } from '$lib/i18n.svelte';
  import { hint } from '$lib/keys.svelte';
  import type { OscMenu } from '$lib/overlays.svelte';
  import { LOOP_LABEL, player } from '$lib/player.svelte';

  interface Props {
    /// Which tools this file offers. Passed rather than recomputed from
    /// `player`/`playlist`/`cast` here: the buttons in the bar ask the same
    /// questions, and two copies of that answer would drift the day one of
    /// them gains a condition.
    showQueue: boolean;
    showChapters: boolean;
    showAudio: boolean;
    showSubs: boolean;
    showCast: boolean;
    onPick: (kind: OscMenu) => void;
    onCycleLoop: () => void;
  }

  let { showQueue, showChapters, showAudio, showSubs, showCast, onPick, onCycleLoop }: Props =
    $props();
</script>

{#snippet tool(kind: OscMenu, label: string)}
  <!-- A row that leads somewhere, drawn like the context menu's submenu heads
       for the same reason: what follows is another panel, and a row that opens
       one has to look different from a row that acts. -->
  <button class="menu-item tool" onclick={() => onPick(kind)}>
    <span>{label}</span>
    <svg class="caret" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
      <path
        d="M6 3.5 10.5 8 6 12.5"
        fill="none"
        stroke="currentColor"
        stroke-width="1.6"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
    </svg>
  </button>
{/snippet}

<div class="menu moremenu">
  <div class="menu-title">{t('osc.more_title')}</div>
  <!-- The one row that acts in place, exactly as its button does: the label is
       the current mode, so the row answers "what is the repeat set to" as well
       as changing it, and the panel stays up to show the new answer. -->
  <button class="menu-item" onclick={onCycleLoop}>
    {t(LOOP_LABEL[player.loopMode])} <span class="hint">{hint('loop')}</span>
  </button>
  {#if showQueue || showChapters || showAudio || showSubs || showCast}
    <div class="menu-sep"></div>
  {/if}
  {#if showQueue}{@render tool('queue', t('osc.queue'))}{/if}
  {#if showChapters}{@render tool('chapter', t('osc.chapters'))}{/if}
  {#if showAudio}{@render tool('audio', t('osc.audio'))}{/if}
  {#if showSubs}{@render tool('sub', t('osc.subs'))}{/if}
  {#if showCast}{@render tool('cast', t('cast.tip'))}{/if}
</div>

<style>
  /* Narrower than the lists: these rows are labels, not release names, and the
     panel opens under a button that sits near the window's right edge. */
  .menu.moremenu {
    min-width: 208px;
  }

  /* The key beside the label, as in the context menu — its rule is scoped
     there, so the class is borrowed and the look has to be written again
     rather than inherited across the component boundary. */
  .menu-item .hint {
    float: right;
    color: #77777f;
    font-size: 12px;
  }

  .menu-item.tool {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .menu-item.tool .caret {
    flex: none;
    color: #77777f;
  }
</style>
