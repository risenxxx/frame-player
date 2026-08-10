<script lang="ts">
  /// The transient popup: icon and value, never a progress bar of its own
  /// beyond the thin one a volume or speed change carries.
  import { t } from '$lib/i18n.svelte';

  interface Props {
    state: { text: string; sub?: string; progress?: number };
    /// Sized for a full window; in a 420px one it would be a third of the
    /// frame, so it scales and moves up — there is no title bar to clear there.
    mini: boolean;
  }

  let { state, mini }: Props = $props();
</script>

<div class="osd" class:mini class:with-bar={state.progress !== undefined}>
  <span class="osd-text">{state.text}</span>
  {#if state.progress !== undefined}
    <div class="osd-bar"><div class="osd-bar-fill" style="width: {state.progress * 100}%"></div></div>
  {/if}
  {#if state.sub}
    <span class="osd-sub">{state.sub}</span>
  {/if}
</div>

<style>
  /* Frosted panel, IINA style: no border, soft radius, deep shadow. A real
     backdrop-filter is impossible here — the video is NOT in the DOM but a
     native layer under the transparent webview, so the web content has nothing
     to blur. The "frost" is therefore made of background density and shadow. */
  /* IINA-style panel: top left under the title bar, no border, dense backing.
     A real backdrop-filter is impossible here — the video is NOT in the DOM but
     a native layer under the transparent webview, so the web content has
     nothing to blur. The "frost" is made of background density and shadow. */
  .osd {
    position: absolute;
    top: 60px;
    left: 18px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    /* Outer: 168 of content plus 32 of padding and 2 of border. Wide enough
       that "70%" and "1.25×" raise a popup of the same size. */
    min-width: 202px;
    /* The same surface as the context menu, settings and tooltips — floating
       layers should come from one scale, not one each. */
    background: rgba(16, 16, 22, 0.96);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 12px;
    padding: 12px 16px;
    pointer-events: none;
    box-shadow:
      0 10px 34px rgba(0, 0, 0, 0.5),
      inset 0 1px 0 rgba(255, 255, 255, 0.06);
    animation: osd-in 0.15s ease;
    /* **Above the dialogs (70), below the tooltip (90) and the veil (100).**
       A popup reports the outcome of something the viewer just did, and the
       thing they just did is very often a button inside a dialog — clearing the
       torrent cache, restarting seeding, switching port forwarding, updating a
       torrent. At 30 every one of those landed *under* the backdrop, which is
       `rgba(0, 0, 0, 0.45)` rather than opaque, so the popup did not vanish: it
       showed through at 55 % over a dimmed screen, reading as something that had
       escaped to the background rather than as an answer. The layers it stays
       below are the two that must cover everything: a tooltip is anchored to
       whatever the pointer is on and is the more local answer, and the veil
       exists to mask the fullscreen transition.

       This is safe only because the popup takes no clicks (`pointer-events:
       none` above) — it is the one floating surface in the player that is
       purely a readout, so nothing behind it becomes unreachable. What it does
       cost is paint: the sheet is 598px wide and centred, so under about
       1040px of window width it reaches left of the popup's own 18 + 202, and
       a sheet tall enough to start at the backdrop's 56px top then loses its
       corner for the 1.2s the popup is up. That is the right side of the
       trade — a notification that covers a corner is a notification you can
       read. */
    z-index: 80;
  }

  /* The bar must not press against the bottom edge */
  .osd.with-bar {
    padding-bottom: 16px;
  }

  .osd-bar {
    height: 4px;
    border-radius: 2px;
    background: rgba(255, 255, 255, 0.18);
    overflow: hidden;
  }

  /* White, like the seekbar fill. Indigo here would mean "selected / on /
     primary action" — that is all it means everywhere else in this UI — while
     the OSD bar selects nothing and is not interactive at all: it merely
     reports a value. It came out inverted: the thing you drag (the seekbar) was
     white, and a passive readout wore the accent. */
  .osd-bar-fill {
    height: 100%;
    border-radius: 2px;
    background: #fff;
  }

  /* Second line: for "resuming" this is the position, smaller and dimmer */
  .osd-sub {
    color: #b9b9c3;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .osd-text {
    color: #e8e8ec;
    font-size: 15px;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  /* Everything that floats over the picture is sized for a full window, and in
     a 420px one that is a third of the frame. Scaled to the window rather than
     shrunk to nothing — these are read at arm's length, which is the whole
     point of a corner window. The popup also moves up: `top: 60px` was there
     to clear the title bar, and in mini there is no title bar. */
  .osd.mini {
    top: 12px;
    left: 12px;
    min-width: 0;
    gap: 6px;
    /* Concentric with the window's own corner: the inner radius should be the
       outer one minus the gap, and the outer one is not knowable — macOS picks
       it, Windows 11 picks another, and macOS 26 rounds windows considerably
       more than macOS 11 did. So it errs *tight* on purpose: an inner corner
       slightly squarer than the rule wants passes unnoticed, while one rounder
       than the curve behind it reads as a mistake. 8 + the 12px gap assumes an
       outer radius near 20; drop it further if the window ever looks rounder
       than the popup inside it. */
    border-radius: 8px;
    padding: 8px 12px;
  }

  .osd.mini .osd-text {
    font-size: 13px;
  }

  .osd.mini .osd-sub {
    font-size: 11px;
  }
  @keyframes osd-in {
    from {
      opacity: 0;
      transform: translateX(-50%) scale(0.95);
    }
  }
</style>
