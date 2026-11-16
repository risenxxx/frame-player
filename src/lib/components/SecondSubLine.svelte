<script lang="ts">
  /// The second subtitle, drawn here rather than by mpv — see second-line.ts
  /// for why mpv's own drawing could not be given a margin. mpv still decides
  /// *what* is on screen and when (`secondary-sub-text`, delay and speed
  /// included); this only decides where and in what look.
  import { chrome } from '$lib/chrome.svelte';
  import { playback } from '$lib/playback.svelte';
  import { player } from '$lib/player.svelte';
  import { lineStyle, lineTop } from '$lib/second-line';
  import { subShift } from '$lib/sub-shift.svelte';
  import { secondSub, secondTrack } from '$lib/tracks.svelte';

  /// Not while a television plays: the local decoder is parked and its text
  /// would be a frozen line from wherever the handover happened.
  const text = $derived(
    secondSub.shown && !playback.remote && secondTrack() ? player.secondSubText.trim() : '',
  );
  const look = $derived(lineStyle(player.subStyle, subShift.windowPx || window.innerHeight));
  /// Under the title bar while the bars are up, at mpv's own margin otherwise —
  /// the mirror of the main line getting out of the control bar's way.
  const top = $derived(
    lineTop(subShift.marginY, subShift.windowPx || window.innerHeight, chrome.topBarPx, !chrome.idle),
  );
</script>

{#if text}
  <div
    class="second-line"
    style:top="{top}px"
    style:font-family={look.fontFamily}
    style:font-size="{look.fontSize}px"
    style:line-height="{look.lineHeight}px"
    style:font-weight={look.fontWeight}
    style:color={look.color}
    style:--stroke="{look.stroke * 2}px"
    style:--stroke-color={look.strokeColor}
    style:text-shadow={look.shadow}
    aria-hidden="true"
  >
    <span class:boxed={look.box !== null} style:background={look.box}>{text}</span>
  </div>
{/if}

<style>
  /* Over the picture and under every surface: it is part of the picture as far
     as the viewer is concerned, so the bars, the menus and the dialogs all
     cover it. It takes no clicks — a click there is a click on the video. */
  .second-line {
    position: absolute;
    left: 5%;
    right: 5%;
    text-align: center;
    pointer-events: none;
    /* Cue text carries its own line breaks, and a long line wraps as mpv
       would wrap it. */
    white-space: pre-line;
    overflow-wrap: break-word;
    /* The bars fade over 0.25 s; the line moves with them rather than ahead. */
    transition: top 0.25s ease;
    /* An outline as libass draws one: a stroke under the fill, so it grows
       outwards and does not eat into the letters. */
    -webkit-text-stroke: var(--stroke) var(--stroke-color);
    paint-order: stroke fill;
  }

  /* mpv's box style: one box round the whole cue, line by line. */
  .boxed {
    padding: 0 0.15em;
    box-decoration-break: clone;
    -webkit-box-decoration-break: clone;
  }

  @media (prefers-reduced-motion: reduce) {
    .second-line {
      transition: none;
    }
  }
</style>
