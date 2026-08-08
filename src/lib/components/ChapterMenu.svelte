<script lang="ts">
  /// The chapter list.
  import { tick } from 'svelte';

  import { cast, castSeek } from '$lib/cast.svelte';
  import { formatTime } from '$lib/format';
  import { t } from '$lib/i18n.svelte';
  import { chapterTitle, player } from '$lib/player.svelte';

  interface Props {
    close: () => void;
    onSeekChapter: (index: number) => void;
  }

  let { close, onSeekChapter }: Props = $props();

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
      class:sel={chapter.index === player.chapterIndex}
      onclick={() => {
        close();
        // While the television owns playback, `chapter` is a property of
        // the paused local player: setting it moves mpv, the knob jumps
        // to the new time and the next status report drags it back — the
        // seek never leaves this machine. The chapter list is local
        // knowledge about the same file, so the jump is a remote seek to
        // its timestamp, exactly as the chapter *keys* already did.
        if (cast.remote) castSeek(chapter.time);
        else onSeekChapter(chapter.index);
      }}
    >
      <span class="chapter-name">{chapterTitle(chapter)}</span>
      <span class="hint">{formatTime(chapter.time)}</span>
    </button>
  {/each}
</div>
