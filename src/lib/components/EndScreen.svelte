<script lang="ts">
  /// Previous / replay / next, with a cancellable countdown on next.
  ///
  /// `keep-open=always` parks mpv on the last frame of *every* entry rather than
  /// only the last one, so this is the end of an episode as much as the end of a
  /// film — advancing is the player's job, not mpv's. Clicking the backdrop
  /// cancels the countdown: it is the largest possible target for "wait".
  import type { Snippet } from 'svelte';
  import { t } from '$lib/i18n.svelte';
  import { isNetworkSource } from '$lib/player.svelte';
  import { ADVANCE_MS, playEntry, playlist, type PlaylistEntry } from '$lib/playlist.svelte';

  interface Props {
    prev: PlaylistEntry | null;
    next: PlaylistEntry | null;
    /// The countdown to the next entry is running.
    counting: boolean;
    /// Bumped each time a countdown starts. It keys the progress sweep so a
    /// fresh countdown replays the animation instead of inheriting a finished
    /// one.
    seq: number;
    oncancel: () => void;
    onreplay: () => void;
  }

  let { prev, next, counting, seq, oncancel, onreplay }: Props = $props();
</script>

<!-- keep-open=always parks mpv on the last frame of EVERY entry, not only
     the last one, so this is the end of an episode as much as the end of a
     film: previous / replay / next, with the countdown on next. Clicking
     the backdrop cancels the countdown — it is the largest possible target
     for "wait". -->
<div class="overlay clickthrough-bg endscreen" role="presentation" onclick={oncancel}>
  <div class="endrow">
    {#if prev}
      {@render endCard(prev, 'prev')}
    {/if}
    <button class="replay" onclick={onreplay} aria-label={t('osc.replay')}>
      <svg viewBox="0 0 24 24" width="48" height="48"><path fill="currentColor" d="M12 5V1L7 6l5 5V7a6 6 0 1 1-6 6H4a8 8 0 1 0 8-8z"/></svg>
    </button>
    {#if next}
      {@render endCard(next, 'next')}
    {/if}
  </div>
  {#if counting}
    <button class="endcancel" onclick={oncancel}>{t('end.cancel')}</button>
  {/if}
</div>

  {#snippet endCard(entry: PlaylistEntry, side: 'prev' | 'next')}
<button
  class="endcard"
  class:prev={side === 'prev'}
  onclick={(e) => {
    e.stopPropagation();
    oncancel();
    void playEntry(entry);
  }}
>
  <span class="card-poster" class:empty={!playlist.posters[entry.path]}>
    {#if playlist.posters[entry.path]}
      <img src={playlist.posters[entry.path]} alt="" />
    {:else if isNetworkSource(entry.path)}
      <!-- No frame is coming: a poster is decoded from the file, and this
           entry is a URL. An empty outlined rectangle reads as a preview
           that failed, so the card says what it is instead. -->
      <span class="card-link">
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
            <path d="M10 13.5a4.6 4.6 0 0 0 6.9.5l2.7-2.7a4.6 4.6 0 0 0-6.5-6.5L11.5 6.4"/>
            <path d="M14 10.5a4.6 4.6 0 0 0-6.9-.5l-2.7 2.7a4.6 4.6 0 0 0 6.5 6.5l1.6-1.6"/>
          </g>
        </svg>
      </span>
    {/if}
    <!-- The countdown, keyed so a restarted one gets a fresh element and
         therefore replays its animation instead of inheriting a finished
         one. Indigo like .card-progress, and for the same reason: it lies
         over an arbitrary video frame, where white disappears. -->
    {#if side === 'next' && counting}
      {#key seq}
        <span class="endprogress" style="--advance: {ADVANCE_MS}ms"></span>
      {/key}
    {/if}
  </span>
  <span class="card-left">{t(side === 'next' ? 'end.next' : 'end.prev')}</span>
  <span class="card-name">{entry.title}</span>
</button>
  {/snippet}

<style>
  .clickthrough-bg {
    background: rgba(0, 0, 0, 0.35);
  }

  button.replay {
    background: rgba(20, 20, 28, 0.75);
    border: none;
    border-radius: 50%;
    width: 96px;
    height: 96px;
    display: grid;
    place-items: center;
    color: #e8e8ec;
    cursor: pointer;
  }

  button.replay:hover {
    background: rgba(40, 40, 54, 0.85);
  }

  /* The start screen's half of this pair moved to StartScreen.svelte with its
     markup; `.endcard` is still drawn here. */
  .endcard:hover .card-poster {
    outline: 2px solid #818cf8;
    outline-offset: 1px;
  }

  /* End of an entry: previous / replay / next. The cards are a fixed width, so
     the replay button stays put whichever of them is missing — a queue has a
     first and a last entry, and the middle control jumping sideways between
     episodes would be the most visible thing on the screen. */
  .endscreen {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 20px;
  }

  .endrow {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 32px;
  }

  .endcard {
    width: 220px;
    display: flex;
    flex-direction: column;
    padding: 0;
    background: none;
    border: none;
    text-align: left;
    color: inherit;
    cursor: pointer;
  }

  /* The pair reads outwards from the replay button: "next" under the card on
     the left, "previous" under the card on the right, each aligned to the edge
     it sits against. Both labels were left-aligned before, which put them at
     the same side of the screen and made the row look like one thing pushed
     off-centre rather than two choices either side of a middle. */
  .endcard.prev {
    text-align: right;
  }

  /* These sit directly on the final frame of the film — a 0.35 scrim is not a
     background — so both lines carry the shadow every other caption over the
     video does. And the label is the start screen's grey lifted to something
     that survives a bright frame: at #8f8f9c on a snow scene it was invisible,
     which is exactly where the viewer is looking for "what is next". A rule
     that has to beat `.card-name`/`.card-left` is written to win rather than
     left to source order (see the queue rows and the torrent list). */
  .endcard .card-name {
    margin-top: 2px;
    color: #f2f2f6;
    text-shadow: var(--ui-shadow);
  }

  /* The label is the first line under the poster here, where on the start
     screen that is `.card-name` — which carries the 6px for it. `.card-left`
     has none of its own, because there it follows the name rather than the
     picture, so without this the caption sat flush against the frame. Same 6px
     as the start card, so the two read as one object seen twice. */
  .endcard .card-left {
    margin-top: 6px;
    color: #d2d2dc;
    text-shadow: var(--ui-shadow);
  }

  /* The countdown drains rather than fills: what it reports is the time left,
     and it lies over an arbitrary video frame, which is why it is indigo like
     .card-progress and not white like every other readout. */
  .endprogress {
    position: absolute;
    left: 0;
    bottom: 0;
    height: 3px;
    background: #6366f1;
    animation: end-advance var(--advance) linear forwards;
  }

  .endcancel {
    padding: 8px 18px;
    border: 1px solid rgba(255, 255, 255, 0.14);
    border-radius: 999px;
    background: rgba(16, 16, 22, 0.92);
    color: #e8e8ec;
    font-size: 13px;
    cursor: pointer;
  }

  .endcancel:hover {
    background: rgba(32, 32, 42, 0.94);
    border-color: rgba(255, 255, 255, 0.24);
  }
  @keyframes end-advance {
    from {
      width: 100%;
    }
    to {
      width: 0;
    }
  }
</style>
