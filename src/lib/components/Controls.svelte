<script lang="ts">
  /// The control row: play, previous/next, volume, speed, loop, the menu
  /// toggles, fullscreen.
  ///
  /// In the mini window everything that belongs to a full-sized session stands
  /// down and the row becomes one column — written on this element rather than
  /// as `.player.mini .controls`, since the player root is the page's and a
  /// scoped selector cannot reach across that boundary.
  import { command } from 'tauri-plugin-libmpv-api';

  import { cast, castAdvance, castSetVolume, castToggleMute, castTogglePause } from '$lib/cast.svelte';
  import { t } from '$lib/i18n.svelte';
  import { hintPair, withKey } from '$lib/keys.svelte';
  import { LOOP_LABEL, isNetworkSource, player } from '$lib/player.svelte';
  import { playlist } from '$lib/playlist.svelte';
  import { parseTorrentUrl } from '$lib/source';

  interface Props {
    mini: boolean;
    fullscreen: boolean;
    openMenu: 'audio' | 'sub' | 'chapter' | 'queue' | 'cast' | null;
    onToggleMenu: (kind: 'audio' | 'sub' | 'chapter' | 'queue' | 'cast') => void;
    onTogglePause: () => void;
    onToggleMute: () => void;
    onSetVolume: (v: number) => void;
    onCycleLoop: () => void;
    onToggleFullscreen: () => void;
  }

  let {
    mini,
    fullscreen,
    openMenu,
    onToggleMenu,
    onTogglePause,
    onToggleMute,
    onSetVolume,
    onCycleLoop,
    onToggleFullscreen,
  }: Props = $props();

  const hasFile = $derived(player.hasFile);
  const VOLUME_MAX = 100;
</script>

<div class="controls" class:mini>
  <div class="cluster cl-left">
  <!-- Disabled with the slider beside it, and for the same reason: a
       receiver that will not take a volume will not take a mute either, and
       a button that flips back on its own reads as broken. The keyboard
       path still explains where the volume lives. -->
  <button
    data-tip={withKey(t('osc.sound'), 'mute')}
    aria-label={t('osc.sound')}
    disabled={cast.remote && !cast.volumeAdjustable}
    onclick={() => (cast.remote ? castToggleMute() : onToggleMute())}
  >
    {#if cast.remote ? cast.muted : player.muted || player.volume === 0}
      <svg viewBox="0 0 24 24"><path fill="currentColor" d="M16.5 12A4.5 4.5 0 0 0 14 8v2.2l2.5 2.5v-.7zM19 12a7 7 0 0 1-1.2 3.9l1.5 1.5A9 9 0 0 0 21 12a9 9 0 0 0-7-8.8v2.1A7 7 0 0 1 19 12zM4.3 3L3 4.3 7.7 9H3v6h4l5 5v-6.7l4.3 4.2a6.9 6.9 0 0 1-2.3 1.2v2.1a9 9 0 0 0 3.7-1.8L20 21.7 21.3 20 4.3 3zM12 4L9.9 6.1 12 8.2V4z"/></svg>
    {:else}
      <svg viewBox="0 0 24 24"><path fill="currentColor" d="M3 9v6h4l5 5V4L7 9H3zm13.5 3A4.5 4.5 0 0 0 14 8v8a4.5 4.5 0 0 0 2.5-4zM14 3.2v2.1a7 7 0 0 1 0 13.4v2.1a9 9 0 0 0 0-17.6z"/></svg>
    {/if}
  </button>
  <!-- While casting the bar is the TV's volume — the receiver's own 0..1
       shown as percent, with no amplification range: 100 is the device's
       ceiling, not a point on the way to 150. -->
  <input
    class="volumebar"
    type="range"
    min="0"
    max={cast.remote ? 100 : VOLUME_MAX}
    step="1"
    disabled={cast.remote && !cast.volumeAdjustable}
    value={cast.remote ? Math.round(cast.volume * 100) : player.volume}
    oninput={(e) =>
      cast.remote
        ? castSetVolume(Number(e.currentTarget.value) / 100)
        : onSetVolume(Number(e.currentTarget.value))}
    onchange={(e) => e.currentTarget.blur()}
    style="--progress: {cast.remote ? cast.volume * 100 : (player.volume / VOLUME_MAX) * 100}%"
    data-tip={t('osd.volume', { value: Math.round(cast.remote ? cast.volume * 100 : player.volume) })}
    aria-label={t('osd.volume', { value: Math.round(cast.remote ? cast.volume * 100 : player.volume) })}
  />
  {#if player.speed !== 1 && !cast.remote}
    <span class="speed" data-tip={t('osc.speed')} aria-label={t('osc.speed')}>{player.speed}×</span>
  {/if}
  </div>
  <div class="cluster cl-center">
  <button data-tip={withKey(t('osc.prev'), 'playlist_prev')} aria-label={t('osc.prev')} disabled={player.playlistPos <= 0} onclick={() => (cast.active ? void castAdvance(-1) : void command('playlist-prev', []))}>
    <svg viewBox="0 0 24 24"><path fill="currentColor" d="M6 6h2v12H6zm3.5 6l8.5 6V6z"/></svg>
  </button>
  <button class="play" data-tip={withKey(t('osc.play'), 'pause')} aria-label={t('osc.play')} disabled={!hasFile} onclick={() => (cast.remote ? castTogglePause() : void onTogglePause())}>
    {#if cast.remote ? cast.paused : player.paused || player.eofReached}
      <svg viewBox="0 0 24 24"><path fill="currentColor" d="M8 5v14l11-7z"/></svg>
    {:else}
      <svg viewBox="0 0 24 24"><path fill="currentColor" d="M6 19h4V5H6v14zm8-14v14h4V5h-4z"/></svg>
    {/if}
  </button>
  <button data-tip={withKey(t('osc.next'), 'playlist_next')} aria-label={t('osc.next')} disabled={player.playlistPos >= player.playlistCount - 1} onclick={() => (cast.active ? void castAdvance(1) : void command('playlist-next', []))}>
    <svg viewBox="0 0 24 24"><path fill="currentColor" d="M16 6h2v12h-2zM6 18l8.5-6L6 6z"/></svg>
  </button>
  </div>
  <div class="cluster cl-right">
  <button
    data-tip={withKey(t(LOOP_LABEL[player.loopMode]), 'loop')}
    aria-label={t(LOOP_LABEL[player.loopMode])}
    class:active={player.loopMode !== 'off'}
    onclick={onCycleLoop}
  >
    <svg viewBox="0 0 24 24">
      <path fill="currentColor" d="M17 7H7v3l-4-4 4-4v3h12v6h-2V7zM7 17h10v-3l4 4-4 4v-3H5v-6h2v4z"/>
      <!-- The digit goes in the gap between the two arrows (y 7–17 of the
           viewBox), which is exactly where Material's own repeat_one puts
           it — drawn as a path rather than a "1" glyph, since a glyph has
           no dependable center in the em square. -->
      {#if player.loopMode === 'one'}
        <path fill="currentColor" d="M13 15V9h-1l-2 1v1h1.5v4H13z"/>
      {/if}
    </svg>
  </button>
  {#if playlist.hasQueue}
    <button
      class="menu-toggle"
      data-tip={t('osc.queue')}
      aria-label={t('osc.queue')}
      class:active={openMenu === 'queue'}
      onclick={() => onToggleMenu('queue')}
    >
      <!-- The bars share the chapter icon's rhythm exactly — same rows,
           same thickness — because the two sit side by side and any
           difference in pitch reads as a mistake rather than as a
           distinction. Two earlier attempts got the vertical placement
           wrong in opposite directions: a play mark hanging BELOW the last
           bar dropped the glyph's center to y=14 (against the box's 12),
           and a tighter pitch then lifted the bar group above its
           neighbour's.
           Everything horizontal here is on a 1.2-unit grid, which is what
           keeps the bars sharp: the 24-unit box renders into 20 px, so one
           unit is 5/6 px and only multiples of 1.2 land on a whole pixel —
           at 1x and, being multiples of 0.6 device px, at 2x as well.
           Off-grid edges bleed across two rows of pixels, which reads as
           bars that are both blurry and too thick, and a glyph in a
           different sub-pixel phase from its neighbour looks like a
           different weight. Hence 2.4 thick on rows 3.6 / 10.8 / 18, shared
           verbatim with the chapter icon.
           The play mark is centered on the last row and kept NARROW (4.8
           against 5.6 tall). A wide one read as another arrowhead beside
           the repeat icon, which carries one in the same corner — two
           right-pointing wedges side by side stop looking like two
           different controls. The ink it gives up is handed back to the
           bottom bar, whose row has to weigh as much as a full-width one or
           the whole glyph floats above its neighbour: measured 11.89
           against the chapter icon's 11.94, where the same mark over a
           short bar gave 11.73. -->
      <svg viewBox="0 0 24 24" fill="currentColor">
        <path d="M2.4 3.6h19.2v2.4H2.4zM2.4 10.8h19.2v2.4H2.4zM2.4 18h13.2v2.4H2.4zM16.8 16.4v5.6l4.8-2.8z"/>
      </svg>
    </button>
  {/if}
  {#if player.hasChapters}
    <button
      class="menu-toggle"
      data-type="chapters"
      data-tip={t('osc.with_key', { label: t('osc.chapters'), key: hintPair('chapter_prev', 'chapter_next') })}
      aria-label={t('osc.chapters')}
      class:active={openMenu === 'chapter'}
      onclick={() => onToggleMenu('chapter')}
    >
      <!-- Drawn to fill the box like its neighbours rather than to the
           proportions of Material's own list glyph: that one occupies
           16×12 of the 24×24 viewBox where the loop is 18×20 and the mic
           18×18, and at a shared 20px it visibly sat smaller than both.
           Bars are 2.4 thick on rows 3.6 / 10.8 / 18 — the 1.2-unit grid
           explained on the queue icon, which shares these rows exactly so
           the two read as one family standing side by side. -->
      <svg viewBox="0 0 24 24" fill="currentColor">
        <path d="M8.4 3.6h13.2v2.4H8.4zM8.4 10.8h13.2v2.4H8.4zM8.4 18h13.2v2.4H8.4z"/>
        <circle cx="4.2" cy="4.8" r="1.8"/>
        <circle cx="4.2" cy="12" r="1.8"/>
        <circle cx="4.2" cy="19.2" r="1.8"/>
      </svg>
    </button>
  {/if}
  {#if player.audioTracks.length > 1}
    <button class="menu-toggle" data-tip={t('osc.audio')} aria-label={t('osc.audio')} class:active={openMenu === 'audio'} onclick={() => onToggleMenu('audio')}>
      <svg viewBox="0 0 24 24"><path fill="currentColor" d="M12 3a9 9 0 0 0-9 9v7a2 2 0 0 0 2 2h3v-8H5v-1a7 7 0 1 1 14 0v1h-3v8h3a2 2 0 0 0 2-2v-7a9 9 0 0 0-9-9z"/></svg>
    </button>
  {/if}
  {#if hasFile}
    <button class="menu-toggle" data-tip={t('osc.subs')} aria-label={t('osc.subs')} class:active={openMenu === 'sub'} onclick={() => onToggleMenu('sub')}>
      <svg viewBox="0 0 24 24"><path fill="currentColor" d="M20 4H4a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V6a2 2 0 0 0-2-2zM6 10h2v2H6v-2zm8 6H6v-2h8v2zm4 0h-2v-2h2v2zm0-4H10v-2h8v2z"/></svg>
    </button>
  {/if}
  {#if (hasFile && (!isNetworkSource(player.filePath) || !!parseTorrentUrl(player.filePath ?? ''))) || cast.active}
    <!-- Indigo while connected: the accent means on/selected, and a live
         cast session is a boolean "on". Local files and torrents (whose
         data may be a complete file on this disk — resolveCastSource
         answers, and an incomplete one refuses with the reason); a plain
         network stream cannot be served to the TV from a disk it is
         not on. -->
    <button
      class="menu-toggle"
      class:cast-on={cast.active}
      data-tip={t('cast.tip')}
      aria-label={t('cast.tip')}
      class:active={openMenu === 'cast'}
      onclick={() => onToggleMenu('cast')}
    >
      <svg viewBox="0 0 24 24"><path fill="currentColor" d="M21 3H3c-1.1 0-2 .9-2 2v3h2V5h18v14h-7v2h7c1.1 0 2-.9 2-2V5c0-1.1-.9-2-2-2zM1 18v3h3c0-1.66-1.34-3-3-3zm0-4v2c2.76 0 5 2.24 5 5h2c0-3.87-3.13-7-7-7zm0-4v2c4.97 0 9 4.03 9 9h2c0-6.08-4.93-11-11-11z"/></svg>
    </button>
  {/if}
  <button
    class="fs"
    data-tip={withKey(t(fullscreen ? 'bar.fullscreen_exit' : 'bar.fullscreen'), 'fullscreen')}
    aria-label={t(fullscreen ? 'bar.fullscreen_exit' : 'bar.fullscreen')}
    onclick={onToggleFullscreen}
  >
    {#if fullscreen}
      <svg viewBox="0 0 24 24"><path fill="currentColor" d="M5 16h3v3h2v-5H5v2zm3-8H5v2h5V5H8v3zm6 11h2v-3h3v-2h-5v5zm2-11V5h-2v5h5V8h-3z"/></svg>
    {:else}
      <svg viewBox="0 0 24 24"><path fill="currentColor" d="M7 14H5v5h5v-2H7v-3zm-2-4h2V7h3V5H5v5zm12 7h-3v2h5v-5h-2v3zM14 5v2h3v3h2V5h-5z"/></svg>
    {/if}
  </button>
  </div>
</div>

<style>  .controls {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    margin-top: 8px;
  }

  .cluster {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .cl-left {
    justify-self: start;
  }

  .cl-center {
    justify-self: center;
    gap: 14px;
  }

  .cl-right {
    justify-self: end;
  }

  .controls button {
    background: transparent;
    border: none;
    border-radius: 8px;
    width: 36px;
    height: 34px;
    display: grid;
    place-items: center;
    color: #e8e8ec;
    cursor: pointer;
    padding: 0;
  }

  .controls button svg {
    width: 20px;
    height: 20px;
  }

  .controls button.play {
    width: 46px;
    height: 42px;
  }

  /* 32 rather than 30: every other box in the row is a multiple of 4 (20 for
     the ordinary controls, 24 for fullscreen), and rounding up also widens the
     gap to prev/next, which is the point of them being small. Those stay at 20
     — their glyph fills 12 of its 24 units, so a 24px box would render it 12px
     wide against play's 14.7 and the two would read as the same size. */
  .controls button.play svg {
    width: 32px;
    height: 32px;
  }

  .controls button:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.12);
  }

  .controls button:disabled {
    color: #6a6a74;
    cursor: default;
  }

  .controls button.active {
    color: #818cf8;
  }

  .speed {
    color: #818cf8;
    font-size: 12px;
    font-weight: 600;
    padding: 0 6px;
    text-shadow: var(--ui-shadow);
  }

  .volumebar {
    width: 90px;
    margin: 0 6px 0 2px;
  }
  /* The TV declared its volume fixed (or never reported one): the slider is
     disabled rather than silently inert, and keys/wheel raise the OSD saying
     volume lives on the TV's own remote. */
  .volumebar:disabled {
    opacity: 0.35;
    cursor: default;
  }

  /* The "corners" glyph is drawn more compactly than the rest — even out the
     optical size */
  .controls button.fs svg {
    width: 24px;
    height: 24px;
  }

  /* Indigo means on: a live cast session is a boolean "on", same rule as
     .switch.on and the focus rings. */
  .controls button.cast-on {
    color: #818cf8;
  }

  .controls.mini .cluster.cl-left,
  .controls.mini .cluster.cl-right {
    display: none;
  }

  /* `.controls` is a `1fr auto 1fr` grid, so hiding the side clusters does not
     center what is left — the remaining cluster auto-places into the first
     column and the row sits against the left edge. The mini row is one item,
     so it gets a one-column grid. */
  .controls.mini {
    grid-template-columns: 1fr;
    justify-items: center;
    margin-top: 4px;
  }

  .controls.mini button.play {
    width: 38px;
    height: 34px;
  }

  .controls.mini button.play svg {
    width: 24px;
    height: 24px;
  }
</style>
