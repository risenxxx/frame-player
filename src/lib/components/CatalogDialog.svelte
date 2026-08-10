<script lang="ts">
  /// The catalog: a grid of titles, and one title's releases.
  ///
  /// Two views in one sheet rather than two dialogs, because they are one
  /// errand — you are choosing a film and then choosing which copy of it — and
  /// a second backdrop over the first would make going back look like leaving.
  ///
  /// Everything that *acts* lives in `catalog.svelte.ts`; this file draws.
  import { tick } from 'svelte';

  // `base` rather than a bare `/tmdb.svg`: SvelteKit resolves static assets
  // against it, and the app is served from a custom protocol in the packaged
  // build rather than from a web root.
  import { base } from '$app/paths';

  import Dialog from '$lib/components/Dialog.svelte';
  import {
    catalog,
    chooseSeason,
    closeTitle,
    notePosterFailed,
    notePosterOk,
    pickTitle,
    posterUrl,
    playRelease,
    releaseTags,
    runSearch,
    setReleaseSort,
    type Release,
  } from '$lib/catalog.svelte';
  import { t } from '$lib/i18n.svelte';
  import { fmtSize } from '$lib/units';

  interface Props {
    onclose: () => void;
  }

  let { onclose }: Props = $props();

  let inputEl = $state<HTMLInputElement | undefined>();

  /// Typing is not a search. A query fires while the fingers are still moving,
  /// and every one of them is a request to somebody else's server — so the
  /// keystrokes are collected first. 350ms is the usual pause-between-words
  /// figure; Enter skips it for somebody who has finished typing and knows it.
  const TYPE_SETTLE_MS = 350;
  let typeTimer: ReturnType<typeof setTimeout> | undefined;

  function onType(value: string) {
    catalog.query = value;
    clearTimeout(typeTimer);
    typeTimer = setTimeout(() => void runSearch(), TYPE_SETTLE_MS);
  }

  function onSubmit() {
    clearTimeout(typeTimer);
    void runSearch();
  }

  /// Focused on the way in: the panel exists to be typed into, and a field that
  /// has to be clicked first puts a step in front of its only purpose.
  $effect(() => {
    void tick().then(() => inputEl?.focus());
  });

  /// The head of a title's page reads "2024 · Фантастика, боевик · 2 ч 09 мин",
  /// with whatever is missing simply absent — a row of separators around empty
  /// strings is worse than a shorter line.
  const pickedMeta = $derived.by(() => {
    const p = catalog.picked;
    if (!p) return '';
    const parts: string[] = [];
    if (p.year) parts.push(String(p.year));
    if (p.genres.length) parts.push(p.genres.slice(0, 3).join(', '));
    if (p.runtime) parts.push(t('catalog.runtime', { minutes: p.runtime }));
    return parts.join(' · ');
  });

  /// A release's dub list, capped. A rip carrying twelve voice-overs would
  /// otherwise be the only row anybody can read on the screen.
  function voicesOf(r: Release): string {
    if (!r.voices.length) return '';
    return r.voices.length > 3
      ? `${r.voices.slice(0, 3).join(', ')} +${r.voices.length - 3}`
      : r.voices.join(', ');
  }
</script>

<Dialog
  title={catalog.picked ? catalog.picked.title : t('catalog.title')}
  label={t('catalog.title')}
  variant="catalog"
  scrollable
  {onclose}
>
  {#if catalog.suppressed}
    <!-- The metadata service reports the catalog as unavailable. One level
         above the address, since an instance may need the panel to stop for
         reasons unrelated to which indexer it points at — and clearing the
         address alone would leave a panel that still looks like it should
         work. Its own sentence when one was supplied, a generic one
         otherwise. -->
    <div class="cat-empty">{catalog.suppressedNotice || t('catalog.unavailable')}</div>
  {:else if !catalog.picked}
    <!-- The grid. The field stays at the top of it rather than becoming a
         header of its own: a search box IS what this view is, and framing it
         separately would make the panel look like it has a toolbar. -->
    <input
      class="link-input"
      bind:this={inputEl}
      value={catalog.query}
      placeholder={catalog.hasMeta ? t('catalog.placeholder') : t('catalog.placeholder_raw')}
      spellcheck="false"
      autocapitalize="off"
      aria-label={t('catalog.title')}
      oninput={(e) => onType(e.currentTarget.value)}
      onkeydown={(e) => {
        if (e.key === 'Enter') onSubmit();
      }}
    />

    {#if !catalog.hasMeta}
      <!-- No metadata service answering. Said once, at the top, rather than as
           an error over an empty grid: nothing is broken, the panel simply has
           no pictures and searches the indexer directly. -->
      <div class="cat-note">{t('catalog.no_key_hint')}</div>
      {@render releaseList()}
    {:else if catalog.phase === 'loading'}
      {@render gridHead()}
      {@render skeletonGrid()}
    {:else if catalog.phase === 'failed'}
      <div class="link-error">{catalog.error}</div>
    {:else if catalog.shown.length === 0}
      <div class="cat-empty">
        {catalog.query.trim() ? t('catalog.nothing') : t('catalog.type_something')}
      </div>
    {:else}
      {@render gridHead()}
      <div class="cat-grid">
        {#each catalog.shown as item (item.kind + item.id)}
          <button class="cat-card" onclick={() => void pickTitle(item)}>
            <span class="cat-poster" class:empty={!item.poster}>
              {#if item.poster}
                <!-- The failure handler is what decides where posters come
                     from: a blocked TMDB CDN looks exactly like an image that
                     will not load, and one such event flips every poster in the
                     panel to the proxy. -->
                <img
                  src={posterUrl(item.poster)}
                  alt=""
                  loading="lazy"
                  onload={notePosterOk}
                  onerror={notePosterFailed}
                />
              {/if}
              {#if item.kind === 'tv'}
                <!-- Series and films sit in one grid, and which is which
                     changes what the next screen offers (a season picker or
                     not). A badge says so before the click rather than after. -->
                <span class="cat-badge">{t('catalog.series')}</span>
              {/if}
            </span>
            <span class="card-name">{item.title}</span>
            <span class="card-left">{item.year ?? ''}</span>
          </button>
        {/each}
      </div>
    {/if}
  {:else}
    {@const p = catalog.picked}
    <button class="cat-back" onclick={closeTitle}>
      <svg viewBox="0 0 16 16" aria-hidden="true">
        <path
          d="M10 3.5 6 8 10 12.5"
          fill="none"
          stroke="currentColor"
          stroke-width="1.8"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
      {t('catalog.back')}
    </button>

    <div class="cat-head">
      <span class="cat-poster cat-poster-big" class:empty={!p.poster}>
        {#if p.poster}<img
            src={posterUrl(p.poster)}
            alt=""
            onload={notePosterOk}
            onerror={notePosterFailed}
          />{/if}
      </span>
      <div class="cat-headtext">
        {#if p.original_title && p.original_title !== p.title}
          <div class="cat-original">{p.original_title}</div>
        {/if}
        {#if pickedMeta}
          <div class="cat-meta">{pickedMeta}</div>
        {/if}
        {#if p.overview}
          <p class="cat-overview">{p.overview}</p>
        {/if}
      </div>
    </div>

    {#if p.seasons.length > 1}
      <!-- Pills rather than a dropdown: this picks one of several named values,
           which is what `.segmented` is for everywhere else in the player. It
           scrolls sideways for a show with twenty seasons — the row is the one
           place a long series must not reshape the sheet. -->
      <div class="cat-seasons">
        <div class="segmented">
          {#each p.seasons as s (s)}
            <button
              class="segopt"
              class:sel={catalog.season === s}
              onclick={() => void chooseSeason(s)}
            >
              {t('catalog.season', { number: s })}
            </button>
          {/each}
        </div>
      </div>
    {/if}

    {@render releaseList()}
  {/if}

  <!-- **Required by the TMDB API terms (§3), not decoration.** The sentence is
       theirs and must be reproduced as written; it belongs in an About or
       Credits section, and the panel that draws their data is the one place a
       viewer will read it. Shown only when there is a key, because with none
       nothing here came from TMDB and claiming otherwise would be its own kind
       of wrong.

       §3 asks for the logo as well as the sentence, kept *less prominent than
       the marks that primarily describe or identify Your Application* — which
       the size does here, against a 42px product name on the start screen and
       the mark in the title bar.

       **Their file, referenced rather than inlined, and unaltered.** It is a
       trademark: it is served as-is from `static/tmdb.svg` instead of being
       pasted into this component, so nothing here can quietly change its
       colours, and its internal `linearGradient id` cannot collide with
       another SVG on the page. No opacity or filter is applied for the same
       reason — the size is what makes it subordinate, not a treatment of the
       mark. Only the height is set, so its own proportions decide the width. -->
  {#if catalog.hasMeta}
    <div class="cat-attr">
      <img class="cat-attr-logo" src="{base}/tmdb.svg" alt="TMDB" />
      <span>{t('catalog.tmdb_notice')}</span>
    </div>
  {/if}
</Dialog>

{#snippet gridHead()}
  <!-- **Rendered by the loading branch as well as the loaded one, and that is
       the point of it being a snippet.** The heading is 35px of layout; drawing
       it only once the data arrives pushes the whole grid — and the
       attribution under it — down by that much at the moment everything else
       settles, which is the jump the skeletons exist to remove. Keeping the
       condition in one place is what stops the two branches drifting apart
       again: they were two copies, and the copy without the heading is exactly
       how this was missed the first time. -->
  {#if !catalog.query.trim()}
    <div class="cat-section">{t('catalog.trending')}</div>
  {/if}
{/snippet}

{#snippet sortRow(label: string)}
  <!-- The count and the order on one line: the order is a real choice here,
       because "the best copy" and "the copy that will actually download" are
       different questions and the list answers whichever is asked. Right of the
       label rather than above the list, so it reads as a property of the
       heading instead of as another section.

       Drawn while the releases are still loading too, with the pills live: they
       only set state, so choosing an order before the answer arrives works —
       and a control that appears with the data would move every row down by the
       difference between a plain line and a row of pills. -->
  <div class="cat-section cat-sortrow">
    <span>{label}</span>
    <div class="segmented cat-sort">
      <button
        class="segopt"
        class:sel={catalog.sort === 'quality'}
        onclick={() => setReleaseSort('quality')}
      >
        {t('catalog.sort_quality')}
      </button>
      <button
        class="segopt"
        class:sel={catalog.sort === 'seeders'}
        onclick={() => setReleaseSort('seeders')}
      >
        {t('catalog.sort_seeders')}
      </button>
    </div>
  </div>
{/snippet}

{#snippet skeletonGrid()}
  <!-- **A placeholder that is the size of the answer, not a spinner.** The
       attribution sits below whatever this branch renders, so a one-line
       spinner puts the TMDB logo just under the search field and the arriving
       grid then shoves it four rows down — a bright mark jumping half the
       sheet. Occupying roughly the final height is the whole point; the
       animation is secondary.

       Twenty because that is what the service returns: TMDB pages both trending
       and search at twenty, so this is the real count rather than a guess, and
       the grid resolves to the same number of rows it will have. -->
  <div class="cat-grid" aria-busy="true" aria-label={t('catalog.searching')}>
    {#each { length: 20 }, i (i)}
      <div class="cat-card">
        <span class="cat-poster skel"></span>
        <!-- Two lines, the second shorter, because that is the shape of a title
             above a year. Uneven widths keep a block of twenty from reading as
             a table. -->
        <span class="skel skel-line" style="width: {70 + ((i * 37) % 26)}%"></span>
        <span class="skel skel-line short"></span>
      </div>
    {/each}
  </div>
{/snippet}

{#snippet skeletonReleases()}
  <div class="cat-releases" aria-busy="true" aria-label={t('catalog.looking')}>
    {#each { length: 6 }, i (i)}
      <div class="skel skel-rel"></div>
    {/each}
  </div>
{/snippet}

{#snippet releaseList()}
  {#if catalog.releasePhase === 'loading'}
    {@render sortRow(t('catalog.releases_loading'))}
    {@render skeletonReleases()}
  {:else if catalog.releasePhase === 'failed'}
    <div class="link-error">{catalog.releaseError}</div>
  {:else if catalog.releasePhase === 'ready' && catalog.releases.length === 0}
    <div class="cat-empty">{t('catalog.no_releases')}</div>
  {:else if catalog.releases.length}
    {@render sortRow(t('catalog.releases', { count: catalog.releases.length }))}
    <div class="cat-releases">
      {#each catalog.sortedReleases as r (r.magnet)}
        <button
          class="rel"
          disabled={catalog.starting !== null}
          onclick={() => void playRelease(r, onclose)}
        >
          <span class="rel-main">
            <span class="rel-title">{r.title}</span>
            <span class="rel-meta">
              <!-- Seeders first and coloured, because on a torrent that is the
                   difference between watching now and waiting — every other
                   figure here is a property of the file, this one is a
                   property of whether it will arrive. -->
              <span class="rel-seed" class:dead={r.seeders === 0}>
                {t('catalog.seeders', { count: r.seeders })}
              </span>
              <span>{fmtSize(r.size)}</span>
              <span class="rel-tracker">{r.tracker}</span>
              {#if voicesOf(r)}<span class="rel-voices">{voicesOf(r)}</span>{/if}
            </span>
          </span>
          <span class="rel-tags">
            {#each releaseTags(r) as tag (tag)}
              <span class="rel-tag">{tag}</span>
            {/each}
          </span>
        </button>
      {/each}
    </div>
  {/if}
{/snippet}

<style>
  /* The attribution required by the TMDB terms. Quiet and last, which is what
     "less prominent than the marks that identify Your Application" asks for,
     but never hidden behind a hover or a fold — it has to be readable wherever
     their data is shown. Separated by the same hairline the settings footer
     uses, so it reads as a footnote to the panel rather than as a row of it. */
  .cat-attr {
    margin-top: 16px;
    padding-top: 10px;
    border-top: 1px solid rgba(255, 255, 255, 0.07);
    color: #6f6f7a;
    font-size: 11px;
    line-height: 1.45;
  }

  /* Only the height is set: the mark is 489×35.4, so letting the width follow
     keeps its own proportions rather than imposing ours. 13px of height puts it
     at ~180px wide — legible, and an order of magnitude below the 42px product
     name on the start screen, which is what "less prominent" asks for.
     `display: block` so the sentence sits under it rather than beside it; a
     180px logo and two lines of text on one row would leave the text in a
     column too narrow to read. */
  .cat-attr-logo {
    display: block;
    height: 13px;
    /* **The ratio has to be stated, and `width: auto` alone is not enough.**
       The file carries a `viewBox` and no `width`/`height` attributes, so it has
       no intrinsic size for `auto` to resolve against — measured in a WKWebView
       harness, `height: 13px; width: auto` rendered a 13×13 square with nothing
       painted in it. The numbers are the viewBox's own, so the mark keeps its
       proportions rather than being given ours. */
    aspect-ratio: 489.04 / 35.4;
    width: auto;
    margin-bottom: 8px;
  }

  /* A note that is not an error: no key in this build, nothing to fix. */
  .cat-note,
  .cat-empty {
    color: #77777f;
    font-size: 12px;
    line-height: 1.45;
    padding: 6px 2px 10px;
  }

  .cat-empty {
    padding: 26px 2px;
    text-align: center;
  }

  .cat-section {
    color: #77777f;
    font-size: 11.5px;
    margin: 10px 2px 10px;
  }

  /* The heading and its order control on one line, the control pushed right.
     `baseline` rather than `center`, so the label sits on the same line as the
     pill text instead of being nudged by the pill's own padding. */
  .cat-sortrow {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
  }

  /* `.segopt` is `flex: 1`, which here would stretch two pills across the whole
     sheet. Written to beat the base rule rather than left to source order —
     app.css and this file are separate stylesheets and the order between them
     is the bundler's, which is the fight this project has now lost twice. */
  .cat-sort {
    flex: none;
    padding: 2px;
  }

  .cat-sort .segopt {
    flex: 0 0 auto;
    padding: 4px 10px;
    font-size: 11.5px;
  }

  /* `auto-fill` with a fixed maximum, not `1fr`: at `1fr` a search that returns
     two titles stretches each poster across half the sheet, and a poster that
     wide stops being a poster. The same trap the recents grid documented before
     it became a rail. */
  .cat-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 150px));
    justify-content: center;
    gap: 16px 16px;
  }

  .cat-card {
    display: block;
    min-width: 0;
    padding: 0;
    border: none;
    background: none;
    color: inherit;
    text-align: left;
    cursor: pointer;
  }

  /* A poster is 2:3, which is the one thing that must not be inherited from
     `.card-poster` — that is 16:9 because it frames a decoded video frame. */
  .cat-poster {
    position: relative;
    display: block;
    aspect-ratio: 2 / 3;
    border-radius: 8px;
    overflow: hidden;
    background: rgba(0, 0, 0, 0.35);
  }

  .cat-poster.empty {
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.07);
  }

  .cat-poster img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .cat-card:hover .cat-poster {
    outline: 2px solid #818cf8;
    outline-offset: 1px;
  }

  /* Over the poster's own corner, so it reads as a label on the picture rather
     than as a row of the caption below it. */
  .cat-badge {
    position: absolute;
    left: 6px;
    top: 6px;
    padding: 2px 6px;
    border-radius: 5px;
    background: rgba(0, 0, 0, 0.72);
    color: #d6d6de;
    font-size: 10.5px;
  }

  /* The two card captions are `.card-name` and `.card-left` from app.css —
     shared with the recents rail, where they were declared. Only the truncation
     ancestors are ours, and both halves are needed or the ellipsis never
     fires. */
  .cat-card .card-name,
  .cat-card .card-left {
    min-width: 0;
    max-width: 100%;
  }

  .cat-back {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin-bottom: 12px;
    padding: 5px 10px 5px 6px;
    border: none;
    border-radius: 7px;
    background: rgba(255, 255, 255, 0.06);
    color: #d6d6de;
    font-size: 12px;
    cursor: pointer;
  }

  .cat-back:hover {
    background: rgba(255, 255, 255, 0.1);
    color: #e8e8ec;
  }

  .cat-back svg {
    width: 14px;
    height: 14px;
  }

  .cat-head {
    display: flex;
    align-items: flex-start;
    gap: 16px;
  }

  .cat-poster-big {
    flex: 0 0 116px;
  }

  .cat-headtext {
    /* Or the description refuses to wrap inside the flex row. */
    min-width: 0;
    flex: 1;
  }

  .cat-original {
    color: #b9b9c3;
    font-size: 13px;
  }

  .cat-meta {
    margin-top: 4px;
    color: #77777f;
    font-size: 11.5px;
  }

  .cat-overview {
    margin: 10px 0 0;
    color: #b9b9c3;
    font-size: 12.5px;
    line-height: 1.5;
  }

  /* The row scrolls rather than wrapping: a show with twenty seasons would
     otherwise make the picker three rows tall and push the releases — the thing
     the page is for — below the fold. */
  .cat-seasons {
    margin-top: 16px;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .cat-seasons::-webkit-scrollbar {
    display: none;
  }

  /* `.segopt` is `flex: 1`, which divides the row between however many seasons
     there are — at twenty that is 25px a pill. Inside a scrolling row each one
     wants its own width instead. Written to beat the base rule rather than left
     to source order, since app.css and this file are separate stylesheets and
     the order between them is the bundler's. */
  .cat-seasons .segmented {
    width: max-content;
    min-width: 100%;
  }

  .cat-seasons .segopt {
    flex: 0 0 auto;
    padding-left: 12px;
    padding-right: 12px;
  }

  .cat-releases {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .rel {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    min-width: 0;
    padding: 9px 11px;
    border: none;
    border-radius: 9px;
    background: rgba(255, 255, 255, 0.035);
    color: #e8e8ec;
    text-align: left;
    cursor: pointer;
    transition: background 120ms ease;
  }

  .rel:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.09);
  }

  .rel:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .rel-main {
    flex: 1;
    min-width: 0;
  }

  /* The tracker's own title, which is the only place the rip's provenance is
     written — truncated rather than wrapped, because two-line rows in a list of
     forty stop being scannable, and the full string is in the tooltip-free
     `title` of nothing: it is what the next screen (the torrent picker) shows
     in full anyway. */
  .rel-title {
    display: block;
    min-width: 0;
    font-size: 12.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .rel-meta {
    display: flex;
    align-items: baseline;
    gap: 9px;
    min-width: 0;
    margin-top: 3px;
    color: #77777f;
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .rel-seed {
    color: #86b782;
  }

  /* Nobody is serving this. Dimmed rather than red: it is not an error, it is a
     release that will take a while or never start, and the viewer may still
     pick it deliberately. */
  .rel-seed.dead {
    color: #77777f;
  }

  .rel-voices {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .rel-tracker {
    color: #6f6f7a;
  }

  .rel-tags {
    display: flex;
    flex: none;
    gap: 5px;
  }

  .rel-tag {
    padding: 2px 7px;
    border-radius: 5px;
    background: rgba(255, 255, 255, 0.08);
    color: #d6d6de;
    font-size: 10.5px;
  }

  /* The skeleton's own fill. Sitting on the sheet's `rgba(16,16,22,0.97)` it
     has to be a lightening rather than a darkening, like every other resting
     surface in the player. */
  .skel {
    background: rgba(255, 255, 255, 0.07);
    border-radius: 6px;
    /* Pulsing the *whole block* rather than a thin stroke: the note on the
       torrent list's delete cross is about a 1.4px glyph, where taking a layer
       off full opacity makes the engine re-rasterise and the mark visibly
       twitches. A 150px slab has no such edge to shimmer.

       Alternating rather than looping, so there is no jump back at the seam,
       and it stops the moment the data lands — the objection to the catalog
       button's gradient (permanent motion in the corner of the eye) does not
       apply to something that exists for half a second. */
    animation: cat-pulse 1.3s ease-in-out infinite alternate;
  }

  /* Its own keyframes, because Svelte scopes them like every other rule in a
     component — the `spin` the torrent rows animate on lives in their file, and
     naming it here would point at nothing at all. (Writing the literal opening
     tag of a style block in a comment is what `css-orphans` finds with
     `lastIndexOf`, so this says it in words instead.) */
  @keyframes cat-pulse {
    to {
      opacity: 0.45;
    }
  }

  /* `.cat-poster` already carries the 2:3 ratio and the radius, so a skeleton
     card is the real card's geometry with nothing in it — which is what makes
     the grid resolve to the height it will actually have. */
  .cat-card .cat-poster.skel {
    background: rgba(255, 255, 255, 0.07);
  }

  /* Matched to `.card-name`: 12.5px text on a 6px top margin gives ~18px of
     line box, so the bar plus its margin occupies the same room the title will. */
  .skel-line {
    display: block;
    height: 9px;
    margin-top: 9px;
    border-radius: 4px;
  }

  /* The year line under the title — `.card-left` is 11.5px and short.
     The 9px margin is not symmetry with the line above it, it is arithmetic:
     measured in a WKWebView harness against the built stylesheet, a 7px margin
     left each skeleton card 2px shorter than the real one, which across four
     rows put the attribution 8px out. At 9px the two grids measure the same. */
  .skel-line.short {
    width: 34%;
    height: 8px;
    margin-top: 9px;
  }

  /* One release row: 9px of padding top and bottom around a 12.5px title and an
     11.5px meta line, which is the 48px this stands in for. */
  .skel-rel {
    height: 48px;
    border-radius: 9px;
  }
</style>
