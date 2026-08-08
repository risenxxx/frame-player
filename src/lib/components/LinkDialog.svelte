<script lang="ts">
  /// The "open a link" box: a URL, a magnet, or a .torrent — plus the way in for
  /// a .torrent *file*, which is the one thing a box that takes text cannot
  /// accept and the reason torrent support used to be invisible here.
  import Dialog from '$lib/components/Dialog.svelte';
  import { displayName, readableLink } from '$lib/format';
  import { t } from '$lib/i18n.svelte';
  import { torrent } from '$lib/torrent.svelte';
  import { ytdlp } from '$lib/player.svelte';

  interface Props {
    /// The box's own state, owned by the page: it survives the dialog closing
    /// (a failed link reopens it with the same text and an explanation).
    link: {
      value: string;
      failed: boolean;
      torrentError: string | null;
      recent: string[];
      titles: Record<string, string>;
      inputEl: HTMLInputElement | undefined;
    };
    /// The yt-dlp install/update button's progress, which is the page's: the
    /// download outlives this dialog being closed and reopened.
    ytdlpBusy: boolean;
    ytdlpPct: number | null;
    onclose: () => void;
    onSubmit: (url?: string) => void;
    onForget: (url: string) => void;
    onPickTorrentFile: () => void;
    onFixYtdlp: () => void;
  }

  let { link, ytdlpBusy, ytdlpPct, onclose, onSubmit, onForget, onPickTorrentFile, onFixYtdlp }: Props = $props();
</script>

<Dialog title={t('link.title')} variant="link" {onclose}>
  <input
    class="link-input"
    type="text"
    spellcheck="false"
    autocapitalize="off"
    autocorrect="off"
    placeholder={t('link.placeholder')}
    bind:this={link.inputEl}
    bind:value={link.value}
    onkeydown={(e) => {
      // Enter submits and Escape closes from inside the field, where the
      // window-level handler never sees them.
      if (e.key === 'Enter') { e.preventDefault(); onSubmit(); }
      if (e.key === 'Escape') { e.preventDefault(); onclose(); }
      e.stopPropagation();
    }}
  />
  <!-- Resolving a magnet is a DHT lookup, routinely ten seconds and
       occasionally a minute, so the dialog stays up and says what it is
       waiting for rather than closing on a black window. -->
  {#if torrent.resolving}
    <div class="link-progress">
      <span class="loading-spin"></span>
      <span>{t('torrent.resolving')}</span>
    </div>
    <div class="setting-hint">{t('torrent.resolving_hint')}</div>
  {:else if link.torrentError}
    <div class="link-error">{link.torrentError}</div>
  {:else if link.failed}
    <div class="link-error">
      {t(ytdlp.present ? 'link.failed_stale' : 'link.failed_missing')}
    </div>
  {:else}
    <div class="setting-hint">{t(ytdlp.present ? 'link.hint_ytdlp' : 'link.hint_plain')}</div>
  {/if}
  {#if link.recent.length && !link.value.trim()}
    <div class="link-recent">
      {#each link.recent as url (url)}
        <!-- Two controls, so the row is a flex container: opening the
             link is the row, forgetting it is the cross. Every ancestor
             between the ellipsised name and the dialog's fixed width
             needs `min-width: 0`, or the row grows to the name instead. -->
        <div class="link-recent-row">
          <button
            class="link-recent-item"
            data-tip={readableLink(url)}
            onclick={() => onSubmit(url)}
          >
            {link.titles[url] ?? displayName(url)}
          </button>
          <button
            class="link-recent-forget"
            data-tip={t('link.forget')}
            aria-label={t('link.forget')}
            onclick={() => onForget(url)}
          >
            <svg viewBox="0 0 10 10" aria-hidden="true">
              <path
                stroke="currentColor"
                stroke-width="1.4"
                stroke-linecap="round"
                d="M1.2 1.2l7.6 7.6M8.8 1.2l-7.6 7.6"
              />
            </svg>
          </button>
        </div>
      {/each}
    </div>
  {/if}
  <div class="link-actions">
    <!-- The one thing this dialog could not do, and the reason torrent
         support was invisible in it: a magnet is pasted, but a
         `.torrent` is a *file* — there is nothing to type, so a box that
         only takes text silently excluded half of how torrents arrive.
         Sits at the left, away from the primary action, because it is
         the other route rather than a variant of this one; the tooltip
         names dropping, which is the cheaper gesture of the two and
         which nothing on screen would otherwise reveal. -->
    <button
      class="btn-outline link-torrent"
      data-tip={t('link.torrent_file_tip')}
      disabled={torrent.resolving}
      onclick={onPickTorrentFile}
    >{t('link.torrent_file')}</button>
    <!-- Offered only for the copy we installed: someone else's yt-dlp is
         their package manager's to update, and pressing -U on it would
         either fail or start a fight. -->
    {#if !ytdlp.present || (link.failed && ytdlp.managed)}
      <!-- The progress is the button's own fill rather than a separate
           bar: a 36 MB download over a slow link otherwise leaves a
           button saying "downloading…" for a minute, which is
           indistinguishable from one that has hung. -->
      <button
        class="btn-outline"
        class:progressing={ytdlpPct !== null}
        style="--pct: {ytdlpPct ?? 0}%"
        disabled={ytdlpBusy}
        onclick={onFixYtdlp}
      >
        {ytdlpBusy
          ? ytdlpPct !== null
            ? t('link.ytdlp_pct', { percent: ytdlpPct })
            : t('link.ytdlp_working')
          : t(ytdlp.present ? 'link.ytdlp_update' : 'link.ytdlp_install')}
      </button>
    {/if}
    <button
      class="primary"
      disabled={!link.value.trim() || torrent.resolving}
      onclick={() => onSubmit()}
    >{t('link.open')}</button>
  </div>
</Dialog>

<style>
  /* Shown only while the field is empty: once something is typed the list is
     about the past and the field is about the present. */
  .link-recent {
    display: flex;
    flex-direction: column;
    margin-top: 8px;
    max-height: 168px;
    overflow-y: auto;
    /* Part of scrolling vertically, not an extra: setting one axis computes the
       other from `visible` to `auto`, and a row a fraction of a pixel too wide
       would otherwise raise a horizontal bar and eat a row's height. */
    overflow-x: hidden;
  }

  .link-recent-row {
    display: flex;
    align-items: center;
    /* The dialog is a fixed width and the name is arbitrary. Without this the
       row grows to the name instead of letting it ellipsise — `min-width: auto`
       is the default on a flex item, and it has to be cleared on every ancestor
       between the label and the fixed box, not only on the label. */
    min-width: 0;
    border-radius: 6px;
  }

  .link-recent-row:hover {
    background: rgba(255, 255, 255, 0.08);
  }

  .link-recent-item {
    flex: 1;
    min-width: 0;
    text-align: left;
    padding: 7px 8px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: #b9b9c3;
    font-size: 12.5px;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .link-recent-row:hover .link-recent-item {
    color: #e8e8ec;
  }

  /* Always on screen, never faded in: taking a 1.4px stroke off full opacity
     makes the engine re-rasterise it and the glyph visibly twitches in
     WKWebView. The three strengths are colour, exactly as on `.torrow-forget`. */
  .link-recent-forget {
    flex: none;
    width: 24px;
    height: 24px;
    margin-right: 4px;
    display: grid;
    place-items: center;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: transparent;
    color: rgba(255, 255, 255, 0.22);
    cursor: pointer;
    transition: color 0.12s ease;
  }

  .link-recent-row:hover .link-recent-forget {
    color: rgba(255, 255, 255, 0.5);
  }

  /* Written to win rather than left to source order: the rule above is a
     descendant selector and would otherwise out-weigh a bare
     `.link-recent-forget:hover`, so the cross would never reach full strength
     under the pointer — and it is only ever hovered while the row is. */
  .link-recent-row:hover .link-recent-forget:hover {
    background: rgba(255, 255, 255, 0.1);
    color: #f0f0f4;
  }

  .link-recent-forget svg {
    width: 10px;
    height: 10px;
  }

  /* `.link-actions` lives in app.css: the subtitle panel and the torrent
     update dialog draw the same footer row. */

  /* Pushed to the far left, so the row reads as "the other way in" on one side
     and "do the thing" on the other. `margin-right: auto` rather than
     `space-between` on the parent, which would also spread the yt-dlp button
     away from the primary one it belongs beside. */
  .link-torrent {
    margin-right: auto;
  }

  .link-progress {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 2px;
    color: #e8e8ec;
    font-size: 12.5px;
  }

  /* The hint's own 5px glues it to the line above, which here is a live status
     rather than a label — the same problem the language pills had, and fixed the
     same way: a sibling rule, so it holds wherever the pair occurs. */
  .link-progress + .setting-hint {
    margin-top: 11px;
  }
</style>
