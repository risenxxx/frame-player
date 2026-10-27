/**
 * The tooltips: one delegated mechanism for every `[data-tip]`, instead of the
 * native `title` (which neither engine lets us place or style). The listeners
 * are the window's; `Tooltip.svelte` only draws what is in `tips`.
 *
 * Out of the page's script, where the listeners were registered in `onMount`
 * and the state sat beside it. `initTooltips` is still called at that same
 * point of the start-up (see boot.ts).
 */

import { tick } from 'svelte';
import { flipAxis, shiftAxis } from './floating';

class Tips {
  el = $state<HTMLDivElement | undefined>();
  // Custom tooltips: one delegated mechanism for every [data-tip]
  tooltip = $state<{ text: string; pos: { x: number; y: number } | null } | null>(null);
}

export const tips = new Tips();

let tipTimer: ReturnType<typeof setTimeout> | undefined;

/// Takes a tooltip down at once — for a press anywhere, which comes before
/// the pointer has moved off the anchor.
export function hideTooltip() {
  clearTimeout(tipTimer);
  tips.tooltip = null;
}

/// Registers the window's listeners and returns their removal.
export function initTooltips(): () => void {
  // **A tooltip is cleared by the pointer not being over an anchor, never by
  // being told it left one.** `pointerout` is not delivered when the hovered
  // node is *removed* (measured, both engines), and the transition from the
  // start screen into playback removes anchors under a stationary cursor by
  // the handful — the torrent file picker being the one that gets noticed.
  // Relying on that event alone left a tooltip nothing could dismiss: moving
  // the mouse over the video hit the early return below, so only a click
  // (which clears it in the pointerdown capture handler) or landing on
  // another anchor would take it away.
  const onTipOver = (e: PointerEvent) => {
    const el = (e.target as HTMLElement | null)?.closest('[data-tip]') as HTMLElement | null;
    const text = el?.getAttribute('data-tip');
    if (!el || !text) {
      // The pointer is over something that has no tooltip, which is the
      // authoritative "nothing to show" — including the pointerover the
      // browser re-dispatches for whatever is revealed when an element under
      // the cursor disappears.
      clearTimeout(tipTimer);
      tips.tooltip = null;
      return;
    }
    clearTimeout(tipTimer);
    tipTimer = setTimeout(async () => {
      // The anchor may have been removed during the wait, and a detached
      // element's rect is **all zeros** (measured) — which `flipAxis` and
      // `shiftAxis` then faithfully place in the top-left corner of the
      // window, where it sat with no anchor left to send a pointerout.
      if (!el.isConnected) {
        tips.tooltip = null;
        return;
      }
      // Rendered unplaced first, then measured, then moved — all before
      // paint. Measuring has to happen at the window's origin, because a box
      // positioned with `left` has only the space to its right to lay out
      // in: a tip near the right edge wrapped to one word per line, and the
      // clamp that followed then read that wrapped width and called it a
      // fit. It is placed with a transform for the same reason — a transform
      // moves the box after layout instead of deciding it.
      tips.tooltip = { text, pos: null };
      await tick();
      // `tooltip` already null means something else cleared it — a click, or
      // the pointer leaving — and this pass has simply been overtaken.
      if (!tips.el || !tips.tooltip) return;
      // Checked again on this side of the await, because `tick()` is exactly
      // when a pending unmount flushes: the anchor can be connected when the
      // timer fires and gone one line later.
      if (!el.isConnected) {
        tips.tooltip = null;
        return;
      }
      const r = el.getBoundingClientRect();
      const w = tips.el.offsetWidth;
      const h = tips.el.offsetHeight;
      // Above by default, which is right for anything in a bar: the tip
      // appears over the picture rather than over the next control. An
      // element opts out with `data-tip-below` when the thing it names is
      // *above* it and would be covered — a recents card, where the tip
      // belongs to the poster it would otherwise hide. Only the preference
      // moves; flipAxis still puts the tip on whichever side has the room.
      const v = flipAxis({
        near: r.top,
        far: r.bottom,
        size: h,
        limit: window.innerHeight,
        gap: 7,
        preferBefore: !el.hasAttribute('data-tip-below'),
      });
      tips.tooltip = {
        text,
        pos: { x: shiftAxis(r.left + r.width / 2 - w / 2, w, window.innerWidth), y: v.pos },
      };
    }, 450);
  };
  const onTipOut = (e: PointerEvent) => {
    if (!(e.target as HTMLElement | null)?.closest('[data-tip]')) return;
    clearTimeout(tipTimer);
    tips.tooltip = null;
  };

  window.addEventListener('pointerover', onTipOver);
  window.addEventListener('pointerout', onTipOut);
  return () => {
    window.removeEventListener('pointerover', onTipOver);
    window.removeEventListener('pointerout', onTipOut);
  };
}
