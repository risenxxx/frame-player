<script lang="ts">
  /// Which file of a multi-video torrent to play.
  ///
  /// The rest still become queue entries, which is free: nothing is downloaded
  /// until mpv reads one (see torrent.rs).
  import Dialog from '$lib/components/Dialog.svelte';
  import { displayName, formatTime } from '$lib/format';
  import { t } from '$lib/i18n.svelte';
  import { torrentVideos, type TorrentFile, type TorrentInfo } from '$lib/torrent.svelte';
  import { fmtSize } from '$lib/units';

  interface Props {
    info: TorrentInfo;
    /// Positions per file, read once when the panel opens rather than per row:
    /// a season is nine names that differ by two characters, and "which one was
    /// I on" is the actual question being asked of this list.
    watched: Record<number, { pos: number; dur: number }>;
    onclose: () => void;
    onPick: (info: TorrentInfo, file: TorrentFile) => void;
  }

  let { info, watched, onclose, onPick }: Props = $props();
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
      
      {@const seen = watched[file.index]}
      <button
        class="menu-item torrent-file"
        class:started={!!seen}
        data-tip={file.path}
        onclick={() => onPick(info, file)}
      >
        <span class="torrent-file-name">{displayName(file.path)}</span>
        {#if seen && seen.dur > 0}
          <span class="torrent-file-left">
            {t('start.remaining', { time: formatTime(Math.max(0, seen.dur - seen.pos)) })}
          </span>
        {/if}
        <span class="torrent-file-size">{fmtSize(file.size)}</span>
      </button>
    {/each}
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
