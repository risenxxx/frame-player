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
  <!-- Wording borrowed from the context menu's own drill-down: it is the same
       word for the same gesture, and a second key holding "Назад" would be a
       translation waiting to drift. -->
  <button class="menu-item back" onclick={() => toggleMenu('more')}>
    <svg class="caret" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
      <path
        d="M10 3.5 5.5 8 10 12.5"
        fill="none"
        stroke="currentColor"
        stroke-width="1.6"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
    </svg>
    <span>{t('ctx.back')}</span>
  </button>
{/if}

<style>
  /* Sticky, and that is not a flourish: the chapter list scrolls itself to the
     chapter being played the moment it opens, so a row that scrolls with the
     content would be gone before it was ever seen.

     The geometry is what it takes for content not to show past it. `.menu` is
     the scroll container, so the sticky offset is measured from its PADDING
     box — the row therefore has to cover that 6px of padding itself, or rows
     would slide through the strip above and beside it. It takes the padding
     back as negative margin and gives the same amount back as padding, so the
     label does not move between the stuck and unstuck states. The square top
     corners need no rounding: a scroll container clips to its own padding box,
     which is already round. */
  .menu-item.back {
    position: sticky;
    top: 0;
    z-index: 1;
    display: flex;
    align-items: center;
    gap: 8px;
    margin: -6px -6px 0;
    padding: 14px 16px 8px;
    color: #a8a8b3;
    /* Opaque, unlike the panel's own 0.94: a translucent header shows the rows
       travelling underneath it. Over anything but a bright frame the two read
       as the same colour. */
    background: #101016;
    border-bottom: 1px solid rgba(255, 255, 255, 0.09);
  }

  /* `.menu-item:hover` is a translucent white wash, which would be see-through
     here for the same reason — so the hover is mixed down to a flat colour. */
  .menu-item.back:hover {
    background: #1e1e26;
  }

  .menu-item.back .caret {
    flex: none;
  }
</style>
