<script lang="ts">
  /// The start screen's torrents: one row per torrent on disk, with what it
  /// takes, where it was left, and its actions — reveal, look for a re-upload,
  /// delete.
  ///
  /// The delete menu is not here. It is drawn outside `.overlay`, which is the
  /// start screen's own markup, so the start screen holds it and says which row
  /// asked (`deleteFor`); this list only lights that row's cross.
  import { revealItemInDir } from '@tauri-apps/plugin-opener';

  import { formatTime } from '$lib/format';
  import { IS_MAC } from '$lib/platform';
  import { t } from '$lib/i18n.svelte';
  import {
    torrentIsPlaying,
    type RememberedTorrent,
    type TorrentRow,
  } from '$lib/torrent.svelte';
  import { fmtSize } from '$lib/units';
  import type { FeedItem } from '$lib/feed';

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
    onOpenTorrent: (row: TorrentRow) => void;
    onUpdateTorrent: (known: RememberedTorrent) => void;
    /// The folder of the row whose delete menu is open, or null.
    deleteFor: string | null;
    openDeleteMenu: (row: TorrentRow, anchor: HTMLElement) => void;
  }

  let {
    torrentRows,
    torrentTotal,
    torrentBusy,
    torrentOpening,
    feedUpdates,
    feedChecking,
    torrentResume,
    onOpenTorrent,
    onUpdateTorrent,
    deleteFor,
    openDeleteMenu,
  }: Props = $props();
</script>

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
        {@const update = row.info_hash ? feedUpdates[row.info_hash] : undefined}
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
                <!-- The feed is named on the row because it changes what the
                     row does: this is the one kind of torrent the player
                     watches for new episodes, and without a mark nothing on
                     screen says so until one arrives. -->
                {#if row.known?.feed}
                  <span class="torrow-feed" data-tip={t('start.torrent_feed_tip')}>
                    {t('start.torrent_feed')}
                  </span>
                {/if}
                {#if update}
                  <span class="torrow-update-line">{t('start.torrent_update')}</span>
                {/if}
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
              class:found={!!update}
              disabled={feedChecking === known.infoHash}
              data-tip={update
                ? t('start.torrent_update_tip', { title: update.title })
                : known.feed
                  ? t('torrent.update_tip_feed')
                  : t('torrent.update_tip')}
              aria-label={update ? t('torrent.update_tip_found') : t('torrent.update')}
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

<style>
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

  /* A tag rather than a word in the sentence: it names what kind of torrent
     this is, and it sits beside the counts rather than among them. */
  .torrow-feed {
    padding: 0 5px;
    border: 1px solid rgba(255, 255, 255, 0.14);
    border-radius: 4px;
    color: #9a9aa6;
    font-size: 10px;
    letter-spacing: 0.04em;
  }

  /* Indigo, and the one place on the row that is: an update is the thing on
     this screen worth acting on, and the button it belongs to is lit the same. */
  .torrow-update-line {
    color: #a5b4fc;
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
     `.card-forget` was ~200 lines further down the file they then shared (it
     is in app.css now and a class lighter, which is no reason to rely on it
     either). That is not a hypothetical: it kept `position: absolute`, and with no positioned ancestor
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

  /* Instant under the keyboard, as `.card-forget:focus-visible` makes the
     recents cross. That rule reached these buttons too while both lived in one
     stylesheet — same weight as `.card-forget.torrow-forget` above and later in
     the file — and in app.css it weighs a class less, so the transition is
     taken back here rather than left to a cascade that no longer gives it. */
  .card-forget.torrow-forget:focus-visible {
    transition: none;
  }

  .card-forget.torrow-forget:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.1);
    color: #e8e8ec;
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

  /* Lit at rest while an update is waiting — the feed found it without being
     asked, and a button that looks the same as always would keep that to
     itself. Five classes, to beat the row-hover rule above. */
  .card-forget.torrow-forget.torrow-update.found,
  .torrow:hover .card-forget.torrow-forget.torrow-update.found {
    color: #818cf8;
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

  .torrow.busy {
    opacity: 0.5;
    pointer-events: none;
  }
</style>
