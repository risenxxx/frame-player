<script lang="ts">
  /// The start screen's "continue watching" rail: one row of cards, what each
  /// one was left at, and the arrows that scroll it.
  ///
  /// The rail's own mechanics live here because nothing outside needs them — a
  /// scroll position and two arrow-visibility flags are as local as state gets.
  /// Opening and forgetting a card reach into the player, so they arrive as
  /// props.
  import { tick } from 'svelte';

  import { formatTime } from '$lib/format';
  import { history, type RecentItem } from '$lib/history.svelte';
  import { t } from '$lib/i18n.svelte';
  import { parseTorrentUrl } from '$lib/source';
  import { isNetworkSource } from '$lib/player.svelte';
  import { rememberedTorrent } from '$lib/torrent.svelte';

  interface Props {
    onOpenRecent: (item: RecentItem) => void;
    onForgetRecent: (item: RecentItem) => void;
  }

  let { onOpenRecent, onForgetRecent }: Props = $props();

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
              <!-- The whole length, under the bar, shown on hover. Not for an
                   entry without a duration: a full track with nothing on it
                   would claim a length the player never learned. -->
              {#if item.dur > 0}
                <span class="card-track"></span>
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

<style>
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
    /* Room for the keyboard focus ring. `:focus-visible` draws a 2px ring at
       `outline-offset: 1px`, which lives 3px outside the card's box and was
       being cut off by this container's own clipping — visible as the top edge
       of the ring disappearing. (The hover used to draw the same ring; it no
       longer leaves the card's box, the focus ring still does.) The negative
       margin gives the padding back so nothing moves, and `scroll-padding`
       keeps a snapped card clear of it. */
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
     `Dutton.Ranch.S01E02.WEB-DLRip-AVC.x264.GROUP`, i.e. every torrent file —
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
     rail's 5px of horizontal padding — there to give the focus ring room —
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

  /* 5px tall and drawn at 3: the hover thickens the bar with `scaleY`, never
     with `height`. The picture under it is a layer of its own (see the hover
     rules below), and WebKit decides per frame whether a box overlapping a
     layer gets one too — while the height was animating that decision kept
     flipping and the bar blinked in and out under the zoomed frame. A bar
     with its own layer and a fixed box gives it nothing to decide. */
  .card-progress {
    position: absolute;
    z-index: 2;
    left: 0;
    bottom: 0;
    height: 5px;
    background: #6366f1;
    transform: scaleY(0.6);
    transform-origin: bottom;
    transition: transform 140ms ease;
    will-change: transform;
  }

  .card-track {
    position: absolute;
    z-index: 2;
    left: 0;
    right: 0;
    bottom: 0;
    height: 5px;
    background: rgba(255, 255, 255, 0.28);
    opacity: 0;
    transform: scaleY(0.6);
    transform-origin: bottom;
    transition: transform 140ms ease, opacity 140ms ease;
    will-change: transform, opacity;
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

  .rail-arrow.on {
    opacity: 1;
  }

  /* ---- Hover: the frame comes closer and the progress steps forward ----
     No ring around the frame. Everything happens inside the poster's own
     box, so the card does not move and the rail needs no room for it.

     Only the <img> is scaled: the track and the bar are its siblings, so they
     stay at the edge and at their own thickness. `isolation` keeps the
     scaled picture inside the rounded corners in WebKit, which otherwise lets
     a transformed child paint square corners for the length of the
     transition, and keeps the z-indexes below to this box.

     The shade under the track is what lets it read on a light frame: white
     at 0.28 over a pale picture is no line at all. It is only as tall and as
     dark as the bar needs — 0.3 under the 5px, gone 24px up — because a
     taller one (45% of the frame at 0.55 was tried) dims the picture the
     hover is showing off. The bar keeps its own
     indigo rather than lightening to #818cf8, for the same reason.

     `.card:hover`, not `.card-open:hover`: the forget cross is a sibling of
     the open button, and pointing at it is still pointing at this card. The
     end screen's twin of this lives in EndScreen.svelte, where `.endcard` is
     drawn. */
  .recent-rail .card-poster {
    isolation: isolate;
  }

  /* **Never quite at scale 1, on purpose.** The poster sits on fractional
     coordinates (200 × 9/16 is 112.5, and the start screen is centered), and
     a layer drawn at an exact identity transform is snapped to whole pixels
     while a scaled one is not — so the frame was drawn unsnapped for the
     length of the zoom back and snapped in its last frame, a visible jump of
     about a pixel. `will-change` alone, which keeps the layer alive, did not
     stop it. 0.1% is 0.2px of overscan, cut off by the poster's own clip.
     There is no brightness filter for the same reason: a second effect ending
     with the transition is a second chance to redraw it differently. */
  .recent-rail .card-poster img {
    transform: scale(1.001);
    transition: transform 280ms cubic-bezier(0.2, 0.7, 0.3, 1);
    will-change: transform;
  }

  .recent-rail .card-poster::after {
    content: '';
    position: absolute;
    z-index: 1;
    left: 0;
    right: 0;
    bottom: 0;
    height: 24px;
    /* Eased rather than linear, so the shade has no visible top edge. */
    background: linear-gradient(
      to top,
      rgba(0, 0, 0, 0.3),
      rgba(0, 0, 0, 0.22) 5px,
      rgba(0, 0, 0, 0.11) 11px,
      rgba(0, 0, 0, 0.04) 17px,
      transparent
    );
    opacity: 0;
    transition: opacity 180ms ease;
    /* Its own layer above the picture's, for the reason on `.card-progress`. */
    will-change: opacity;
    pointer-events: none;
  }

  .recent-rail .card-name,
  .recent-rail .card-left {
    transition: color 140ms ease;
  }

  .card:hover .card-poster img,
  .card-open:focus-visible .card-poster img {
    transform: scale(1.06);
  }

  .card:hover .card-poster::after,
  .card-open:focus-visible .card-poster::after,
  .card:hover .card-track,
  .card-open:focus-visible .card-track {
    opacity: 1;
  }

  .card:hover .card-track,
  .card-open:focus-visible .card-track,
  .card:hover .card-progress,
  .card-open:focus-visible .card-progress {
    transform: none;
  }

  .card:hover .card-name,
  .card-open:focus-visible .card-name {
    color: #fff;
  }

  .card:hover .card-left,
  .card-open:focus-visible .card-left {
    color: #c4c4ce;
  }

  @media (prefers-reduced-motion: reduce) {
    .recent-rail .card-poster img,
    .recent-rail .card-poster::after,
    .card-track,
    .card-progress {
      transition: none;
    }
  }
</style>
