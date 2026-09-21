<script lang="ts">
  /// The way back to the overflow panel, at the top of every OSC panel that can
  /// be reached from it.
  ///
  /// It appears only while the bar is folded (`overlays.folded`), because only
  /// then is there somewhere to go back *to*: with the buttons on screen, each
  /// panel was opened from its own button and "back" would mean nothing. That
  /// also answers what happens when the window is widened while a panel is up —
  /// the flag falls, the row disappears, and the button it came from is in the
  /// bar again.
  ///
  /// One component rather than the same six lines in four panels.
  import { t } from '$lib/i18n.svelte';
  import { overlays, toggleMenu } from '$lib/overlays.svelte';
</script>

{#if overlays.folded}
  <!-- The context menu's drill-down back row (`submenuBack` in
       ContextMenu.svelte) verbatim — the one the mini player shows — with its
       wording: a second key holding "Назад" would be a translation waiting to
       drift. Both panels carry 6px of padding, so the same markup lands in the
       same place, 6px from the top as from the sides.

       It scrolls with the list, exactly as the panel's own heading does. Two
       attempts to make it sticky are why: a sticky row inside a panel whose
       background is translucent needs a background of its own to hide the rows
       passing under it, and any background there lies ON TOP of the panel's —
       darker than the rest of the popup however it is tinted — while the mask
       that hides the padding strips around it reads as extra space above the
       row. Keeping it in view would take the panels splitting into a header and
       a scrolling body, not styling on this row. -->
  <button class="menu-item back" onclick={() => toggleMenu('more')}>
    <svg class="caret" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
      <path d="M10 3.5 5.5 8 10 12.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>
    </svg>
    <span>{t('ctx.back')}</span>
  </button>
  <div class="menu-sep"></div>
{/if}

<style>
  /* Copied from ContextMenu.svelte rather than shared: those rules are scoped
     there, and a scoped rule cannot reach this markup. Keep the two in step. */
  .menu-item.back {
    display: flex;
    align-items: center;
    gap: 8px;
    color: #a8a8b3;
  }

  .menu-item.back .caret {
    flex: none;
  }
</style>
