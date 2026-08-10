<script lang="ts">
  /// The start screen: how to open something, what was being watched, and which
  /// torrents are on disk.
  ///
  /// The rail's own mechanics live here because nothing outside needs them — a
  /// scroll position and two arrow-visibility flags are as local as state gets.
  /// Everything that *acts* arrives as a prop instead: opening a file, a link or
  /// a torrent all reach into the player, and this component deliberately knows
  /// nothing about it.
  import { tick } from 'svelte';

  import { flipAxis, shiftAxis } from '$lib/floating';
  import { revealItemInDir } from '@tauri-apps/plugin-opener';

  import { formatTime } from '$lib/format';
  import { IS_MAC } from '$lib/platform';
  import { history, type RecentItem } from '$lib/history.svelte';
  import { t } from '$lib/i18n.svelte';
  import { withKey } from '$lib/keys.svelte';
  import { parseTorrentUrl } from '$lib/source';
  import { isNetworkSource } from '$lib/player.svelte';
  import {
    rememberedTorrent,
    torrentIsPlaying,
    watchedFiles,
    type RememberedTorrent,
    type TorrentRow,
  } from '$lib/torrent.svelte';
  import { fmtSize } from '$lib/units';

  interface Props {
    torrentRows: TorrentRow[];
    torrentTotal: number;
    torrentBusy: string | null;
    torrentOpening: string | null;
    torrentResume: (row: TorrentRow) => { name: string; pos: number; dur: number; index: number } | null;
    onOpenFile: () => void;
    onOpenLink: () => void;
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
    torrentResume,
    onOpenFile,
    onOpenLink,
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

  /**
   * What a recents card is pointing at, for its tooltip.
   *
   * A torrent episode's path is
   * `http://127.0.0.1:53864/t/<40 hex>/3/Silo.S03E04.mkv` — where the port is
   * this run's, the hash is unreadable and neither survives a restart. What
   * identifies the episode there is its name inside the torrent, and the
   * torrent's own name is the context worth adding.
   */
  function recentTip(item: RecentItem): string {
    const ref = parseTorrentUrl(item.path);
    if (!ref) return item.path;
    const file = decodeURIComponent(item.path.split('/').pop() ?? '');
    const known = rememberedTorrent(ref.infoHash);
    return known?.name ? `${known.name} · ${file}` : file;
  }

  // ---- The recents rail ---------------------------------------------------

  let railEl = $state<HTMLDivElement | undefined>();
  /// Both true when the row fits, which is what hides the arrows entirely.
  let railAtStart = $state(true);
  let railAtEnd = $state(true);

  function updateRailEnds() {
    const el = railEl;
    if (!el) return;
    railAtStart = el.scrollLeft <= 1;
    // A pixel of slack: `scrollWidth` and the sum of the fractional layout
    // widths do not always agree to the last unit, and a permanently lit arrow
    // that scrolls nowhere is worse than one that vanishes a pixel early.
    railAtEnd = el.scrollLeft + el.clientWidth >= el.scrollWidth - 1;
  }

  function scrollRail(dir: -1 | 1) {
    const el = railEl;
    if (!el) return;
    // Just under a full view, so the card at the edge stays visible and gives
    // the eye something to carry across — a full-width jump loses the thread.
    el.scrollBy({ left: dir * el.clientWidth * 0.8, behavior: 'smooth' });
  }

  /// Recompute when the list changes: with a short list the rail does not
  /// scroll at all and both arrows have to stay hidden.
  $effect(() => {
    void history.recent.length;
    void tick().then(updateRailEnds);
  });

  /// And when the rail itself is resized — a narrower window turns a row that
  /// fitted into one that scrolls. Observing the element rather than the window
  /// because that is the measurement the arrows actually depend on, and it also
  /// covers the overlay's own scrollbar appearing beside it.
  $effect(() => {
    const el = railEl;
    if (!el) return;
    const ro = new ResizeObserver(updateRailEnds);
    ro.observe(el);
    updateRailEnds();
    return () => ro.disconnect();
  });
</script>

<div class="overlay start scrollable">
  <!-- The padding lives on the inner wrapper, not on the scroll container:
       browsers do not include a scrollable block's padding-bottom in the
       scrollable area, so at the height where the content is flush but no
       scrollbar has appeared yet, the bottom gap vanished and the card text
       ran into the window edge. -->
  <div class="start-inner">
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
    </div>
  </div>
  {#if history.recent.length}
    <div class="recent">
      <div class="recent-head">{t('start.recent')}</div>
      <div class="recent-railwrap">
      <div class="recent-rail" bind:this={railEl} onscroll={updateRailEnds}>
        {#each history.recent as item (item.path)}
          <div class="card">
            <button
              class="card-open"
              aria-label={item.name}
              onclick={() => onOpenRecent(item)}
            >
              <span class="card-poster" class:empty={!item.poster}>
                {#if item.poster}
                  <img src={item.poster} alt="" />
                {:else if isNetworkSource(item.path)}
                  <!-- No frame is coming: a poster is decoded from a file
                       and this entry is a URL. The end-of-file cards learned
                       the same thing — an empty outlined rectangle reads as
                       a preview that failed, so the card says what it is. -->
                  <span class="card-link">
                    <svg viewBox="0 0 24 24" aria-hidden="true">
                      <g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
                        <path d="M10 13.5a4.6 4.6 0 0 0 6.9.5l2.7-2.7a4.6 4.6 0 0 0-6.5-6.5L11.5 6.4"/>
                        <path d="M14 10.5a4.6 4.6 0 0 0-6.9-.5l-2.7 2.7a4.6 4.6 0 0 0 6.5 6.5l1.6-1.6"/>
                      </g>
                    </svg>
                  </span>
                {/if}
                <span
                  class="card-progress"
                  style="width: {item.dur > 0
                    ? Math.min(100, (item.pos / item.dur) * 100)
                    : 0}%"
                ></span>
              </span>
              <!-- The tip is on the caption, not on the whole card. It
                   names what the card already shows in short — the full
                   path, or the torrent this episode came out of — so it is
                   an answer to "which one is this", asked by reading the
                   truncated name. Hung on the poster it was answering a
                   question nobody had, and covering the frame to do it. -->
              <span class="card-meta" data-tip={recentTip(item)} data-tip-below>
                <span class="card-name">{item.name}</span>
                <span class="card-left">
                  {item.dur > 0
                    ? t('start.remaining', { time: formatTime(item.dur - item.pos) })
                    : formatTime(item.pos)}
                </span>
              </span>
            </button>
            <button
              class="card-forget"
              data-tip={t('start.forget')}
              aria-label={t('start.forget')}
              onclick={() => onForgetRecent(item)}
            >
              <!-- An SVG rather than the "×" glyph: the multiplication sign
                   has no guaranteed vertical center in the em square, it
                   sits above the middle of the line, and no place-items
                   fixes that — what gets centered is the line, not the sign. -->
              <svg viewBox="0 0 10 10" aria-hidden="true">
                <path
                  stroke="currentColor"
                  stroke-width="1.4"
                  stroke-linecap="round"
                  d="M1.2 1.2l7.6 7.6M8.8 1.2l-7.6 7.6"
                />
              </svg>
            </button>
          </div>
        {/each}
      </div>
      <!-- Shown only where there is something to reach. Arrows rather than a
           scrollbar because this is a row of targets, not a document — and
           because a trackpad already swipes it (the start overlay is in
           WHEEL_SURFACES, so `onWheel` hands wheel events straight to the
           browser and horizontal ones scroll this natively). -->
      <button
        class="rail-arrow prev"
        class:on={!railAtStart}
        aria-label={t('start.rail_prev')}
        tabindex={railAtStart ? -1 : 0}
        onclick={() => scrollRail(-1)}
      >
        <span>
          <svg viewBox="0 0 16 16" aria-hidden="true">
            <path d="M10 3.5 6 8 10 12.5" fill="none" stroke="currentColor"
              stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" />
          </svg>
        </span>
      </button>
      <button
        class="rail-arrow next"
        class:on={!railAtEnd}
        aria-label={t('start.rail_next')}
        tabindex={railAtEnd ? -1 : 0}
        onclick={() => scrollRail(1)}
      >
        <span>
          <svg viewBox="0 0 16 16" aria-hidden="true">
            <path d="M6 3.5 10 8 6 12.5" fill="none" stroke="currentColor"
              stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" />
          </svg>
        </span>
      </button>
      </div>
    </div>
  {/if}

  <!-- Torrents, one row per RAW TORRENT rather than per file — which is the
       whole reason this is not a duplicate of the grid above. That grid
       holds individual episodes with positions; a torrent is the container,
       and its episodes that were never started have no representation
       anywhere else. So this is how you get back into a season without the
       magnet, and the disk figure is secondary to that rather than the point
       of it: leading with a size would turn the start screen into a
       download manager, which is precisely what this player is not. -->
  {#if torrentRows.length}
    <div class="recent torrents">
      <div class="recent-head">
        <span>{t('start.torrents')}</span>
        <span class="torrents-total">{fmtSize(torrentTotal)}</span>
      </div>
      <div class="torrent-list">
        {#each torrentRows as row (row.folder)}
          {@const resume = torrentResume(row)}
          {@const opening = torrentOpening === row.folder}
          <div class="torrow" class:busy={torrentBusy === row.folder}>
            <!-- Openable on the magnet, which the info hash supplies when
                 nothing was remembered — a torrent on this disk that our own
                 store has never heard of is still this torrent. Only a folder
                 from the older name-based layout has nothing to open with. -->
            <button
              class="torrow-open"
              disabled={!row.magnet || opening}
              data-tip={row.magnet
                ? resume
                  ? t('start.torrent_continue_tip', { name: resume.name })
                  : t('start.torrent_open')
                : t('start.torrent_unknown')}
              onclick={() => onOpenTorrent(row)}
            >
              <span class="torrow-name">{row.name ?? t('start.torrent_unnamed')}</span>
              <span class="torrow-meta">
                <!-- Resolving a magnet is a DHT lookup — a second at best,
                     and the row used to sit there looking untouched for all
                     of it. It answers the click immediately instead. -->
                {#if opening}
                  <span class="torrow-working">
                    <span class="torrow-spin"></span>
                    {t('torrent.resolving')}
                  </span>
                {:else}
                  {#if row.known}
                    <span>{t('start.torrent_files', { count: row.known.videos })}</span>
                  {/if}
                  <span>{fmtSize(row.size)}</span>
                  {#if resume}
                    <span class="torrow-resume">
                      {t('start.torrent_resume', {
                        name: resume.name,
                        time: formatTime(Math.max(0, resume.dur - resume.pos)),
                      })}
                    </span>
                  {/if}
                {/if}
              </span>
            </button>
            <!-- Grouped, so `.torrow`'s 11px gap falls once — between the
                 text and the actions — rather than between every pair and
                 undoing the centering it was computed for. -->
            <div class="torrow-actions">
            <!-- Where the files are is a question this section raises and,
                 until now, refused to answer: it is the one place that says
                 how much disk a torrent takes, while the folder itself sits in
                 a cache directory under an info-hash name that nobody would
                 find by browsing. The path comes from Rust because the root is
                 Rust's to know — and is about to stop being a constant. -->
            <button
              class="card-forget torrow-forget torrow-reveal"
              data-tip={IS_MAC ? t('start.torrent_reveal_mac') : t('start.torrent_reveal_win')}
              aria-label={IS_MAC ? t('start.torrent_reveal_mac') : t('start.torrent_reveal_win')}
              onclick={() => void revealItemInDir(row.path)}
            >
              <!-- Nudged right and up by (0.25, 0.75) of the viewBox against
                   the obvious drawing, and the numbers are measured rather than
                   dialled in by eye. A folder is not a symmetric glyph: the tab
                   puts extra outline in the top-left, so the *centroid of the
                   stroke* — sum of segment lengths times their midpoints — lands
                   at (7.76, 8.74) in a box whose centre is (8, 8). Every other
                   icon in this set is a shape whose bounding box and centroid
                   coincide (the plus, the cross), which is why this one alone
                   read as sitting low and left inside its 28px button.

                   Written into the three absolute coordinates rather than as a
                   `transform`, so the path *is* centred and nothing downstream
                   has to know about a correction. At the 14px render that is
                   0.22px right and 0.66px up. -->
              <svg viewBox="0 0 16 16" aria-hidden="true">
                <path
                  d="M2.75 11.45V3.85a.8.8 0 0 1 .8-.8h2.6l1.3 1.5h5.5a.8.8 0 0 1 .8.8v6.1a.8.8 0 0 1-.8.8H3.55a.8.8 0 0 1-.8-.8z"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.4"
                  stroke-linejoin="round"
                />
              </svg>
            </button>
            {#if row.known}
              {@const known = row.known}
              <button
                class="card-forget torrow-forget torrow-update"
                data-tip={t('torrent.update_tip')}
                aria-label={t('torrent.update')}
                onclick={() => onUpdateTorrent(known)}
              >
                <!-- The same plus as "add an excluded folder", deliberately
                     one glyph rather than two near-identical ones. A ring was
                     here first and was the only curve in a 16px set of
                     chevrons, crosses and strokes — next to the delete cross
                     it read as borrowed from elsewhere. This is built exactly
                     as that cross is (two straight strokes, round caps, same
                     weight), which also makes the pair legible as what it is:
                     one adds episodes, the other removes the lot. A download
                     arrow was the other candidate and lost on meaning — that
                     glyph already says "fetching right now" in `.torchip`. -->
                <svg viewBox="0 0 16 16" aria-hidden="true">
                  <path
                    d="M8 3.5v9M3.5 8h9"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.5"
                    stroke-linecap="round"
                  />
                </svg>
              </button>
            {/if}
            <button
              class="card-forget torrow-forget"
              class:open={deleteFor === row.folder}
              disabled={torrentIsPlaying(row)}
              data-tip={torrentIsPlaying(row)
                ? t('start.torrent_playing')
                : t('start.torrent_delete')}
              aria-label={t('start.torrent_delete')}
              onclick={(e) => openDeleteMenu(row, e.currentTarget)}
            >
              <svg viewBox="0 0 10 10" aria-hidden="true">
                <path
                  stroke="currentColor"
                  stroke-width="1.4"
                  stroke-linecap="round"
                  d="M1.2 1.2l7.6 7.6M8.8 1.2l-7.6 7.6"
                />
              </svg>
            </button>
            </div>
          </div>
        {/each}
      </div>
    </div>
  {/if}
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
  /* The two ways in, side by side: the file picker is the primary action and
     the link sits next to it as a plain one. */
  .start-actions {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 16px;
  }

  .recent {
    width: 100%;
    max-width: 900px;
    flex: none;
  }

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

  .recent-head {
    color: #b9b9c3;
    font-size: 12.5px;
    margin-bottom: 16px;
    text-align: center;
  }

  /* Secondary to the grid above, and deliberately so: this section is how you
     get back into a season, not the primary "what do I watch" answer. A second
     grid of poster cards would compete with the first and turn the start screen
     into two equal halves. */
  .torrents {
    margin-top: 34px;
  }

  .torrents .recent-head {
    display: flex;
    align-items: baseline;
    justify-content: center;
    gap: 10px;
  }

  .torrents-total {
    color: #77777f;
    font-variant-numeric: tabular-nums;
  }

  .torrent-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    /* Matches the recents grid at its narrowest so the two sections line up
       rather than each finding its own width from its content. */
    width: min(560px, 100%);
    margin: 0 auto;
  }

  /* The row is a container, not a control — exactly like `.queue-row`, which
     also carries no hover and no fill of its own. Highlighting the whole row
     said "this is one clickable thing" when it is two with different actions,
     and it lit up even for a row whose open button is disabled (a torrent from
     the older layout, which cannot be reopened at all). Putting the hover on the
     button instead makes the disabled case correct for free.

     The resting fill is what gives the list structure on a start screen that
     has no panel around it. Keeping it and highlighting the button was what
     produced a visible seam in the first attempt — the highlight stopped where
     the cross began and the row's own fill carried on past it in a different
     shade. The fix is not to drop the fill but to **inset** the highlight: the
     row pads itself, so the button's pill floats inside the fill and touches
     none of its edges. Rendering the two settled it. */
  .torrow {
    display: flex;
    align-items: stretch;
    /* 11px, and it is arithmetic rather than taste: the cross has 8px of margin
       plus the row's 3px of padding to its right, so 11px is what sits between
       it and the row's edge. Matching that on the left centers it in the space
       the highlight leaves — measured, it was 2px against 11px, and the cross
       visibly hugged the highlight instead of sitting between the two edges. */
    gap: 11px;
    padding: 3px;
    border-radius: 11px;
    background: rgba(255, 255, 255, 0.035);
  }

  .torrow-open {
    flex: 1;
    /* Both halves, or the ellipsis below never fires: a flex item defaults to
       `min-width: auto` and refuses to shrink below its content. */
    min-width: 0;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 3px;
    padding: 10px 12px;
    border: none;
    /* Rounded on every side: the highlight is a pill of its own, not a slab cut
       off where the cross begins. */
    border-radius: 8px;
    background: transparent;
    color: #e8e8ec;
    font-size: 13px;
    text-align: left;
    cursor: pointer;
    transition: background 120ms ease;
  }

  /* 0.06 rather than `.menu-item:hover`'s 0.09 because this one composites over
     the row's own 0.035: 0.035 + 0.06·0.965 ≈ 0.093, so what the eye gets is the
     same step the queue gives. */
  .torrow-open:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.06);
  }

  /* A torrent from the older name-based layout, or one opened with history off:
     the data is there and can be removed, but there is no magnet to reopen it
     with. Dimmed rather than hidden — it is occupying disk either way. */
  .torrow-open:disabled {
    cursor: default;
    color: #9a9aa6;
  }

  .torrow-name {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .torrow-meta {
    display: flex;
    align-items: baseline;
    gap: 8px;
    max-width: 100%;
    overflow: hidden;
    color: #77777f;
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .torrow-resume {
    overflow: hidden;
    text-overflow: ellipsis;
    color: #9a9aa6;
  }

  .torrow-working {
    display: flex;
    align-items: center;
    gap: 7px;
    color: #b9b9c3;
  }

  .torrow-spin {
    flex: none;
    width: 11px;
    height: 11px;
    border-radius: 50%;
    border: 1.5px solid rgba(255, 255, 255, 0.25);
    border-top-color: #b9b9c3;
    animation: spin 0.8s linear infinite;
  }

  /* `.card-forget` is built to sit on top of a poster and stay invisible until
     the card is hovered. Both of those have to be undone here: the row is not a
     `.card`, so its hover rule never fires and the button would be permanently
     invisible — a delete control nobody can find.

     **Written to win, not left to source order.** `.card-forget` and a bare
     `.torrow-forget` are both one class, so the later rule takes it — and
     `.card-forget` is ~200 lines further down this file. That is not a
     hypothetical: it kept `position: absolute`, and with no positioned ancestor
     in the row the button escaped to `top: 4px; right: 4px` of the overlay and
     sat under the title-bar logo. Exactly the fight `.menu-item.torrent-file`
     above already documents losing once. */
  /* `.card-forget`'s own dark disc is for sitting on top of a poster, where a
     scrim is what makes a white glyph readable over an arbitrary frame. Over
     this row there is no frame to fight, and it inverted the app's one
     direction: every hover here lightens.

     **Visible at rest, unlike the recents cards and the queue** — and that is a
     difference in what the list is for, not an inconsistency. There, deletion
     is a rare correction to a list whose point is playing something, so hiding
     it until hover keeps the common case clean. Here it is half the reason the
     section exists: this is the only place disk usage is shown and the only
     place it can be reclaimed. And a row from the older layout has its open
     button disabled, so with the cross hidden it offers *no* action at all and
     reads as something the player is showing you for no reason. Quiet at rest,
     full strength when the row is under the cursor, lit when it is. */
  /* **Opacity is fixed at 1 and never animated**, and the three strengths come
     from color instead. Since the cross is now permanently visible there is
     nothing opacity buys, and animating it on this element costs something:
     taking a layer off full opacity makes the engine re-rasterise it and switch
     how it antialiases, which on a 1.4px stroke reads as the glyph twitching as
     the pointer arrives — the "jumps and comes back" this replaced. A color
     transition changes no geometry and no compositing. */
  .card-forget.torrow-forget {
    position: static;
    flex: none;
    align-self: center;
    /* Larger than `.card-forget`'s 22px, which is sized for sitting on a poster
       thumbnail. These are the only controls on their row and the row is 58px
       tall; at 22px with an 8px glyph they read as afterthoughts, and a ~1px
       ring at that size antialiases unevenly enough to look crooked. */
    width: 28px;
    height: 28px;
    background: transparent;
    color: #6f6f7a;
    opacity: 1;
    transition: background 120ms ease, color 120ms ease;
  }

  .torrow:hover .card-forget.torrow-forget,
  .card-forget.torrow-forget:focus-visible {
    color: #b9b9c3;
  }

  .card-forget.torrow-forget:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.1);
    color: #e8e8ec;
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

  /* Lit while its panel is open, so the row that is asking is obvious when two
     are near each other. Written to beat `.torrow:hover .card-forget` (three
     classes) rather than left to source order — the fight this file has now
     lost twice. */
  .card-forget.torrow-forget.open {
    background: rgba(255, 255, 255, 0.1);
    color: #e8e8ec;
  }

  /* The row's two actions as one group, so `.torrow`'s 11px gap falls between
     the text and them rather than between every pair — which is what keeps the
     cross centered against the row's edge (see `.torrow`). The 8px that used to
     sit on the cross moves here for the same reason. */
  .torrow-actions {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-right: 8px;
  }

  .card-forget.torrow-forget svg {
    width: 10px;
    height: 10px;
  }

  /* A shade larger than the cross: a plus and a cross are the same two strokes
     forty-five degrees apart, but diagonals cover more of their box, so at equal
     size the plus reads lighter. Measured, this lands the glyph at ~8.1 against
     the cross's 7.60 — the small bump that makes them look equal.

     Written to beat `.torrow-forget svg` above rather than left to source order,
     since the update button carries both classes. */
  .card-forget.torrow-forget.torrow-update svg {
    width: 14px;
    height: 14px;
  }

  /* An outline the size of the plus rather than of the cross: a folder is a
     closed shape carrying its own tab, and at the cross's 10px the notch stops
     being legible as one. Written to beat `.torrow-forget svg` for the same
     reason the update button is — the button carries both classes and equal
     specificity would leave it to source order. */
  .card-forget.torrow-forget.torrow-reveal svg {
    width: 14px;
    height: 14px;
  }

  /* Written to beat `.torrow-forget:hover` (four classes), not left to source
     order — the lesson this file has now learned twice. Indigo rather than the
     cross's red-ish white: this one adds episodes. */
  .card-forget.torrow-forget.torrow-update:hover:not(:disabled) {
    color: #a5b4fc;
  }

  /* The torrent playing right now. Clearly below the resting strength above, or
     "cannot be deleted" and "can be deleted" look the same until clicked. More
     specific than the rule above on purpose — that one would otherwise brighten
     it on row hover. */
  .torrow:hover .card-forget.torrow-forget:disabled,
  .card-forget.torrow-forget:disabled {
    color: #3d3d45;
    cursor: default;
  }


  /* auto-FIT, not auto-fill: fill keeps empty columns in place, so a single
     card was pinned to the left of a wide empty grid. fit collapses the unused
     columns (together with their gaps) and a group of 1-3 cards is centered by
     justify-content.
     The 220px upper bound is mandatory: without it (at 1fr) a lone card would
     stretch across the whole list. In a full row it is never reached — four
     columns in 900px give 214px each — so the card size is unchanged.
     Caveat: an incomplete LAST row (a 5th card under four, say) is still pinned
     left — grid cannot center a partial row in principle. That is ordinary grid
     behavior, and it was not what the complaint was about. */
  /* **One row, always.** As a wrapping grid this was the only part of the start
     screen that grew with its content, and once network entries stopped being
     silently dropped it went three rows deep and pushed the torrents section
     220px below the fold at 1280x800 — measured. A rail keeps the page a fixed
     height whatever the history holds, so the list below it is never missed.

     **`safe center`, never a bare `center`.** A few cards should sit centered as
     the grid did, but a centered flex container that overflows does so in BOTH
     directions and the start becomes unreachable — no scrollbar goes there.
     Rendered with a bare `center`, twelve cards began at the fifth with the
     first four clipped off the left edge for good. `safe` falls back to
     flex-start the moment the content stops fitting, which is the same fix the
     start overlay itself already carries. */
  .recent-rail {
    display: flex;
    justify-content: safe center;
    gap: 14px;
    overflow-x: auto;
    /* Room for the hover outline. `.card-poster` draws a 2px ring at
       `outline-offset: 1px`, which lives 3px outside the card's box and was
       being cut off by this container's own clipping — visible as the top edge
       of the ring disappearing. The negative margin gives the padding back so
       nothing moves, and `scroll-padding` keeps a snapped card clear of it. */
    overflow-y: hidden;
    padding: 5px;
    margin: -5px;
    scroll-padding-inline: 5px;
    scroll-behavior: smooth;
    /* Proximity, not mandatory: a flick that lands between cards should settle
       on the nearer one, not be dragged back for the sake of alignment. */
    scroll-snap-type: x proximity;
    /* The one place the universal scrollbar skin is turned off on purpose. The
       arrows and the trackpad are the affordance here, and a bar under a single
       row of cards reads as the page having a second scroller. */
    scrollbar-width: none;
  }

  .recent-rail::-webkit-scrollbar {
    display: none;
  }

  /* `flex: 0 0 200px` is only half of a fixed width: **`min-width: auto` is the
     default and it wins**, so a card whose name is one long unbreakable token —
     `Dutton.Ranch.S01E02.WEB-DLRip-AVC.x264.seleZen`, i.e. every torrent file —
     grew to fit it. Measured at 312px against its neighbours' 200, and since the
     poster's height comes from its width that card was also half again as tall.
     The same rule the queue rows already carry, missed here. */
  .recent-rail .card {
    flex: 0 0 200px;
    min-width: 0;
    scroll-snap-align: start;
  }

  .recent-railwrap {
    position: relative;
  }

  /* Over the rail's edges rather than beside it: the row is already as wide as
     the screen allows, and taking 40px off each side for arrows would cost a
     card. Circular for the same reason the mini player's close button is —
     nothing to disagree with the card corners underneath. */
  /* Centered ON the rail's edge, half outside it. Inside, it covered the first
     card it was meant to reveal; fully outside, it drifted away from the row it
     belongs to. Straddling costs nothing here because `.start-inner` carries
     24px of horizontal padding — more than the 16px overhang — so nothing
     reaches the scroll container and no horizontal scrollbar can appear. */
  .rail-arrow {
    position: absolute;
    /* Level with the posters rather than the whole card: the name and the
       remaining time sit below and would pull the arrow off center. */
    top: 0;
    bottom: 26px;
    width: 32px;
    display: grid;
    place-items: center;
    padding: 0;
    border: none;
    background: none;
    color: #e8e8ec;
    cursor: pointer;
    opacity: 0;
    transition: opacity 140ms ease;
  }

  /* 21, not 16, and the extra 5 is not a fudge. A scroll container's content
     *begins* at its content edge but is *clipped* at its padding edge, so the
     rail's 5px of horizontal padding — there to give the hover outline room —
     puts the row's start and its cut-off 5px apart. Measured: at scrollLeft 0
     the first card sits at the content edge, while cards on the right run all
     the way to the padding edge. So the left arrow lines up on the first card
     and this one on the clip boundary, and each is on the edge it actually
     shares with the row. */
  .rail-arrow.next {
    right: -21px;
  }

  /* 32 flat, border included — the circle has to be the button it sits in, or
     the glyph lands a pixel off center and the chevron visibly leans. That is
     the border-box reset doing it rather than a line here. */
  .rail-arrow span {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 50%;
    background: rgba(16, 16, 22, 0.92);
    border: 1px solid rgba(255, 255, 255, 0.12);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.45);
  }

  .rail-arrow:hover span {
    background: rgba(32, 32, 42, 0.96);
  }

  /* **Optically centered, which is not the same as geometrically centered.** A
     chevron's ink sits in its open arms while the eye reads its vertex, so a
     bbox-centered one leans away from the point: measured on the rendered
     pixels, the center of mass of `‹` is 0.18px right of the button's center
     and `›` 0.23px left — exactly the directions they look wrong in. The nudge
     is toward the point, the same correction a play triangle needs. */
  .rail-arrow svg {
    width: 15px;
    height: 15px;
  }

  .rail-arrow.next svg {
    transform: translateX(0.5px);
  }

  .card {
    position: relative;
  }

  .card-open {
    display: block;
    width: 100%;
    padding: 0;
    background: none;
    border: none;
    text-align: left;
    cursor: pointer;
    color: inherit;
  }

  .card-progress {
    position: absolute;
    left: 0;
    bottom: 0;
    height: 3px;
    background: #6366f1;
  }

  /* The card's caption, and with it the tooltip's hit area: the name and the
     remaining time, never the poster above them. It gets no box of its own —
     `.card-name`'s 6px top margin collapses straight through it, so the wrapper
     covers the two line boxes and not the gap between the caption and the
     frame, and the card is exactly as tall as before. Measured in both engines:
     the card and both lines land on the same pixel with and without it. */
  .card-meta {
    display: block;
  }

  .card-forget {
    position: absolute;
    top: 4px;
    right: 4px;
    width: 22px;
    height: 22px;
    display: grid;
    place-items: center;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: rgba(0, 0, 0, 0.6);
    color: #fff;
    cursor: pointer;
    opacity: 0;
    /* Only the fade-out stays animated — see the rule below. */
    transition: opacity 0.12s ease;
  }

  .card-forget svg {
    width: 8px;
    height: 8px;
    fill: none;
    display: block;
  }

  /* Appearance is instant (transition: none), like the fullscreen veil: the
     card outline is not animated at all, and any delay on the cross reads as
     "the card highlight is fading in slowly". */
  .card:hover .card-forget,
  .card-forget:focus-visible {
    opacity: 1;
    transition: none;
  }

  /* And on the ancestors between the flex item and the ellipsis, or the label
     never gets the chance to truncate. */
  .recent-rail .card-open,
  .recent-rail .card-meta,
  .recent-rail .card-name,
  .recent-rail .card-left {
    min-width: 0;
    max-width: 100%;
  }

  .rail-arrow.prev {
    left: -16px;
  }

  .rail-arrow.prev svg {
    transform: translateX(-0.5px);
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

  .torrow.busy {
    opacity: 0.5;
    pointer-events: none;
  }

  .rail-arrow.on {
    opacity: 1;
  }

  /* The content does not shrink when height runs short — the container scrolls
     instead. */
  .overlay.start > *,
  .start-inner > * {
    flex: none;
  }

  /* The end screen's twin of this lives in +page.svelte, where `.endcard` is
     drawn. */
  .card-open:hover .card-poster {
    outline: 2px solid #818cf8;
    outline-offset: 1px;
  }
  .panel h1 {
    font-size: 42px;
    font-weight: 600;
    letter-spacing: 0.06em;
    margin: 0 0 8px;
  }
</style>
