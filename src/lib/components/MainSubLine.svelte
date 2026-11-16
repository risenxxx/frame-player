<script lang="ts">
  /// The handle the main subtitle line is dragged up and down by. mpv draws the
  /// line, and keeps drawing it — frame export keeps it, and ASS and image
  /// subtitles have nothing else to be drawn by — so only `sub-pos` moves, the
  /// lever the settings slider and the bar's lift already use (main-line.ts).
  /// Only for a plain-text track: sublines.lua can say where such a line is,
  /// and an ASS line's place is the file's own.
  import { chrome } from '$lib/chrome.svelte';
  import { dragSubPos, parseMainLine, type MainLine } from '$lib/main-line';
  import { playback } from '$lib/playback.svelte';
  import { player } from '$lib/player.svelte';
  import { holdSubtitles, keepSubtitlesPos, moveSubtitles } from '$lib/sub-shift.svelte';
  import SubHandle from './SubHandle.svelte';

  const drawn = $derived(parseMainLine(player.mainBox));
  let hovered = $state(false);
  let drag = $state<MainLine | null>(null);
  /// The last `sub-pos` the drag wrote.
  let last: number | null = null;

  /// Where the handle goes: the line as drawn, or — dragged across the gap
  /// between two cues — where it was when the drag began.
  const box = $derived(drawn?.box ?? drag?.box ?? null);
  /// Only while the bars are up — the pointer is moving — or while in use, as
  /// for the second line: an idle film relays out nothing at every cue. Not
  /// while a television plays: mpv's line is a frozen one then.
  const handle = $derived(
    drag !== null || (!playback.remote && drawn !== null && (!chrome.idle || hovered)),
  );

  function hover(over: boolean) {
    if (over === hovered) return;
    hovered = over;
    holdSubtitles(over);
  }

  // A cue that ends under a resting pointer takes the handle away without a
  // `pointerleave`; nothing is over the line any more.
  $effect(() => {
    if (!handle && hovered) hover(false);
  });

  $effect(() => {
    return () => {
      if (hovered) holdSubtitles(false);
    };
  });

  function start() {
    if (drawn) drag = drawn;
    last = null;
  }

  function move(_dx: number, dy: number) {
    if (!drag) return;
    const pos = dragSubPos(drag.pos, dy, window.innerHeight, drag.range);
    if (pos === last) return;
    last = pos;
    moveSubtitles(pos);
  }

  function end(moved: boolean) {
    if (moved && last !== null) keepSubtitlesPos(last);
    drag = null;
  }
</script>

{#if handle && box}
  <SubHandle {box} onhover={hover} onstart={start} onmove={move} onend={end} />
{/if}
