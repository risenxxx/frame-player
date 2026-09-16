<script lang="ts">
  /// Which release of an RSS feed to open.
  ///
  /// A per-release feed usually holds the same season in more than one encoding
  /// (AVC and HEVC side by side is the ordinary case), and only the viewer knows
  /// which one they want. What is chosen here is also what the player will watch
  /// the feed *for*: the next upload of this exact release, not of its neighbour.
  import Dialog from '$lib/components/Dialog.svelte';
  import ScrollFade from '$lib/components/ScrollFade.svelte';
  import type { Feed, FeedItem } from '$lib/feed';
  import { locale, t } from '$lib/i18n.svelte';
  import { rememberedTorrent } from '$lib/torrent.svelte';
  import { fmtSize } from '$lib/units';

  interface Props {
    feed: Feed;
    items: FeedItem[];
    onclose: () => void;
    onPick: (item: FeedItem) => void;
  }

  let { feed, items, onclose, onPick }: Props = $props();

  /// A date and nothing finer: which of two uploads is newer is the question,
  /// and a feed's timestamps are the tracker's, not the viewer's clock.
  function day(published: number | null): string {
    if (!published) return '';
    return new Date(published * 1000).toLocaleDateString(locale(), {
      day: 'numeric',
      month: 'short',
      year: 'numeric',
    });
  }
</script>

<Dialog
  title={feed.title ?? t('feed.pick_title')}
  label={t('feed.pick_title')}
  variant="link"
  {onclose}
>
  <div class="setting-hint">{t('feed.pick_hint')}</div>
  <div class="feed-items">
    {#each items as item, i (i)}
      {@const known = item.info_hash ? rememberedTorrent(item.info_hash) : null}
      <button
        class="menu-item feed-item"
        class:known={!!known}
        data-tip={item.title}
        onclick={() => onPick(item)}
      >
        <span class="feed-item-name">{item.title}</span>
        <span class="feed-item-meta">
          {#if known}
            <span class="feed-item-known">{t('feed.pick_known')}</span>
          {/if}
          {#if item.published}<span>{day(item.published)}</span>{/if}
          {#if item.size}<span>{fmtSize(item.size)}</span>{/if}
        </span>
      </button>
    {/each}
    <ScrollFade />
  </div>
</Dialog>

<style>
  .feed-items {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-top: 10px;
    max-height: min(360px, 50vh);
    overflow-y: auto;
    overflow-x: hidden;
  }

  /* Two classes of equal weight decide `display` — written to win, the lesson
     the queue rows and the torrent picker both paid for. A column rather than a
     row: release names are long, and the one thing that tells two of them apart
     (AVC against HEVC, 1-11 against 1-12) is usually at the end. */
  .menu-item.feed-item {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 3px;
    min-width: 0;
  }

  .feed-item-name {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .feed-item-meta {
    display: flex;
    gap: 8px;
    color: #8a8a95;
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
  }

  .feed-item-known {
    color: #818cf8;
  }
</style>
