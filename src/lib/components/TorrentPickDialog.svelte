<script lang="ts">
  /// Which file of a multi-video torrent to play.
  ///
  /// The rest still become queue entries, which is free: nothing is downloaded
  /// until mpv reads one (see torrent.rs).
  import Dialog from '$lib/components/Dialog.svelte';
  import ScrollFade from '$lib/components/ScrollFade.svelte';
  import { displayName, formatTime } from '$lib/format';
  import { t } from '$lib/i18n.svelte';
  import { torrentVideos, type TorrentFile, type TorrentInfo } from '$lib/torrent.svelte';
  import { baseName } from '$lib/format';
  import { fmtSize } from '$lib/units';

  interface Props {
    info: TorrentInfo;
    /// Positions per file, read once when the panel opens rather than per row:
    /// a season is nine names that differ by two characters, and "which one was
    /// I on" is the actual question being asked of this list.
    positions: Record<number, { pos: number; dur: number }>;
    /// Episodes watched to the end, by file name.
    ///
    /// **Not derivable from `positions`**: a finished video has its position
    /// *deleted*, which is what takes it out of "continue watching" and is
    /// exactly why the torrent store keeps this list. Without it the two states
    /// a viewer needs to tell apart — seen, and never opened — look identical.
    finished: Set<string>;
    onclose: () => void;
    onPick: (info: TorrentInfo, file: TorrentFile) => void;
  }

  let { info, positions, finished, onclose, onPick }: Props = $props();
</script>

<Dialog
  title={info.name ?? t('torrent.pick_title')}
  label={t('torrent.pick_title')}
  variant="link"
  {onclose}
>
  <div class="setting-hint">{t('torrent.pick_hint')}</div>
  <div class="torrent-files scrollable">
    {#each torrentVideos(info) as file (file.index)}
      
      {@const seen = positions[file.index]}
      {@const done = finished.has(baseName(file.path))}
      <button
        class="menu-item torrent-file"
        class:started={!!seen}
        class:done
        data-tip={file.path}
        onclick={() => onPick(info, file)}
      >
        {#if done}
          <!-- A mark rather than a word: the row is already three columns of
               text, and the one thing it has to answer at a glance is which of
               nine near-identical names has been seen. -->
          <svg class="torrent-file-tick" viewBox="0 0 12 12" aria-hidden="true">
            <path
              d="M2.2 6.3L4.7 8.8 9.8 3.7"
              fill="none"
              stroke="currentColor"
              stroke-width="1.6"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
          </svg>
        {/if}
        <span class="torrent-file-name">{displayName(file.path)}</span>
        {#if seen && seen.dur > 0}
          <span class="torrent-file-left">
            {t('start.remaining', { time: formatTime(Math.max(0, seen.dur - seen.pos)) })}
          </span>
        {/if}
        <span class="torrent-file-size">{fmtSize(file.size)}</span>
      </button>
    {/each}
    <ScrollFade />
  </div>
</Dialog>

<style>
  .torrent-files {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-top: 10px;
    /* Bounded rather than free: a season is dozens of rows and a dialog that
       grows past the window has nowhere to put its own close button. */
    max-height: min(360px, 50vh);
    overflow-y: auto;
    overflow-x: hidden;
  }

  .torrent-file-name {
    /* Both halves are needed: a flex item defaults to `min-width: auto` and so
       never shrinks below its content, and the ellipsis then never fires. */
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .torrent-file-size {
    flex: none;
    color: #8a8a95;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }

  /* A file with a position is the one being looked for, so it reads brighter
     than the rest rather than merely carrying extra text. */
  .torrent-file.started .torrent-file-name {
    color: #ffffff;
  }

  /* And a finished one reads quieter, which is the other direction from the
     same reference point: the list has three states and the eye should be able
     to sort them without reading a word. */
  .torrent-file.done .torrent-file-name {
    color: #8a8a95;
  }

  .torrent-file-tick {
    flex: none;
    width: 12px;
    height: 12px;
    color: #6f6f7a;
    /* The row is baseline-aligned, which puts an inline SVG's *bottom* edge on
       the text baseline and leaves the tick riding high above it. */
    align-self: center;
  }

  .torrent-file-left {
    flex: none;
    color: #818cf8;
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
  }

  /* Two classes of equal weight decide `display`, and which one wins is source
     order — the queue rows lost that fight once already (see CLAUDE.md). Written
     to win rather than left to chance. */
  .menu-item.torrent-file {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
  }
</style>
