<script lang="ts">
  /// "Skip intro" / "skip credits" / "next episode".
  ///
  /// Deliberately outside `.osc`: it has to stay up while the UI is idle, which
  /// is the entire point of it — and idle is precisely when it is wanted. That
  /// costs it a `z-index`, since `.osc` comes later in DOM order and its scrim
  /// reaches 44px above the controls.
  ///
  /// Everything it shows about time comes from the position, never a CSS clock:
  /// the draining wash is `background-size` driven by `--skip-left` and the tail
  /// fade is an inline opacity, so it survives a seek into the middle of an
  /// opening and a paused film freezes the countdown with nobody arranging it.
  import { t } from '$lib/i18n.svelte';
  import type { PlaylistEntry } from '$lib/playlist.svelte';

  /// Which of the five announcement chapters this is. The labels live here
  /// because nothing else names them.
  const SKIP_LABEL = {
    intro: 'skip.intro',
    recap: 'skip.recap',
    preview: 'skip.preview',
    credits: 'skip.credits',
    ad: 'skip.ad',
  } as const;

  interface Props {
    /// `left` drives the draining wash and `fade` the tail, both computed from
    /// the position rather than from a CSS clock — see the note above.
    hint: {
      kind: keyof typeof SKIP_LABEL;
      left: number;
      fade: number;
      /// Set on the last chapter, where the button leaves the file entirely.
      entry: PlaylistEntry | null;
    };
    mini: boolean;
    onskip: () => void;
  }

  let { hint, mini, onskip }: Props = $props();
</script>

<button
  class="skipbtn" class:mini
  style="--skip-left: {hint.left}; opacity: {hint.fade}"
  onclick={(e) => {
    e.stopPropagation();
    onskip();
  }}
  ondblclick={(e) => e.stopPropagation()}
>
  <!-- On the last chapter the button leaves the file entirely, and saying
       "skip credits" for something that starts the next episode would be a
       lie about where the click leads. -->
  <span class="skip-label">
    {t(hint.entry ? 'skip.next_episode' : SKIP_LABEL[hint.kind])}
  </span>
  <!-- Filled, like every other glyph in this player. The stroked chevrons
       that were here read as a web-page affordance among a set of solid
       Material-shaped icons. Not the OSC's next-file glyph (▶|) either:
       that one already means "next entry in the playlist". -->
  <svg viewBox="0 0 24 24" aria-hidden="true">
    <path fill="currentColor" d="M4 18l8.5-6L4 6v12zm9-12v12l8.5-6L13 6z" />
  </svg>
</button>

<style>
  /* Sits where the track menus do (same right edge, just above the bar), and is
     hidden whenever one of them is open rather than fighting it for the space.
     A pill like .updbtn, which is the same species of control: a suggested
     action that turns up when it is relevant and leaves on its own. The surface
     is the shared floating-surface value, not an indigo fill — over arbitrary
     video the accent shouts, and it means "on/selected" everywhere else. */
  .skipbtn {
    position: absolute;
    right: 24px;
    bottom: 108px;
    /* Above the bar, below everything in the popup scale (.osd is 30). The OSC
       is the reason this is needed at all: its scrim reaches 44px above the
       controls, and .osc comes later in DOM order with a stacking context of
       its own — so without a z-index the gradient washes over the top half of
       the button, tinting it unevenly against its own shadow. */
    z-index: 20;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 14px 22px;
    border: 1px solid rgba(255, 255, 255, 0.14);
    border-radius: 999px;
    /* The remaining time is the background IMAGE, drained by animating
       background-size. It was a scaled pseudo-element, and a transform distorts
       what it scales: the wash's own corners flattened as it narrowed, and its
       increasingly square left edge crept out from under the pill's rounded
       one. A background is clipped by the border-radius by definition, so there
       is nothing left to distort or to escape — and it removes the ::before,
       and with it the z-index/isolation pair that gotcha 9 forces on every
       absolutely positioned pseudo-element in this file.
       White rather than indigo: this reports a value, it does not offer a
       choice — the rule that keeps .seekfill white. */
    background-color: rgba(16, 16, 22, 0.92);
    background-image: linear-gradient(rgba(255, 255, 255, 0.1), rgba(255, 255, 255, 0.1));
    background-repeat: no-repeat;
    background-position: left center;
    background-size: calc(var(--skip-left, 1) * 100%) 100%;
    /* The width comes from the playback position, which arrives a handful of
       times a second — the transition is what turns those steps into a glide.
       It also means the bar cannot lie after a seek, and that a paused film
       freezes it without anyone arranging for that. */
    transition:
      background-size 0.3s linear,
      opacity 0.3s ease;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.45);
    color: #e8e8ec;
    font-size: 15px;
    letter-spacing: 0.01em;
    cursor: pointer;
    /* Entrance only, and a fixed duration: the button's lifetime is now the
       chapter's, so anything keyed to percentages of it would stretch a 0.2 s
       fade into three seconds on a minute-long opening. The exit is the fade
       carried by the inline opacity, which is driven by the same clock as the
       bar. */
    animation: skip-in 0.18s ease;
  }

  /* Only the color: `background` shorthand here would drop the drain gradient. */
  .skipbtn:hover {
    background-color: rgba(32, 32, 42, 0.94);
    border-color: rgba(255, 255, 255, 0.24);
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.5);
  }

  /* The label never wraps: the button is anchored to the right edge, and a
     second line would push the chevron off its baseline. */
  .skip-label {
    white-space: nowrap;
  }

  .skipbtn svg {
    width: 17px;
    height: 17px;
    opacity: 0.75;
  }

  .skipbtn:hover svg {
    opacity: 1;
  }

  .skipbtn.mini {
    right: 12px;
    /* Clears the shortened bar, which is ~77px tall: 6px of padding, a 34px
       play row with its 4px margin, and a seek row of a 14px timecode line
       above a ~19px track. At 72 the pill sat on the timecodes. */
    bottom: 88px;
    gap: 8px;
    padding: 9px 16px;
    font-size: 13px;
  }

  .skipbtn.mini svg {
    width: 15px;
    height: 15px;
  }
  @keyframes skip-in {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
  }
</style>
