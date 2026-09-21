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
  /// One component rather than the same six lines in four panels, which is also
  /// what keeps the sticky arithmetic below in one place.
  import { t } from '$lib/i18n.svelte';
  import { overlays, toggleMenu } from '$lib/overlays.svelte';
</script>

{#if overlays.folded}
  <!-- The row and the rule under it are the context menu's drill-down back row
       (`submenuBack` in ContextMenu.svelte) verbatim — the one the mini player
       shows — and the wording is borrowed with it: a second key holding
       "Назад" would be a translation waiting to drift. Both panels have the
       same 6px of padding, so the same markup lands in the same place.
       The wrapper exists only to keep it in view (see below); it has no
       geometry of its own that the row could inherit. -->
  <div class="back-head">
    <button class="menu-item back" onclick={() => toggleMenu('more')}>
      <svg class="caret" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
        <path d="M10 3.5 5.5 8 10 12.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>
      </svg>
      <span>{t('ctx.back')}</span>
    </button>
    <div class="menu-sep"></div>
  </div>
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

  /* Sticky, because the chapter list scrolls itself to the chapter being
     played the moment it opens, and a row scrolled away with it would be gone
     before it was ever seen. The context menu never needed this — its
     submenus are short.

     This is the second attempt, and the first one is the reason for the
     shape. It made the ROW sticky and stretched it over the panel's 6px of
     padding with negative margins so rows could not slide past its edges —
     which put its hover highlight flush against the panel's border, changed
     its padding, and swapped the separator for a hairline sitting directly on
     the section title. Everything the row looks like belongs to the row, so
     the sticking is done by a wrapper that paints nothing but a mask:
     - `top: 6px` is where the panel's padding already puts it, so it does not
       jump when it starts sticking (the offset is measured from the panel's
       padding box, not its content);
     - the shadow is the panel's own colour spread 6px and lifted 6px, which
       covers the padding strips above and beside it — where passing rows
       would otherwise show — without painting over the title below;
     - the background is opaque where the panel's is 0.94, for the same
       reason; over anything but a bright frame the two are the same colour. */
  .back-head {
    position: sticky;
    top: 6px;
    z-index: 1;
    background: #101016;
    box-shadow: 0 -6px 0 6px #101016;
  }
</style>
