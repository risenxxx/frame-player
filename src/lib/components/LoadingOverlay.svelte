<script lang="ts">
  /// Shown while a network source is resolving and buffering.
  ///
  /// It reports a real figure rather than a guess: mpv's `cache-buffering-state`
  /// is the cache fill and is *absent* while there is no cache yet, which means
  /// "still resolving" and not "0 %".
  import { t } from '$lib/i18n.svelte';

  interface Props {
    /// The sentence under the title — what is being waited on right now.
    label: string;
    /// A torrent's peers and rate, when that is what is being opened.
    torrentLabel: string | null;
  }

  let { label, torrentLabel }: Props = $props();
</script>

<div class="overlay loading-overlay">
  <div class="loading-box">
    <span class="loading-spin"></span>
    <!-- The two lines are ONE flex item, not two siblings of the spinner.
         That is what makes them share a left edge structurally instead of
         by an offset someone has to keep correct. -->
    <span class="loading-text">
      <span class="loading-title">{label}</span>
      <!-- Why it is waiting, when the answer is a swarm. Without it a
           torrent stall is indistinguishable from a hung player. -->
      {#if torrentLabel}
        <span class="loading-sub">{torrentLabel}</span>
      {/if}
    </span>
  </div>
</div>

<style>
  /* Over the black field mpv shows before its first frame. No backdrop of its
     own: there is nothing underneath to dim. */
  .loading-overlay {
    pointer-events: none;
  }

  /* The shared floating-surface fill comes with a hairline and a shadow, and
     both halves are load-bearing: this fill is the start screen's own color,
     so without the border the plate is invisible exactly when a link has just
     been pasted — which is the moment it exists for. The shadow does the same
     job over video, where the border alone would be lost against a bright
     frame. */
  /* Icon column, text column — the layout every notification with a subtitle
     uses, and the reason is not taste. The first version wrapped a second line
     onto a new flex row and re-derived its left offset by hand; measured, that
     put the two lines 4px apart horizontally and 16px apart vertically, because
     `flex-wrap` makes `gap` a ROW gap as well and because the spinner is not the
     width it is declared to be (see .loading-spin). With the text as one item
     there is no offset left to compute, and nothing to keep in sync when the
     spinner or the gap changes. */
  .loading-box {
    display: flex;
    align-items: center;
    gap: 13px;
    padding: 13px 17px;
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 12px;
    background: rgba(16, 16, 22, 0.92);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.45);
    color: #e8e8ec;
    font-size: 13px;
    /* The figures refresh every second — without this the box twitches as the
       digits change width. */
    font-variant-numeric: tabular-nums;
  }

  .loading-text {
    display: flex;
    flex-direction: column;
    /* Title to subtitle, not paragraph spacing: they are one statement. */
    gap: 2px;
    min-width: 0;
    /* A librqbit error can be a sentence. Wrapping it inside the column keeps
       the wrapped lines aligned under the first, where a wrap at the box level
       would not be. */
    max-width: min(420px, calc(100vw - 96px));
  }

  .loading-title {
    line-height: 1.3;
  }

  /* The state is the title ("waiting for data"); this is the evidence for it.
     Below rather than beside, because on one line the two read as a single
     sentence and stop being a state and its reason. */
  .loading-sub {
    color: #9a9aa6;
    font-size: 12px;
    line-height: 1.35;
  }
</style>
