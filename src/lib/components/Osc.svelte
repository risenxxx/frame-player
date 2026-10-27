<script lang="ts">
  /// The control bar: the menus that open above it, the seekbar and the
  /// control row, under one scrim. The page decides whether it exists at all
  /// (not on the start screen) and when it has faded far enough to leave
  /// (`gone`, from `faded`, which has to outlive this component); the mini
  /// player's tighter padding reads the page's root (`.player.mini`), so that
  /// one rule is written against the ancestor with `:global`.
  import CastMenu from '$lib/components/CastMenu.svelte';
  import ChapterMenu from '$lib/components/ChapterMenu.svelte';
  import Controls from '$lib/components/Controls.svelte';
  import QueueMenu from '$lib/components/QueueMenu.svelte';
  import SeekBar from '$lib/components/SeekBar.svelte';
  import TrackMenu from '$lib/components/TrackMenu.svelte';
  import { diag, runDiagnosis } from '$lib/cast-diagnosis.svelte';
  import { chrome, toggleFullscreen } from '$lib/chrome.svelte';
  import { overlays, toggleMenu } from '$lib/overlays.svelte';
  import { cycleLoop } from '$lib/player.svelte';
  import { removeFromQueue } from '$lib/playlist.svelte';
  import { resume } from '$lib/resume.svelte';
  import { seekbar } from '$lib/seekbar.svelte';
  import { subShift } from '$lib/sub-shift.svelte';
  import { addTrackFile, nudgeDelayHere, resetDelayHere, selectTrack } from '$lib/tracks.svelte';
  import { mini } from '$lib/window-prefs.svelte';

  interface Props {
    /// Faded out and taken off the page — see `faded`.
    gone: boolean;
  }

  let { gone }: Props = $props();
</script>

<!-- The click handler is a guard, not an affordance: it stops a click on the
     bar reaching the video's play/pause. There is nothing here to activate
     from the keyboard — every control inside is a real button, reachable on
     its own — so a keydown handler would add a focus stop that leads nowhere.
     `role="toolbar"` already says what this is. The directive has to be a
     comment of its own and start with the word: prose in front of it and it
     is not seen at all. -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="osc"
  class:hidden={chrome.idle}
  class:gone
  bind:this={subShift.oscEl}
  role="toolbar"
  tabindex="-1"
  onclick={(e) => e.stopPropagation()}
  ondblclick={(e) => e.stopPropagation()}
  onmouseenter={() => (chrome.oscHover = true)}
  onmouseleave={() => (chrome.oscHover = false)}
>
  {#if overlays.menu === 'queue'}
    <QueueMenu
      close={() => (overlays.menu = null)}
      onRemove={(index) => void removeFromQueue(index)}
    />
  {:else if overlays.menu === 'chapter'}
    <ChapterMenu close={() => (overlays.menu = null)} />
  {:else if overlays.menu === 'cast'}
    <CastMenu
      close={() => (overlays.menu = null)}
      diagBusy={diag.busy}
      onDiagnose={(device) => void runDiagnosis(device)}
    />
  {:else if overlays.trackMenu}
    <TrackMenu
      kind={overlays.trackMenu}
      close={() => (overlays.menu = null)}
      onSelect={(k, tr) => { overlays.menu = null; selectTrack(k, tr); }}
      onAddFile={(k) => void addTrackFile(k)}
      onNudgeDelay={nudgeDelayHere}
      onResetDelay={resetDelayHere}
    />
  {/if}

  <SeekBar
    barDuration={resume.barDuration}
    chapterMarks={seekbar.chapterMarks}
    abRegion={seekbar.abRegion}
    hoverChapter={seekbar.hoverChapter}
    hasThumbs={seekbar.hasThumbs}
    thumbAspect={seekbar.thumbAspect}
    mini={mini.on}
  />
  <Controls
    mini={mini.on}
    fullscreen={chrome.fullscreen}
    openMenu={overlays.menu}
    onToggleMenu={toggleMenu}
    onCycleLoop={cycleLoop}
    onToggleFullscreen={() => void toggleFullscreen()}
  />
</div>

<style>
  .osc {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    /* No top padding: those 44px were pure gradient with no controls, yet they
       caught clicks (see .topbar). The gradient moved into ::before, which
       extends the same 44px upwards and receives no events.
       20px at the sides is the window's one edge — see `.controls` in
       Controls.svelte, which is what else sits on it. */
    padding: 0 20px 12px;
    /* Its own stacking context, so that z-index: -1 on ::before lowers the
       gradient under the controls without dropping it behind the whole bar. */
    isolation: isolate;
    /* `--chrome-fade`: see `.topbar`, and `chrome.unsteady` for why. */
    transition: opacity var(--chrome-fade, 0.25s) ease, transform var(--chrome-fade, 0.25s) ease;
  }

  .osc::before {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    top: -44px;
    background: linear-gradient(to top, rgba(0, 0, 0, 0.72), rgba(0, 0, 0, 0.32) 55%, transparent);
    pointer-events: none;
    /* MANDATORY. By the painting rules an absolutely positioned ::before lands
       ABOVE ordinary in-flow children (.seekrow, .controls), and the gradient
       darkened the buttons — they looked disabled. This did not happen before,
       because the gradient was the bar's own background and backgrounds are
       painted first. The same scheme works on .topbar without z-index only
       because its children are themselves absolutely positioned and come after
       ::before in DOM order. */
    z-index: -1;
  }

  .osc.hidden {
    opacity: 0;
    transform: translateY(10px);
    pointer-events: none;
  }

  /* Faded out and taken off the page: the seekbar under a transparent bar is
     still written on every video frame, and still painted. See `faded`. */
  .osc.gone {
    visibility: hidden;
  }

  :global(.player.mini) .osc {
    padding: 0 10px 6px;
  }
</style>
