<script lang="ts">
  /// Shown while a network source is resolving and buffering — and while the
  /// room's own content is being opened, which is the longest of those waits
  /// and the one that used to show nothing at all.
  ///
  /// It reports a real figure rather than a guess: mpv's `cache-buffering-state`
  /// is the cache fill and is *absent* while there is no cache yet, which means
  /// "still resolving" and not "0 %". Where there is no figure to report at all
  /// — a magnet resolve — it reports the one thing it does know, which is how
  /// long it has been going.
  import { formatTime } from '$lib/format';

  interface Props {
    /// The sentence under the title — what is being waited on right now.
    label: string;
    /// Why it is waiting: a torrent's peers and rate, or — while the room's own
    /// content is being opened — what this player is opening and why.
    sub: string | null;
    /// When the wait began, for a wait with nothing to count but seconds. Zero
    /// for one that reports a figure of its own, where a clock adds nothing.
    since?: number;
  }

  let { label, sub, since = 0 }: Props = $props();

  /// Seconds on screen. A timer rather than something derived, because while a
  /// magnet resolves **nothing else on this box changes** — no percentage, no
  /// peer count, no rate — and a plate that never moves for ninety seconds is
  /// exactly what reads as a player that has hung rather than one still looking.
  let elapsed = $state(0);

  // Writes `elapsed` and never reads it: an effect that read its own state back
  // would re-register itself on every tick (the ScrollFade lesson), and here it
  // would also restart the interval a second at a time.
  $effect(() => {
    if (!since) {
      elapsed = 0;
      return;
    }
    const tick = () => {
      elapsed = Math.max(0, Math.round((Date.now() - since) / 1000));
    };
    tick();
    const timer = setInterval(tick, 1000);
    return () => clearInterval(timer);
  });
</script>

<div class="overlay loading-overlay">
  <div class="loading-box">
    <span class="loading-spin"></span>
    <!-- The two lines are ONE flex item, not two siblings of the spinner.
         That is what makes them share a left edge structurally instead of
         by an offset someone has to keep correct. -->
    <span class="loading-text">
      <!-- The clock rides in the title rather than under it: it is not a
           second fact about the wait, it is how long *this* wait has been
           going on, and on its own line it would read as a duration of
           something. -->
      <span class="loading-title"
        >{label}{#if since}<span class="loading-clock">{formatTime(elapsed)}</span>{/if}</span
      >
      <!-- Why it is waiting, when the answer is a swarm. Without it a
           torrent stall is indistinguishable from a hung player. -->
      {#if sub}
        <span class="loading-sub">{sub}</span>
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

  /* Dimmer than the sentence it follows, because it is a measurement rather
     than part of it. The gap is a margin and not a space in the markup: the two
     are one line of text, and this way the distance is a chosen 7px rather than
     whatever the font's word space happens to be. */
  .loading-clock {
    margin-left: 7px;
    color: #9a9aa6;
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
