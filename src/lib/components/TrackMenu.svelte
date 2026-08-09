<script lang="ts">
  /// The audio and subtitle track menus. They share their whole body — the list
  /// plus the delay stepper — which the chapter menu does not, so one component
  /// serves both and `kind` narrows it.
  import { t } from '$lib/i18n.svelte';
  import { playback } from '$lib/playback.svelte';
  import { delayIsZero, formatDelay, player, type Track } from '$lib/player.svelte';
  import { openSubsDialog, removeSubtitle } from '$lib/subs.svelte';

  /// One press of the stepper. Matches mpv's own default sub-delay granularity.
  const DELAY_STEP = 0.1;

  interface Props {
    kind: 'audio' | 'sub';
    close: () => void;
    onSelect: (kind: 'audio' | 'sub', track: Track | null) => void;
    onAddFile: (kind: 'sub' | 'audio') => void;
    onNudgeDelay: (kind: 'sub' | 'audio', delta: number) => void;
    onResetDelay: (kind: 'sub' | 'audio') => void;
  }

  let {
    kind,
    close,
    onSelect,
    onAddFile,
    onNudgeDelay,
    onResetDelay,
  }: Props = $props();
</script>

<div class="menu scrollable">
  <div class="menu-title">{t(kind === 'audio' ? 'osc.audio' : 'osc.subs')}</div>
  <!-- Over DLNA the file went across with all its tracks and the choice
       belongs to the television — its renderer declares no action for
       audio at all (it has vendor ones for subtitles and 3D, so the
       absence is a decision, not a gap). The list would otherwise keep
       showing this player's selection, which is a claim we cannot back:
       a switch made on the TV's own remote never reaches us. -->
  {#if !playback.can.trackChoice}
    <div class="cast-hint">{t('cast.tracks_on_tv')}</div>
  {/if}
  {#each kind === 'audio' ? player.audioTracks : player.subTracks as track (track.id)}
    <!-- The × appears for external subtitle tracks only. An embedded one
         cannot be removed without rewriting the video file, and mpv's
         `sub-remove` refuses it too. -->
    {#if kind === 'sub' && track.external}
      <div class="queue-row">
        <button
          class="menu-item chapter-item"
          class:sel={track.selected}
          data-tip={track.label}
          onclick={() => onSelect('sub', track)}
        >
          <span class="chapter-name">{track.label}</span>
        </button>
        <button
          class="queue-remove"
          data-tip={t('osc.sub_remove')}
          aria-label={t('osc.sub_remove')}
          onclick={() => void removeSubtitle(track)}
        >
          <svg viewBox="0 0 10 10" aria-hidden="true">
            <path stroke="currentColor" stroke-width="1.4" stroke-linecap="round" d="M1.2 1.2l7.6 7.6M8.8 1.2l-7.6 7.6"/>
          </svg>
        </button>
      </div>
    {:else}
      <button class="menu-item" class:sel={track.selected} onclick={() => onSelect(kind!, track)}>
        {track.label}
      </button>
    {/if}
  {/each}
  {#if kind === 'sub'}
    <button
      class="menu-item"
      class:sel={!player.subTracks.some((track) => track.selected)}
      onclick={() => onSelect('sub', null)}
    >
      {t('osc.subs_off')}
    </button>
  {/if}
  <button class="menu-item" onclick={() => onAddFile(kind!)}>
    {t('osc.add_file')}
  </button>
  {#if kind === 'sub'}
    <button class="menu-item" onclick={() => { close(); void openSubsDialog(); }}>
      {t('subs.find')}
    </button>
  {/if}
  <div class="menu-sep"></div>
  <div class="menu-title">{t('osc.delay')}</div>
  <!-- Stays open on click: a delay is dialled in by repeated nudges while
       watching the result, not chosen once from a list. -->
  <div class="delayrow">
    <button class="speedopt" aria-label="-0.1" onclick={() => onNudgeDelay(kind!, -DELAY_STEP)}>−</button>
    <span class="delayval">{formatDelay(kind === 'audio' ? player.audioDelay : player.subDelay)}</span>
    <button class="speedopt" aria-label="+0.1" onclick={() => onNudgeDelay(kind!, DELAY_STEP)}>+</button>
    <span class="delaysep"></span>
    <!-- Always rendered, only disabled: the menu is anchored by its bottom
         edge, so a control appearing here would move the −/+ row the moment the
         delay leaves zero — right under the cursor that is clicking it. -->
    <button
      class="speedopt delayreset"
      disabled={delayIsZero(kind === 'audio' ? player.audioDelay : player.subDelay)}
      onclick={() => onResetDelay(kind!)}
    >
      {t('osc.delay_reset')}
    </button>
  </div>
</div>

<style>
  /* Delay stepper: the same pill row as the speed presets, because it is the
     same job — a compact group of small actions. The value between them is a
     readout, not a control, so it stays plain text rather than a third pill.

     Reset is *inside* the row rather than a `.menu-item` below it, and the
     reason is what hover exposed: a menu row is full-bleed and indented to line
     its text up with the list, so next to a 6px-inset pill its highlight was a
     different width and started at a different place — two shapes for one
     setting. In the row it is a fourth cell of the same control, and the
     divider is what says it belongs to the group without belonging to the
     stepper. */
  .delayrow {
    display: flex;
    align-items: center;
    gap: 2px;
    margin: 2px 8px 6px;
    /* 5px and 11px: the concentric rule against `.speedopt`'s 6px, and the same
       numbers as `.speedrow` — see the note there. */
    padding: 5px;
    background: rgba(255, 255, 255, 0.07);
    border-radius: 11px;
  }

  .delayval {
    flex: 1;
    text-align: center;
    color: #e8e8ec;
    font-size: 12px;
    /* Tabular figures: the value changes under the cursor, and proportional
       digits make the whole row twitch sideways with every nudge. */
    font-variant-numeric: tabular-nums;
  }

  .delayrow .speedopt {
    flex: 0 0 34px;
    font-size: 15px;
    line-height: 1;
  }

  /* A hairline, not a gap: the stepper and the reset are one control and two
     jobs, which is exactly what a divider says. Inset top and bottom so it
     reads as a rule between cells rather than a border of the row. */
  .delaysep {
    flex: 0 0 1px;
    align-self: stretch;
    margin: 3px 5px;
    background: rgba(255, 255, 255, 0.14);
  }

  /* Wider than the two glyph buttons because it holds a word, and at the list's
     own size — it is text, not a symbol. `auto` rather than a fixed width so
     "Сброс" and "Reset" each take what they need.

     One word, not a sentence: the label was `Сбросить задержку`, which is right
     for a menu row and wrong for a cell — the noun is already on the section
     title above, and at 75px the word squeezed the readout hard enough to wrap
     the unit onto a second line. Measured in the built stylesheet.

     `align-self: stretch` is the other half of the shape. Writing `padding: 0
     10px` for the horizontal inset silently took the vertical padding to zero,
     so this cell was **12px tall inside a 27px row** — the long thin pill. It
     now fills the row's content height like the two glyph buttons, and the text
     is centered by the grid rather than by a padding that has to be kept in step
     with theirs. */
  .delayrow .speedopt.delayreset {
    flex: 0 0 auto;
    align-self: stretch;
    display: grid;
    place-items: center;
    padding: 0 12px;
    font-size: 12px;
  }

  /* The readout may not wrap: it is the one part of the row whose width is not
     ours to choose, and a second line would grow the whole control. */
  .delayval {
    white-space: nowrap;
  }

  .delayrow .speedopt:disabled {
    color: #6a6a74;
    cursor: default;
  }

  .delayrow .speedopt:disabled:hover {
    background: transparent;
  }
</style>
