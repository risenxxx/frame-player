<script lang="ts">
  /// While the TV plays, this window is a remote — the picture is over there,
  /// so the video area becomes a status card.
  ///
  /// Nearly opaque but not fully: mpv sits paused behind on the frame the cast
  /// started from, and letting it ghost through says "this is what is on the
  /// TV". It carries its own click handler because otherwise the gesture the
  /// player is built around — click to pause — simply stops existing the moment
  /// a cast starts.
  import { t } from '$lib/i18n.svelte';
  import { cast } from '$lib/cast.svelte';

  interface Props {
    stateLabel: string;
    /// What is playing, by the same name the title bar shows.
    title: string;
    onclick: () => void;
    /// A double click still means fullscreen, and it has to cancel the pending
    /// single click before it reaches the television.
    ondblclick: () => void;
  }

  let { stateLabel, title, onclick, ondblclick }: Props = $props();
</script>

<!-- The status card covers the video area, so without a handler of its own
     the gesture the player is built around — click to pause — simply stops
     existing the moment a cast starts. It routes to the same deferred path
     as the video underneath. -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="overlay castscreen"
  onclick={(e) => {
    if (e.target !== e.currentTarget && !(e.target as HTMLElement).closest('.castscreen-box'))
      return;
    onclick();
  }}
  ondblclick={() => {
    ondblclick();
  }}
>
  <div class="castscreen-box">
    <svg viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="M21 3H3c-1.1 0-2 .9-2 2v3h2V5h18v14h-7v2h7c1.1 0 2-.9 2-2V5c0-1.1-.9-2-2-2zM1 18v3h3c0-1.66-1.34-3-3-3zm0-4v2c2.76 0 5 2.24 5 5h2c0-3.87-3.13-7-7-7zm0-4v2c4.97 0 9 4.03 9 9h2c0-6.08-4.93-11-11-11z"/></svg>
    <span class="castscreen-title">{t('cast.casting_on', { name: cast.deviceName ?? '' })}</span>
    <span class="castscreen-sub">{title}</span>
    {#if cast.busy}
      <span class="castscreen-state">{stateLabel}</span>
    {/if}
  </div>
</div>

<style>
  .castscreen-box {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 24px;
    color: #e8e8ec;
    text-align: center;
  }

  .castscreen-box > svg {
    width: 56px;
    height: 56px;
    opacity: 0.85;
  }

  .castscreen-title {
    font-size: 15px;
    font-weight: 600;
  }

  .castscreen-sub {
    font-size: 13px;
    color: rgba(232, 232, 236, 0.6);
    max-width: min(440px, 80vw);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .castscreen-state {
    font-size: 12px;
    color: rgba(232, 232, 236, 0.5);
  }
  /* Nearly opaque, deliberately not fully: mpv sits paused behind on the frame
     the cast started from, and letting it ghost through says "this is what is
     on the TV" — while the alpha is high enough that even if nothing painted
     there would be a dark field, not the desktop (gotcha 10: mpv always paints
     its field once the VO is configured, so this is belt and braces). */
  .overlay.castscreen {
    background: rgba(11, 11, 16, 0.88);
  }
</style>
