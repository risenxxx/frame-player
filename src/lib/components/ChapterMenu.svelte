<script lang="ts">
  /// The chapter list.
  import { tick } from 'svelte';

  import { formatTime } from '$lib/format';
  import { t } from '$lib/i18n.svelte';
  import { jumpToChapter, playback } from '$lib/playback.svelte';
  import { chapterTitle, player } from '$lib/player.svelte';

  interface Props {
    close: () => void;
  }

  let { close }: Props = $props();

  let el = $state<HTMLDivElement | undefined>();

  /// A film has dozens of chapters and the list opens scrolled to the top, so
  /// the one being played is usually out of sight — which is the only entry the
  /// viewer has a reference point for. `nearest` so an item already visible
  /// does not scroll anything at all.
  $effect(() => {
    void tick().then(() =>
      el?.querySelector('.menu-item.sel')?.scrollIntoView({ block: 'nearest' }),
    );
  });
</script>

<div class="menu chapters scrollable" bind:this={el}>
  <div class="menu-title">{t('osc.chapters')}</div>
  {#each player.chapters as chapter (chapter.index)}
    <button
      class="menu-item chapter-item"
      class:sel={chapter.index === playback.chapterIndex}
      onclick={() => {
        close();
        jumpToChapter(chapter);
      }}
    >
      <span class="chapter-name">{chapterTitle(chapter)}</span>
      <span class="hint">{formatTime(chapter.time)}</span>
    </button>
  {/each}
</div>
