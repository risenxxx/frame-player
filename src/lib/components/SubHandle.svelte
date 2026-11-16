<script lang="ts">
  /// The invisible box a subtitle line is dragged by. mpv draws the lines; this
  /// is laid over where a script reports one is (sublines.lua), and turns the
  /// pointer into a hover and a drag for the line's own component to apply —
  /// `SecondSubLine` moves its line anywhere, `MainSubLine` moves mpv's up and
  /// down.
  import type { Box } from '$lib/second-line';

  let {
    box,
    guide = false,
    onhover,
    onstart,
    onmove,
    onend,
  }: {
    /// Where the line is, as fractions of the window.
    box: Box;
    /// Draw the window's middle under the line (a snap is holding it there).
    guide?: boolean;
    /// The pointer came onto the line, or left it — a drag in progress counts
    /// as being on it until it ends.
    onhover: (over: boolean) => void;
    /// A drag begins.
    onstart: () => void;
    /// The pointer has moved this far, in CSS pixels, since the press.
    onmove: (dx: number, dy: number) => void;
    /// The drag is over; `moved` is false for a press that never moved.
    onend: (moved: boolean) => void;
  } = $props();

  let drag = $state<{ id: number; x: number; y: number; moved: boolean } | null>(null);

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    // The picture's own handler leaves a press that is not on the picture
    // alone (no window drag, no pan), so this only keeps the press from
    // selecting anything. It still bubbles: a press here closes an open menu
    // like a press anywhere else.
    e.preventDefault();
    try {
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    } catch {
      // A pointer the browser no longer knows: the drag still follows the
      // moves that reach the handle.
    }
    drag = { id: e.pointerId, x: e.clientX, y: e.clientY, moved: false };
    onstart();
  }

  function move(e: PointerEvent) {
    if (!drag || e.pointerId !== drag.id) return;
    const dx = e.clientX - drag.x;
    const dy = e.clientY - drag.y;
    // A press that has not moved is not a drag yet — it stays a click.
    if (!drag.moved && dx * dx + dy * dy < 9) return;
    drag.moved = true;
    onmove(dx, dy);
  }

  function up(e: PointerEvent) {
    if (!drag || e.pointerId !== drag.id) return;
    const moved = drag.moved;
    drag = null;
    onend(moved);
    // Released outside the line: nothing is over it any more.
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const inside = e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top && e.clientY <= r.bottom;
    if (!inside) onhover(false);
  }
</script>

<div
  class="handle afloat"
  class:dragging={drag !== null}
  style:left="{box.x0 * 100}%"
  style:top="{box.y0 * 100}%"
  style:width="{(box.x1 - box.x0) * 100}%"
  style:height="{(box.y1 - box.y0) * 100}%"
  onpointerenter={() => onhover(true)}
  onpointerleave={() => {
    if (!drag) onhover(false);
  }}
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={up}
  role="presentation"
></div>
{#if guide}
  <!-- The middle, while the line's centre is held on it. -->
  <div
    class="guide"
    style:top="calc({box.y0 * 100}% - 12px)"
    style:height="calc({(box.y1 - box.y0) * 100}% + 24px)"
    aria-hidden="true"
  ></div>
{/if}

<style>
  /* Over the picture and under every surface, like the line it stands for: the
     bars, the menus and the dialogs all cover it. Nothing is painted — the
     line is mpv's — only a few pixels of slack round it to grab by. */
  .handle {
    position: absolute;
    margin: -6px;
    padding: 6px;
    box-sizing: content-box;
    cursor: grab;
    touch-action: none;
  }

  .handle.dragging {
    cursor: grabbing;
  }

  /* The idle rule hides the cursor everywhere; a handle must not bring it back. */
  :global(.player.nocursor) .handle:not(.dragging) {
    cursor: none;
  }

  .guide {
    position: absolute;
    left: 50%;
    width: 1px;
    margin-left: -0.5px;
    background: rgba(129, 140, 248, 0.8);
    pointer-events: none;
  }
</style>
