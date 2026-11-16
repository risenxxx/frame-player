<script lang="ts">
  /// The second subtitle's place, sent to secondsub.lua — mpv draws the line on
  /// its OSD (see second-line.ts), so this component draws nothing. mpv decides
  /// *what* is on screen and when (`secondary-sub-text`, delay and speed
  /// included); this decides whether and where. Mounted by the page exactly
  /// while there is a picture for the line to be over, and unmounting hides it.
  ///
  /// The line moves at once when the bars come and go, where the web view's
  /// line followed their 0.25 s fade: mpv has no animation for an overlay, the
  /// same reason the main line's lift (sub-shift.svelte.ts) jumps.
  import { command } from 'tauri-plugin-libmpv-api';

  import { chrome } from '$lib/chrome.svelte';
  import { playback } from '$lib/playback.svelte';
  import { lineTop, placeArgs } from '$lib/second-line';
  import { subShift } from '$lib/sub-shift.svelte';
  import { secondSub, secondTrack } from '$lib/tracks.svelte';

  /// Not while a television plays: the local decoder is parked and its text
  /// would be a frozen line from wherever the handover happened.
  const shown = $derived(secondSub.shown && !playback.remote && secondTrack() !== undefined);
  const windowH = $derived(subShift.windowPx || window.innerHeight);
  /// Under the title bar while the bars are up, at mpv's own margin otherwise —
  /// the mirror of the main line getting out of the control bar's way.
  const top = $derived(lineTop(subShift.marginY, windowH, chrome.topBarPx, !chrome.idle));
  const args = $derived(placeArgs(shown, top, windowH));

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
    send(args);
  });

  $effect(() => {
    return () => {
      if (secondSub.drawer) send(placeArgs(false, 0, 1));
    };
  });
</script>
