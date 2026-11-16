<script lang="ts">
  /// The second subtitle's place, and the handle that moves it. mpv draws the
  /// line on its OSD (sublines.lua, see second-line.ts), so what is here is
  /// the place sent to it and an invisible box over the line to drag it by.
  /// mpv decides *what* is on screen and when (`secondary-sub-text`, delay and
  /// speed included). Mounted by the page exactly while there is a picture for
  /// the line to be over, and unmounting hides it.
  ///
  /// The line moves at once when the bars come and go, where the web view's
  /// line followed their 0.25 s fade: mpv has no animation for an overlay, the
  /// same reason the main line's lift (sub-shift.svelte.ts) jumps.
  import { command } from 'tauri-plugin-libmpv-api';

  import { chrome } from '$lib/chrome.svelte';
  import { playback } from '$lib/playback.svelte';
  import { player } from '$lib/player.svelte';
  import {
    defaultPlace,
    dragPlace,
    parseBox,
    placeArgs,
    placeInForce,
    type Box,
    type LinePlace,
  } from '$lib/second-line';
  import { subShift } from '$lib/sub-shift.svelte';
  import { secondSub, secondTrack, setSecondPlace } from '$lib/tracks.svelte';
  import SubHandle from './SubHandle.svelte';

  /// Not while a television plays: the local decoder is parked and its text
  /// would be a frozen line from wherever the handover happened.
  const shown = $derived(secondSub.shown && !playback.remote && secondTrack() !== undefined);
  const windowH = $derived(subShift.windowPx || window.innerHeight);

  /// The viewer's place, clear of the bars while they are up.
  const inForce = $derived(
    placeInForce(
      secondSub.place ?? defaultPlace(subShift.marginY),
      windowH,
      chrome.topBarPx,
      subShift.barPx,
      !chrome.idle,
      subShift.enabled,
    ),
  );

  /// The pointer is over the line. The bars come up with the pointer, and the
  /// line would then step out of their way — out from under the pointer, which
  /// would leave the box, the bars would go, the line come back under it. So
  /// while the pointer is over it, and while it is dragged, the line holds the
  /// place it had when the pointer arrived.
  let held = $state<LinePlace | null>(null);
  let drag = $state<{ start: LinePlace; box: Box } | null>(null);
  let draft = $state<LinePlace | null>(null);
  let snapped = $state(false);

  const place = $derived(draft ?? held ?? inForce);
  const drawn = $derived(parseBox(player.secondBox));
  /// What the handle covers: the line as drawn, or — while it is being dragged
  /// across the gap between two cues — where it was when the drag began, so
  /// the handle and its pointer capture outlive the line.
  const box = $derived(drawn ?? drag?.box ?? null);
  /// The handle exists only while the bars are up — the pointer is moving — or
  /// while it is in use: the rest of the time it would be a box re-laid out at
  /// every cue for a pointer that is not there, and a page that repaints is
  /// what the line left the web view to avoid.
  const handle = $derived(
    drag !== null || (shown && drawn !== null && (!chrome.idle || held !== null)),
  );

  /// The last message sent, so a resize that changes nothing sends nothing.
  let sent = '';

  function send(message: string[]) {
    const key = message.join(' ');
    if (key === sent) return;
    sent = key;
    void command('script-message-to', message).catch(() => {});
  }

  $effect(() => {
    if (!secondSub.drawer) return;
    send(placeArgs(shown, place));
  });

  // A cue that ends under a resting pointer takes the handle away without a
  // `pointerleave`; nothing is over the line any more.
  $effect(() => {
    if (!handle) held = null;
  });

  $effect(() => {
    return () => {
      if (secondSub.drawer) send(placeArgs(false, defaultPlace(0)));
    };
  });

  function hover(over: boolean) {
    held = over ? { ...place } : null;
  }

  function start() {
    if (box) drag = { start: { ...place }, box: { ...box } };
  }

  function move(dx: number, dy: number) {
    if (!drag) return;
    const next = dragPlace(drag.start, drag.box, dx, dy, window.innerWidth, window.innerHeight);
    draft = next.place;
    snapped = next.snapped;
  }

  function end(moved: boolean) {
    if (moved && draft) {
      setSecondPlace(draft);
      held = draft;
    }
    drag = null;
    draft = null;
    snapped = false;
  }
</script>

{#if handle && box}
  <SubHandle {box} guide={snapped} onhover={hover} onstart={start} onmove={move} onend={end} />
{/if}
