<script lang="ts">
  /// The start screen: how to open something, what was being watched, and which
  /// torrents are on disk.
  ///
  /// This file is the frame those three parts sit in (`start/OpenPanel`,
  /// `start/RecentRail`, `start/TorrentList`) and the torrent list's delete
  /// menu, which is drawn outside the scrolling overlay and therefore belongs
  /// to the frame rather than to the list. Everything that *acts* arrives as a
  /// prop instead: opening a file, a link or a torrent all reach into the
  /// player, and this component deliberately knows nothing about it.
  import { tick } from 'svelte';

  import { flipAxis, shiftAxis } from '$lib/floating';

  import type { RecentItem } from '$lib/history.svelte';
  import { t } from '$lib/i18n.svelte';
  import {
    watchedFiles,
    type RememberedTorrent,
    type TorrentRow,
  } from '$lib/torrent.svelte';
  import type { FeedItem } from '$lib/feed';

  import OpenPanel from './start/OpenPanel.svelte';
  import RecentRail from './start/RecentRail.svelte';
  import TorrentList from './start/TorrentList.svelte';

  interface Props {
    torrentRows: TorrentRow[];
    torrentTotal: number;
    torrentBusy: string | null;
    torrentOpening: string | null;
    /// Newer uploads found in the feeds torrents came from, by info hash.
    feedUpdates: Record<string, FeedItem>;
    /// The torrent whose feed is being read for the update button.
    feedChecking: string | null;
    torrentResume: (row: TorrentRow) => { name: string; pos: number; dur: number; index: number } | null;
    onOpenFile: () => void;
    onOpenLink: () => void;
    /// Null while the catalog is switched off, which is the default — the
    /// button is then not drawn at all rather than drawn and disabled. A
    /// disabled control invites a click and then explains; an absent one is
    /// honest about a feature nobody has turned on.
    onOpenCatalog: (() => void) | null;
    onOpenRecent: (item: RecentItem) => void;
    onForgetRecent: (item: RecentItem) => void;
    onOpenTorrent: (row: TorrentRow) => void;
    onUpdateTorrent: (known: RememberedTorrent) => void;
    onDeleteTorrent: (row: TorrentRow) => void;
    onDeleteWatched: (row: TorrentRow) => void;
  }

  let {
    torrentRows,
    torrentTotal,
    torrentBusy,
    torrentOpening,
    feedUpdates,
    feedChecking,
    torrentResume,
    onOpenFile,
    onOpenLink,
    onOpenCatalog,
    onOpenRecent,
    onForgetRecent,
    onOpenTorrent,
    onUpdateTorrent,
    onDeleteTorrent,
    onDeleteWatched,
  }: Props = $props();

  /**
   * The delete menu: which row asked, where it goes, and what it may offer.
   *
   * A floating panel rather than the row expanding underneath itself. The
   * expanding version pushed every row below it down at the moment a
   * destructive question was asked — the list moved under the cursor that was
   * about to answer — and it made a row two heights, which a list of rows
   * should not be.
   *
   * `placed` is the two-pass placement every floating box here needs: the size
   * cannot be measured until it is in the DOM, so it is rendered hidden, laid
   * out, and shown. `visibility` rather than a conditional block, or there is
   * nothing to measure.
   */
  let menu = $state<{
    row: TorrentRow;
    folder: string;
    seen: number;
    x: number;
    y: number;
    placed: boolean;
  } | null>(null);
  let menuEl = $state<HTMLDivElement | undefined>();
  /// The row whose cross is lit — the menu's own folder, or nothing.
  const deleteFor = $derived(menu?.folder ?? null);

  async function openDeleteMenu(row: TorrentRow, anchor: HTMLElement) {
    if (menu?.folder === row.folder) {
      closeDeleteMenu();
      return;
    }
    menu = {
      row,
      folder: row.folder,
      seen: row.info_hash ? watchedFiles(row.info_hash).size : 0,
      x: 0,
      y: 0,
      placed: false,
    };
    await tick();
    if (!menuEl || !menu) return;
    const a = anchor.getBoundingClientRect();
    const box = menuEl.getBoundingClientRect();
    // Sideways out of the row, like a submenu: left of the cross by preference,
    // since the cross sits at the right edge of a panel that is itself centred
    // and the window margin beside it is usually the narrower side.
    const h = flipAxis({
      near: a.left,
      far: a.right,
      size: box.width,
      limit: window.innerWidth,
      gap: 6,
      preferBefore: true,
    });
    menu = {
      ...menu,
      x: h.pos,
      // Hung from the cross's top edge and slid up only as far as it must, so
      // the panel reads as coming out of the row it belongs to.
      y: shiftAxis(a.top - 6, box.height, window.innerHeight),
      placed: true,
    };
  }

  function closeDeleteMenu() {
    menu = null;
  }

  /**
   * Everything that invalidates a placement in viewport coordinates.
   *
   * A fixed panel does not travel with the content underneath it, so a scroll
   * would leave it pointing at a row that has moved. Closing is the honest
   * answer — the alternative is re-placing on every scroll frame for a panel
   * that is open for a second.
   *
   * The outside click is captured, so it closes the menu before it reaches
   * whatever is under it; the cross's own handler still toggles, because the
   * capture listener leaves clicks inside the menu and on the anchor alone.
   */
  $effect(() => {
    if (!menu) return;
    const onDown = (e: PointerEvent) => {
      const target = e.target as Element | null;
      if (target?.closest('.rowmenu') || target?.closest('.torrow-forget')) return;
      closeDeleteMenu();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        closeDeleteMenu();
      }
    };
    window.addEventListener('pointerdown', onDown, true);
    window.addEventListener('keydown', onKey, true);
    window.addEventListener('resize', closeDeleteMenu);
    // The list scrolls inside `.overlay`, and the window itself does not, so
    // this has to listen in the capture phase: a scroll event does not bubble.
    window.addEventListener('scroll', closeDeleteMenu, true);
    return () => {
      window.removeEventListener('pointerdown', onDown, true);
      window.removeEventListener('keydown', onKey, true);
      window.removeEventListener('resize', closeDeleteMenu);
      window.removeEventListener('scroll', closeDeleteMenu, true);
    };
  });
</script>

<div class="overlay start scrollable">
  <!-- The padding lives on the inner wrapper, not on the scroll container:
       browsers do not include a scrollable block's padding-bottom in the
       scrollable area, so at the height where the content is flush but no
       scrollbar has appeared yet, the bottom gap vanished and the card text
       ran into the window edge. -->
  <div class="start-inner">
  <OpenPanel {onOpenFile} {onOpenLink} {onOpenCatalog} />
  <RecentRail {onOpenRecent} {onForgetRecent} />

  <TorrentList
    {torrentRows}
    {torrentTotal}
    {torrentBusy}
    {torrentOpening}
    {feedUpdates}
    {feedChecking}
    {torrentResume}
    {onOpenTorrent}
    {onUpdateTorrent}
    {deleteFor}
    {openDeleteMenu}
  />
  </div>
</div>

<!-- **Outside `.overlay`, which scrolls.** `position: fixed` inside a scroll
     container measures and hit-tests correctly in both engines and is still
     clipped on screen — the finding that moved the context menu's submenus out
     of their parent. So the panel is a sibling of the list, placed in viewport
     coordinates, and closed when anything moves underneath it. -->
{#if menu}
  <div
    class="rowmenu"
    bind:this={menuEl}
    style="left: {menu.x}px; top: {menu.y}px; visibility: {menu.placed
      ? 'visible'
      : 'hidden'}"
    role="menu"
    tabindex="-1"
  >
    <div class="rowmenu-head">{t('start.torrent_delete_what')}</div>
    {#if menu.seen > 0}
      <button
        class="menu-item"
        role="menuitem"
        onclick={() => {
          const row = menu?.row;
          closeDeleteMenu();
          if (row) onDeleteWatched(row);
        }}
      >
        {t('start.torrent_delete_watched', { count: menu.seen })}
      </button>
    {/if}
    <button
      class="menu-item danger"
      role="menuitem"
      onclick={() => {
        const row = menu?.row;
        closeDeleteMenu();
        if (row) onDeleteTorrent(row);
      }}
    >
      {t('start.torrent_delete_all')}
    </button>
  </div>
{/if}

<style>
  /* Padding and layout live here rather than on the scroll container: a
     scrollable block's padding-bottom is not part of its scrollable area, and
     the bottom gap vanished at exactly the height where the content was flush
     but no scrollbar had appeared yet. For an ordinary in-flow element the
     padding is part of its height, so it always works. */
  .start-inner {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 26px;
    padding: 32px 24px;
    width: 100%;
    /* 900px for .recent plus the horizontal padding */
    max-width: 948px;
  }

  /* The floating-surface recipe, taken whole: the fill is also the start
     screen's own colour, so without the hairline border this panel would be
     invisible here, and without the shadow it would be lost over a bright
     poster. Narrower than `.ctxmenu`'s 256px minimum on purpose — that number
     is sized for the speed row, and this menu is two short answers to one
     question. */
  .rowmenu {
    position: fixed;
    z-index: 60;
    min-width: 190px;
    padding: 6px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 10px;
    background: rgba(16, 16, 22, 0.96);
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.45);
  }

  .rowmenu-head {
    padding: 4px 8px 6px;
    color: #77777f;
    font-size: 11.5px;
  }

  /* The one destructive row, coloured on hover rather than at rest: red text
     sitting in a menu reads as a warning about the menu, not as a description
     of one item in it. */
  .rowmenu .menu-item.danger:hover {
    color: #f0a0a0;
  }

  /* ---- Start screen: continue watching ----
     flex, not the grid inherited from .overlay: that one has `place-items:
     center`, i.e. the column is sized from its content. The card grid's width
     then depends on the cards, and they change as posters finish loading —
     hence the layout jump. In a flex column with align-items: center the
     children's width is computed from the container, not the other way round,
     and the images can no longer affect it.
     The scrollbar's space is reserved in advance — otherwise its appearance
     after loading would shift the content sideways. */
  .overlay.start {
    /* The scroll area starts below the title bar (48px of content height):
       otherwise the scrollbar reaches the top of the window and runs into the
       window controls. Centering then happens within the remaining area, which
       is arguably more correct for the start screen anyway — the bar occupies
       the top regardless. */
    top: 48px;
    display: flex;
    flex-direction: column;
    align-items: center;
    /* `safe` is mandatory: in a centered scrollable flex container the overflow
       goes BOTH ways, and the top of the content is cut off — it cannot be
       scrolled to, the scrollbar does not go there. With `safe`, centering
       applies only while the content fits, and beyond that the layout is
       pinned to the start. */
    justify-content: safe center;
    /* The reserve has to be mirrored, because the content here is centered:
       holding space only on the right moves the center left by half the bar,
       which is noticeable against the centered title in the bar above.

       This was `overflow-y: auto` + `scrollbar-gutter: stable both-edges`, and
       on macOS the gutter is not honoured at all — so the shift it was written
       to prevent was happening in full. Measured in a WKWebView harness: a
       centered 200px child in a 400px box sat at 100/100 while the content fit
       and at **90/110** once it scrolled. Chromium did honour it (100/100 in
       both), so this was a macOS-only defect, and `CSS.supports` reports the
       property as supported on both.

       Forcing the bar on reserves its 10px on the right in either engine, and
       an equal padding on the left mirrors it. Measured: 100/100 in all four
       combinations of engine and overflow. It also costs 10px less width than
       `both-edges` did on Chromium, which reserved a gutter on each side. */
    overflow-y: scroll;
    padding-left: 10px;
  }

  /* The content does not shrink when height runs short — the container scrolls
     instead. `.start-inner`'s children are drawn by the components under
     `start/`, which is what the `:global` is for. */
  .overlay.start > *,
  .start-inner > :global(*) {
    flex: none;
  }
</style>
