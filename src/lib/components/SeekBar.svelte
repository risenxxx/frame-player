<script lang="ts">
  /// The seekbar: the track, the chapter notches, the A–B band, the buffered
  /// map for a torrent, and the hover preview.
  ///
  /// The gestures themselves are in `$lib/seek.svelte.ts` — this draws what they
  /// report and hands them the pointer events. The invariant that matters here
  /// is that the knob reads `seek.value`, never `player.timePos`: while a
  /// gesture owns the position those two disagree on purpose.
  import { formatTime } from '$lib/format';
  import { chapterTitle, player, type Chapter } from '$lib/player.svelte';
  import { onSeekDown, onSeekMove, onSeekUp, seek } from '$lib/seek.svelte';
  import { thumbs } from '$lib/thumbs.svelte';
  import { positionBuffered, torrent } from '$lib/torrent.svelte';

  interface Props {
    /// What the bar measures against — mpv's duration once it has one, and the
    /// remembered duration from the history while the file is still opening.
    barDuration: number;
    /// Chapter boundaries as percentages of the bar.
    chapterMarks: number[];
    abRegion: { left: number; width: number; armed: boolean } | null;
    hoverChapter: Chapter | null;
    hasThumbs: boolean;
    thumbAspect: string;
    mini: boolean;
  }

  let {
    barDuration,
    chapterMarks,
    abRegion,
    hoverChapter,
    hasThumbs,
    thumbAspect,
    mini,
  }: Props = $props();

  const hasFile = $derived(player.hasFile);
</script>

<div class="seekrow" class:mini>
  <span class="time">{formatTime(seek.value)}</span>
  <div
    class="seekwrap"
    role="presentation"
    bind:this={seek.wrapEl}
    onpointerdown={onSeekDown}
    onpointermove={onSeekMove}
    onpointerup={onSeekUp}
    onmouseleave={() => {
      if (!seek.dragging) seek.hoverTime = null;
    }}
  >
    {#if seek.hoverTime !== null && hasFile && player.duration > 0}
      <div class="hovertip" style="left: {seek.hoverX}px">
        {#if hasThumbs}
        <!-- The box keeps the VIDEO's aspect, not a hardcoded 16:9. mpv
             reports `dwidth`/`dheight` — the size with the aspect already
             applied — as soon as a file opens, so this is exact and needs no
             frame to have arrived. Without it the placeholder was 16:9 and
             the first real frame resized the whole popup, which a 4:3 film
             or a phone clip did on every single hover. -->
        {@const aspect = thumbAspect}
        <!-- A position of a torrent that has not been downloaded cannot be
             decoded at all (see `positionBuffered`). An empty outlined box
             there reads as a preview that BROKE rather than one that was
             never coming — the mistake the end-of-file cards already learned
             not to make — so it says which it is. -->
        {@const previewable =
          !thumbs.partial || positionBuffered(seek.hoverTime / Math.max(1e-9, player.duration))}
        <div class="thumbwrap" class:loading={thumbs.loading && previewable}>
          {#if previewable && thumbs.src}
            <img class="thumb" src={thumbs.src} alt="" style="aspect-ratio: {aspect}" />
          {:else if previewable}
            <div class="thumb placeholder" style="aspect-ratio: {aspect}"></div>
          {:else}
            <div class="thumb placeholder pending" style="aspect-ratio: {aspect}">
              <svg viewBox="0 0 16 16" aria-hidden="true">
                <path
                  d="M8 2.5v8m0 0L4.8 7.3M8 10.5l3.2-3.2M3 13.5h10"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.6"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                />
              </svg>
            </div>
          {/if}
          {#if previewable && thumbs.fading}
            <!-- Keyed so a fade following another gets a fresh element and
                 therefore restarts the animation instead of inheriting a
                 finished one. -->
            {#key thumbs.fading}
              <img class="thumb fading" src={thumbs.fading} alt="" />
            {/key}
          {/if}
          <div class="spinner" class:on={thumbs.loading && previewable}></div>
        </div>
        {/if}
        <span>{formatTime(seek.hoverTime)}</span>
        {#if hoverChapter}
          <span class="hover-chapter">{chapterTitle(hoverChapter)}</span>
        {/if}
      </div>
    {/if}
    <div class="seektrack">
      <!-- What is on disk, drawn UNDER the played fill: the bar's job here
           is to answer "will a jump there land, or wait", which is the one
           question a torrent's seekbar is really read for. -->
      {#each torrent.buffered as [from, to] (from)}
        <div
          class="seekbuffered"
          style="left: {from * 100}%; width: {(to - from) * 100}%"
        ></div>
      {/each}
      <div class="seekfill" style="width: {barDuration > 0 ? (seek.value / barDuration) * 100 : 0}%"></div>
      {#if abRegion}
        <div
          class="abregion"
          class:disarmed={!abRegion.armed}
          style="left: {abRegion.left}%; width: {abRegion.width}%"
        ></div>
      {/if}
      {#each chapterMarks as mark, i (i)}
        <div class="chapmark" style="left: {mark}%"></div>
      {/each}
      <div class="seekknob" style="left: {barDuration > 0 ? (seek.value / barDuration) * 100 : 0}%"></div>
    </div>
  </div>
  <span class="time">{formatTime(barDuration)}</span>
</div>

<style>
  /* Timecodes above the bar (current on the left, duration on the right), the
     bar full-width below: the DOM is unchanged, flex order does the arranging. */
  .seekrow {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
  }

  .seekrow .time:first-child {
    order: 1;
    margin-right: auto;
    margin-left: 8px;
    text-align: left;
  }

  .seekrow .time:last-child {
    order: 2;
    margin-right: 8px;
    text-align: right;
  }

  .seekwrap {
    order: 3;
    /* slightly narrower than the row: the track edges line up with the visible
       edges of the icons below */
    flex: 1 0 calc(100% - 16px);
    margin: 6px 8px 0;
    position: relative;
    display: flex;
    align-items: center;
    padding: 6px 0;
    cursor: pointer;
    touch-action: none;
  }

  .seektrack {
    position: relative;
    width: 100%;
    height: 4px;
    border-radius: 2px;
    background: rgba(255, 255, 255, 0.28);
  }

  /* Below the played fill in DOM order, so the white overtakes it rather than
     the other way round. Dimmer than the fill and brighter than the track: it
     is neither what has been watched nor empty bar, and the three have to be
     distinguishable at a glance on a 4px strip. */
  .seekbuffered {
    position: absolute;
    top: 0;
    bottom: 0;
    border-radius: 2px;
    background: rgba(255, 255, 255, 0.3);
  }

  .seekfill {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    border-radius: 2px;
    background: #fff;
  }

  /* The A–B segment. Indigo because it lies over the bar's own white fill and
     the video behind it, where another white would simply vanish — the same
     exception .card-progress makes. Under the chapter notches and the knob in
     DOM order, so neither is swallowed by it. */
  .abregion {
    position: absolute;
    top: -2px;
    bottom: -2px;
    background: rgba(99, 102, 241, 0.55);
    border-radius: 2px;
    pointer-events: none;
    transition: background 0.15s ease;
  }

  /* Marks still set, loop not running (playback is past B). Faint rather than
     gone: the segment is still there to come back to, and mpv re-arms it by
     itself the moment playback is inside it again. */
  .abregion.disarmed {
    background: rgba(99, 102, 241, 0.2);
  }

  /* Chapter boundaries: a notch cut through both the fill and the empty part of
     the track, so it reads the same on either side of the playhead. Not part of
     the hit area — every pointer event on the bar belongs to .seekwrap, and a
     marker that swallowed a click would be a dead pixel column in the middle of
     the seekbar. */
  .chapmark {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 2px;
    margin-left: -1px;
    border-radius: 1px;
    background: rgba(16, 16, 22, 0.9);
    pointer-events: none;
  }

  .seekknob {
    position: absolute;
    top: 50%;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: #fff;
    transform: translate(-50%, -50%);
    transition: transform 0.12s ease;
    pointer-events: none;
  }

  .seekwrap:hover .seekknob {
    transform: translate(-50%, -50%) scale(1.3);
  }

  /* No container: a thumbnail with a white outline and a shadowed timestamp
     straight over the video */
  .hovertip {
    position: absolute;
    bottom: 18px;
    transform: translateX(-50%);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    color: #fff;
    font-size: 13px;
    font-variant-numeric: tabular-nums;
    pointer-events: none;
    white-space: nowrap;
    text-shadow: var(--ui-shadow);
  }

  .hovertip .thumbwrap {
    position: relative;
  }

  /* Capped at the thumbnail width on purpose: seek.hoverX is clamped assuming the
     tip is exactly that wide, so a long chapter title would push the whole
     popup past the window edge. */
  .hover-chapter {
    max-width: 184px;
    margin-top: -4px;
    overflow: hidden;
    text-overflow: ellipsis;
    color: #d6d6de;
    font-size: 12px;
  }

  /* 184, border included — the outer width of the popup, which is the figure
     everything around it is written against: `.hover-chapter` caps itself at
     the same number, and `onSeekHover` clamps seek.hoverX by half of it (92). Those
     agreed with the rendered box while this said 180 and grew by its outline;
     with the border-box reset they agree with the declaration too.
     One consequence of the box changing meaning: `aspect-ratio` follows
     box-sizing, so the video's shape is now the shape of the outlined frame
     rather than of the picture inside it. That is a 2px inset on each side —
     under 2% of the ratio, taken by `object-fit: cover` — and the frame is the
     part anyone sees. */
  .hovertip .thumb {
    display: block;
    width: 184px;
    /* Set inline from the video's own dimensions; this is the fallback for the
       moment before mpv has reported them. */
    aspect-ratio: 16 / 9;
    object-fit: cover;
    border-radius: 10px;
    border: 2px solid rgba(255, 255, 255, 0.85);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.45);
  }

  .hovertip .thumb.placeholder {
    background: rgba(16, 16, 22, 0.8);
  }

  /* The exact frame fetched when the cursor comes to rest is a different scene
     from the grid frame it replaces in roughly 40% of hovers, so it crosses
     over instead of cutting. This layer is the OUTGOING frame fading out — the
     one underneath is always the newest, which is what keeps stale content from
     showing through while the browser decodes it. Frames that merely track a
     moving cursor never get here and stay instant.
     Keep the duration under FADE_HOLD_MS in thumbs.svelte.ts, which drops this
     element once the animation has run. */
  .hovertip .thumb.fading {
    position: absolute;
    inset: 0;
    /* **Takes its size from the box it covers, never from a ratio of its own.**
       It inherits `.thumb`'s width and aspect-ratio, and the inline aspect that
       corrects those lives only on the in-flow frame — so this layer computed
       16:9 and, on anything wider (a 1920x816 rip is ordinary), came out 105px
       tall over an 80px box: it overflowed 25px downward onto the timestamp for
       the length of the crossfade and snapped back. Measured, and it explains
       why arriving from an un-downloaded position never blinked — there is no
       outgoing frame there, so this element is never created. */
    /* 100%, not `auto`: an <img> is a REPLACED element, so `width: auto` takes
       its intrinsic size rather than resolving from the insets — measured, that
       made the layer 484px tall. Percentages against the positioned wrapper are
       what actually make it cover exactly, and they only do so because boxes
       are border boxes: `.thumb`'s own 2px border used to be added on top of
       the 100% and left this 4px bigger, the same overflow in miniature. */
    width: 100%;
    height: 100%;
    aspect-ratio: auto;
    object-fit: cover;
    animation: thumb-cross 0.14s ease forwards;
  }

  /* The dimming and the spinner appear with a ~150 ms delay: on fast responses
     (a cache hit) the loader does not flash at all. */
  .thumbwrap.loading .thumb {
    transition: opacity 0.15s ease 0.15s;
    opacity: 0.45;
  }

  .time {
    color: #e0e0e6;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    min-width: 44px;
    text-align: center;
    text-shadow: var(--ui-shadow);
  }

  /* Not "the preview failed" but "this part has not arrived". The arrow is the
     one already used by the download readout, so the two say the same thing in
     the same shape. */
  .hovertip .thumb.pending {
    display: grid;
    place-items: center;
    color: #6f6f7a;
  }

  .hovertip .thumb.pending svg {
    width: 22px;
    height: 22px;
  }
  .seekrow.mini .time {
    font-size: 10.5px;
  }

  .seekrow.mini .seekwrap {
    margin: 2px 6px 0;
  }
  @keyframes thumb-cross {
    to {
      opacity: 0;
    }
  }
</style>
