<script lang="ts">
  /// The play queue: the chapter list's markup plus a remove button and
  /// drag-to-reorder.
  ///
  /// The reorder lives here because nothing outside needs it — a grab offset,
  /// a hovered index and the flag that tells a drag's trailing click from a
  /// real one are as local as state gets.
  import { tick } from 'svelte';
  import { command } from 'tauri-plugin-libmpv-api';

  import { t } from '$lib/i18n.svelte';
  import { openEntry } from '$lib/playback.svelte';
  import { player } from '$lib/player.svelte';
  import { loadPlaylist, playlist } from '$lib/playlist.svelte';

  interface Props {
    close: () => void;
    onRemove: (index: number) => void;
  }

  let { close, onRemove }: Props = $props();

  let el = $state<HTMLDivElement | undefined>();

  /// A film has dozens of entries and the list opens scrolled to the top, so
  /// the one playing is usually out of sight — the only row the viewer has a
  /// reference point for. `nearest`, so a row already visible scrolls nothing.
  $effect(() => {
    void tick().then(() =>
      el?.querySelector('.menu-item.sel')?.scrollIntoView({ block: 'nearest' }),
    );
  });

const DRAG_SLOP = 4;

let dragFrom = $state<number | null>(null);
/// Where it would land if released now. Same number as `dragFrom` until the
/// pointer crosses a neighbouring row.
let dragTo = $state<number | null>(null);
let dragStartY = 0;
let dragRowHeight = 0;
/// Read from the markup, so it has to be state — the row's lifted styling
/// and the shift of its neighbours both key off it.
let dragArmed = $state(false);

function onQueueDown(event: PointerEvent, index: number) {
  // The × is a control of its own; a press there is never a drag.
  if ((event.target as HTMLElement).closest('.queue-remove')) return;
  const row = (event.currentTarget as HTMLElement).closest('.queue-row');
  if (!row) return;
  dragStartY = event.clientY;
  dragRowHeight = row.getBoundingClientRect().height || 1;
  dragFrom = index;
  dragTo = index;
  dragArmed = false;
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
}

function onQueueMove(event: PointerEvent) {
  if (dragFrom === null) return;
  const traveled = event.clientY - dragStartY;
  if (!dragArmed && Math.abs(traveled) < DRAG_SLOP) return;
  dragArmed = true;
  const steps = Math.round(traveled / dragRowHeight);
  const last = playlist.entries.length - 1;
  dragTo = Math.min(last, Math.max(0, dragFrom + steps));
}

async function onQueueUp(event: PointerEvent) {
  const from = dragFrom;
  const to = dragTo;
  const wasDrag = dragArmed;
  dragFrom = null;
  dragTo = null;
  dragArmed = false;
  try {
    (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
  } catch {
    // The capture is gone already — nothing to release.
  }
  // A press that never traveled is a click, and the click handler on the row
  // does the rest.
  if (!wasDrag || from === null || to === null || from === to) return;
  // mpv moves the entry so that it *takes the place of* index2, computed on
  // the list as it stands. Moving down therefore needs one more: in
  // [A,B,C,D], putting A at index 2 means `playlist-move 0 3` — with 2 it
  // would land before C, at index 1.
  const target = to > from ? to + 1 : to;
  await command('playlist-move', [String(from), String(target)]).catch((e: unknown) => {
    console.warn('playlist-move failed:', e);
  });
  await loadPlaylist();
}

/// How far a row is displaced while a drag is in flight. The dragged row
/// follows the pointer in whole rows; the ones it passes step aside by one.
function queueRowShift(index: number): number {
  if (dragFrom === null || dragTo === null || !dragArmed) return 0;
  if (index === dragFrom) return (dragTo - dragFrom) * dragRowHeight;
  if (dragFrom < dragTo && index > dragFrom && index <= dragTo) return -dragRowHeight;
  if (dragFrom > dragTo && index < dragFrom && index >= dragTo) return dragRowHeight;
  return 0;
}

/// Same reasoning as the chapter list: a season has dozens of entries and the
/// one playing is the only row the viewer has a reference point for.
</script>

<div class="menu chapters queue scrollable" bind:this={el}>
  <div class="menu-title">{t('osc.queue')}</div>
  {#each playlist.entries as entry (entry.index)}
    <!-- A row and its remove button, not a button inside a button: nested
         interactive elements are invalid and the inner one stops being
         reachable. Same shape as the recents card and its × on the start
         screen. -->
    <div
      class="queue-row"
      class:dragging={dragArmed && dragFrom === entry.index}
      style="transform: translateY({queueRowShift(entry.index)}px)"
    >
      <button
        class="menu-item chapter-item"
        class:sel={entry.index === player.playlistPos}
        data-tip={entry.title}
        onpointerdown={(e) => onQueueDown(e, entry.index)}
        onpointermove={onQueueMove}
        onpointerup={(e) => void onQueueUp(e)}
        onpointercancel={(e) => void onQueueUp(e)}
        onclick={() => {
          // A drag ends with a click on the same element; the guard is
          // the flag the pointer handlers leave behind.
          if (dragArmed) return;
          close();
          openEntry(entry);
        }}
      >
        <span class="chapter-name">{entry.title}</span>
      </button>
      <button
        class="queue-remove"
        data-tip={t('osc.queue_remove')}
        aria-label={t('osc.queue_remove')}
        onclick={() => onRemove(entry.index)}
      >
        <svg viewBox="0 0 10 10" aria-hidden="true">
          <path stroke="currentColor" stroke-width="1.4" stroke-linecap="round" d="M1.2 1.2l7.6 7.6M8.8 1.2l-7.6 7.6"/>
        </svg>
      </button>
    </div>
  {/each}
</div>

<style>
  /* `.queue-row` and `.queue-remove` live in app.css: the subtitle list in the
     track menu is a sibling component using the same markup, and a scoped copy
     here left its rows unstyled. Only the drag, which is this menu's alone,
     stays. */
  .queue-row.dragging {
    position: relative;
    z-index: 1;
    transition: none;
    background: rgba(255, 255, 255, 0.06);
    border-radius: 6px;
  }

  /* Wider than the chapter list it shares its markup with: those are sentences,
     these are release names, and the panel grows with them until the window
     says no. */
  .menu.queue {
    max-width: min(520px, calc(100vw - 48px));
  }
</style>
