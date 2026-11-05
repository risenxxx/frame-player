<script lang="ts">
  /// What an audio file shows instead of a picture: its cover over a blurred
  /// copy of itself, the title, the artist and the album, and the line of the
  /// lyrics being sung. See `audio-screen.svelte.ts` for when it is up and
  /// where each of those comes from.
  ///
  /// Takes no pointer events at all: a click on it is a click on the video —
  /// pause, a double click for fullscreen, the wheel for the volume — which
  /// `onVideoClick` only answers when the click lands on the player itself.
  import { audioScreen } from '$lib/audio-screen.svelte';
  import { player } from '$lib/player.svelte';

  const byline = $derived(audioScreen.tags.artist);
  // The same separator the track menus put between a track's name and its
  // language: two facts about one thing, not a list.
  const album = $derived([audioScreen.tags.album, audioScreen.tags.year].filter(Boolean).join(' · '));
</script>

<div class="audio-screen" aria-hidden="true">
  {#if audioScreen.cover}
    <div class="audio-wash" style:background-image="url({audioScreen.cover})"></div>
  {/if}
  <div class="audio-body">
    <div class="audio-cover" class:empty={!audioScreen.cover}>
      {#if audioScreen.cover}
        <img src={audioScreen.cover} alt="" draggable="false" />
      {:else}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
          <path d="M9 18V5l12-2v13" />
          <circle cx="6" cy="18" r="3" />
          <circle cx="18" cy="16" r="3" />
        </svg>
      {/if}
    </div>
    <div class="audio-text">
      <div class="audio-title">{player.displayTitle}</div>
      {#if byline}<div class="audio-artist">{byline}</div>{/if}
      {#if album}<div class="audio-album">{album}</div>{/if}
      {#key audioScreen.lyric}
        {#if audioScreen.lyric}<div class="audio-lyric">{audioScreen.lyric}</div>{/if}
      {/key}
    </div>
  </div>
</div>

<style>
  /* Opaque: with no picture mpv paints only its black field, and the blurred
     wash is the whole of the background — and where there is no cover, the
     player's own dark (gotcha 10 holds either way). */
  .audio-screen {
    position: absolute;
    inset: 0;
    overflow: hidden;
    background: #101016;
    pointer-events: none;
    container-type: size;
  }

  .audio-wash {
    position: absolute;
    inset: -12%;
    background-size: cover;
    background-position: center;
    filter: blur(48px) saturate(1.25) brightness(0.42);
  }

  /* The space between the title bar and the control bar, so neither ever
     covers the cover. */
  .audio-body {
    position: absolute;
    inset: 56px 32px 96px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 22px;
    color: #f2f2f5;
    text-align: center;
  }

  .audio-cover {
    flex: none;
    width: min(58cqh, 44cqw, 440px);
    aspect-ratio: 1;
    border-radius: 8px;
    overflow: hidden;
    box-shadow: 0 18px 50px rgba(0, 0, 0, 0.5);
  }

  .audio-cover img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .audio-cover.empty {
    display: grid;
    place-items: center;
    background: linear-gradient(145deg, #2a2a36, #18181f);
    color: rgba(242, 242, 245, 0.35);
  }

  .audio-cover.empty svg {
    width: 38%;
    height: 38%;
  }

  .audio-text {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    max-width: min(620px, 100%);
    min-width: 0;
  }

  .audio-title,
  .audio-artist,
  .audio-album {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .audio-title {
    font-size: 20px;
    font-weight: 600;
  }

  .audio-artist {
    font-size: 15px;
    color: rgba(242, 242, 245, 0.78);
  }

  .audio-album {
    font-size: 13px;
    color: rgba(242, 242, 245, 0.5);
  }

  .audio-lyric {
    margin-top: 14px;
    font-size: 17px;
    line-height: 1.35;
    color: rgba(242, 242, 245, 0.92);
    text-shadow: 0 1px 8px rgba(0, 0, 0, 0.6);
    white-space: pre-line;
    animation: audio-lyric-in 0.25s ease-out;
  }

  @keyframes audio-lyric-in {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
  }

  /* A short window — the mini player, or one dragged low and wide: the cover
     goes beside the text rather than above it, and the album and the lyrics
     give way first. */
  @container (max-height: 380px) and (min-aspect-ratio: 4 / 3) {
    .audio-body {
      flex-direction: row;
      justify-content: center;
      gap: 20px;
      inset: 40px 24px 72px;
      text-align: left;
    }

    .audio-cover {
      width: min(100cqh - 112px, 40cqw);
    }

    .audio-text {
      align-items: flex-start;
    }
  }

  @container (max-height: 260px) {
    .audio-album,
    .audio-lyric {
      display: none;
    }

    .audio-title {
      font-size: 16px;
    }

    .audio-artist {
      font-size: 13px;
    }
  }
</style>
