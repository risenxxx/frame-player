<script lang="ts">
  /// The title bar: window buttons, the centered video title, the update button,
  /// and the torrent readout that hangs below it.
  ///
  /// Two of its states used to be written as `.player.mini .topbar` and
  /// `.player.no-video .topbar::before` — an ancestor on the page's own root.
  /// Svelte scopes a selector to the component that wrote it, so neither could
  /// have reached this element from there; they arrive as props and are written
  /// on the bar itself.
  import type { Snippet } from 'svelte';
  import { t } from '$lib/i18n.svelte';
  import { withKey } from '$lib/keys.svelte';
  import { IS_MAC } from '$lib/platform';
  import { player } from '$lib/player.svelte';
  import { sync } from '$lib/sync/apply.svelte';
  import { formatCode } from '$lib/sync/protocol';
  import { wire } from '$lib/sync/wire.svelte';
  import type { TorrentStatus } from '$lib/torrent.svelte';

  interface Props {
    idle: boolean;
    mini: boolean;
    /// No file open, so the scrim has nothing to darken.
    noVideo: boolean;
    fullscreen: boolean;
    /// How much room the centered title may take before it would run into the
    /// window buttons — measured by the page, which owns both ends.
    barSide: number;
    barTitleText: string;
    titleSlide: string;
    brandEl: HTMLElement | null;
    chromeEl: HTMLElement | null;
    updateAvail: { version: string } | null;
    updatePct: number | null;
    torrentChip: TorrentStatus | null;
    torrentLabel: string | null;
    onTitlebarMouseDown: (e: MouseEvent) => void;
    onInstallUpdate: () => void;
    onMinimize: () => void;
    onClose: () => void;
    onToggleFullscreen: () => void;
    onExitFullscreen: () => void;
    onBarHover: (over: boolean) => void;
  }

  let {
    idle,
    mini,
    noVideo,
    fullscreen,
    barSide,
    barTitleText,
    titleSlide,
    brandEl = $bindable(),
    chromeEl = $bindable(),
    updateAvail,
    updatePct,
    torrentChip,
    torrentLabel,
    onTitlebarMouseDown,
    onInstallUpdate,
    onMinimize,
    onClose,
    onToggleFullscreen,
    onExitFullscreen,
    onBarHover,
  }: Props = $props();
</script>

{#snippet brandMark()}
  <svg class="logo" viewBox="0 0 24 24"><path fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" d="M3.2 7.6V5.9c0-1.5 1.2-2.7 2.7-2.7h1.7M16.4 3.2h1.7c1.5 0 2.7 1.2 2.7 2.7v1.7M20.8 16.4v1.7c0 1.5-1.2 2.7-2.7 2.7h-1.7M7.6 20.8H5.9c-1.5 0-2.7-1.2-2.7-2.7v-1.7"/><path fill="#e8e8ec" stroke="#e8e8ec" stroke-width="1.6" stroke-linejoin="round" d="M10 9v6l5.4-3z"/></svg>
  <span class="appname">Frame Player</span>
{/snippet}

<div
  class="topbar"
  class:hidden={idle}
  class:mini
  class:no-video={noVideo}
  style="--bar-side: {barSide}px"
  data-tauri-drag-region={IS_MAC || undefined}
  role="presentation"
  onmousedown={onTitlebarMouseDown}
  onmouseenter={() => (onBarHover(true))}
  onmouseleave={() => (onBarHover(false))}
  onclick={(e) => e.stopPropagation()}
  ondblclick={(e) => e.stopPropagation()}
>
  {#if !IS_MAC}
    <div class="brand" bind:this={brandEl}>{@render brandMark()}</div>
  {/if}
  <div class="titlecenter">
    <span
      class="title"
      class:slide-out-next={titleSlide === 'out-next'}
      class:slide-out-prev={titleSlide === 'out-prev'}
      class:slide-prep-next={titleSlide === 'prep-next'}
      class:slide-prep-prev={titleSlide === 'prep-prev'}>{barTitleText}</span>
  </div>
  <div class="chrome" bind:this={chromeEl}>
  {#if updateAvail}
    <button
      class="updbtn"
      class:progressing={updatePct !== null}
      style="--pct: {updatePct ?? 0}%"
      onclick={onInstallUpdate}
      disabled={updatePct !== null}
    >
      {updatePct === null
        ? t('bar.update', { version: updateAvail.version })
        : t('bar.downloading', { percent: updatePct })}
    </button>
  {/if}
  <!-- macOS: minimize/close/fullscreen are the system traffic lights on the
       left, and fullscreen is left with the OSC button (bottom right), Esc or
       F — there are no title-bar buttons of our own there at all. -->
  {#if !IS_MAC}
    <div class="winbtns">
      <button class="winbtn" data-tip={t('bar.minimize')} aria-label={t('bar.minimize')} onclick={onMinimize}>
        <svg viewBox="0 0 10 10"><path stroke="currentColor" d="M0 5h10"/></svg>
      </button>
      {#if fullscreen}
        <button class="winbtn fsx" data-tip={t('bar.fullscreen_exit_esc')} aria-label={t('bar.fullscreen_exit_esc')} onclick={() => onExitFullscreen()}>
          <svg viewBox="0 0 24 24"><path fill="currentColor" d="M5 16h3v3h2v-5H5v2zm3-8H5v2h5V5H8v3zm6 11h2v-3h3v-2h-5v5zm2-11V5h-2v5h5V8h-3z"/></svg>
        </button>
      {:else}
        <button class="winbtn fsx" data-tip={withKey(t('bar.fullscreen'), 'fullscreen')} aria-label={t('bar.fullscreen')} onclick={() => onToggleFullscreen()}>
          <svg viewBox="0 0 24 24"><path fill="currentColor" d="M7 14H5v5h5v-2H7v-3zm-2-4h2V7h3V5H5v5zm12 7h-3v2h5v-5h-2v3zM14 5v2h3v3h2V5h-5z"/></svg>
        </button>
      {/if}
      <button class="winbtn close" data-tip={t('bar.close')} aria-label={t('bar.close')} onclick={onClose}>
        <svg viewBox="0 0 10 10"><path stroke="currentColor" d="M0 0l10 10M10 0L0 10"/></svg>
      </button>
    </div>
  {/if}
  {#if IS_MAC}
    <div class="brand brand-right">{@render brandMark()}</div>
  {/if}
  </div>
</div>

<!-- Live torrent readout, and it closes a real gap rather than decorating:
     `opening` is cleared on `file-loaded`, so a torrent that runs out of
     pieces *mid-playback* used to freeze the picture with nothing on screen
     to say why. Deliberately OUTSIDE `.topbar` — like the skip button and for
     the same reason, it has to survive the UI going idle, which is exactly
     when a stall happens. It follows the chrome the rest of the time, so the
     picture stays clean while the swarm is keeping up.

     Top-right, because top-left is where `.osd` appears — the popup for every
     volume change and seek — and it would cover this outright. -->
{#if torrentChip}
  <div
    class="torchip"
    class:hidden={idle && !player.stalled}
    class:waiting={player.stalled}
  >
    <div class="torchip-line">
      <svg class="torchip-icon" viewBox="0 0 16 16" aria-hidden="true">
        <path
          d="M8 2.5v8m0 0L4.8 7.3M8 10.5l3.2-3.2M3 13.5h10"
          fill="none"
          stroke="currentColor"
          stroke-width="1.6"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
      <!-- The same sentence the loading overlay uses, not a second copy of
           the logic: it already separates "no peers at all" from "connecting"
           from a rate, and those distinctions are worth exactly as much here.
           Stalling outranks it, because that is what the viewer is looking
           at the chip to find out. -->
      <span>{player.stalled ? t('load.stalled') : (torrentLabel ?? '')}</span>
    </div>
    {#if torrentChip.file_size > 0}
      <div class="torchip-bar">
        <span style="width: {(torrentChip.file_done / torrentChip.file_size) * 100}%"></span>
      </div>
    {/if}
  </div>
{/if}

<!-- Watching together, and the one thing about it that must survive the UI
     going idle: whether the room is waiting for somebody. That is exactly when
     it happens — a torrent runs out of pieces while nobody is touching the
     keyboard — and it is the difference between "the film stopped" and "the
     film is waiting for Anna". Outside `.topbar` for the same reason the
     torrent readout and the skip button are.

     Stacked below the torrent chip rather than beside it: both are top-right
     (top-left is where `.osd` appears, and it would cover either outright), and
     when a torrent is feeding a shared room both are legitimately up. -->
{#if wire.on}
  <div
    class="roomchip"
    class:below={!!torrentChip}
    class:hidden={idle && !wire.waiting.length}
    class:waiting={wire.waiting.length > 0}
  >
    <div class="roomchip-line">
      <svg class="roomchip-icon" viewBox="0 0 16 16" aria-hidden="true">
        <path
          d="M6 7.2a2 2 0 1 0 0-4 2 2 0 0 0 0 4Zm5.2.6a1.6 1.6 0 1 0 0-3.2 1.6 1.6 0 0 0 0 3.2ZM1.8 13c0-2.1 1.9-3.4 4.2-3.4S10.2 10.9 10.2 13m1.2-3.2c1.7.2 3 1.2 3 2.9"
          fill="none"
          stroke="currentColor"
          stroke-width="1.4"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
      <span>
        {#if sync.holdingUp}
          {t('sync.waiting_you')}
        {:else if wire.waiting.length === 1}
          {t('sync.waiting_one', { name: wire.waitingFor[0]?.name || t('sync.you') })}
        {:else if wire.waiting.length > 1}
          {t('sync.waiting_many', { count: wire.waiting.length })}
        {:else}
          {t('sync.room')} {formatCode(wire.room)}
        {/if}
      </span>
    </div>
  </div>
{/if}

<style>
  .topbar {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    /* Three layers: brand on the left, video title centered, window buttons in
       the corner. The height is exactly the 48px of content, and that is the
       HIT AREA: the gradient tail lives in ::before, which catches no events.
       The bar used to occupy all 76px and swallowed clicks in the bottom 28px
       (nothing but a faint shadow there) — to the user that looked like
       clicking the video and the player ignoring it. The children
       (.brand/.titlecenter/.chrome) are absolutely positioned within 48px
       anyway, so they do not depend on the bar's height. */
    height: 48px;
    transition: opacity 0.25s ease;
  }

  .topbar::before {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    height: 76px;
    background: linear-gradient(to bottom, rgba(0, 0, 0, 0.6), transparent);
    pointer-events: none;
  }

  .brand {
    position: absolute;
    top: 0;
    left: 18px;
    height: 48px;
    display: flex;
    align-items: center;
    gap: 8px;
    pointer-events: none;
  }

  /* macOS: the window buttons are the system ones on the left, so the brand
     moves to the right corner and flips: caption first, then the mark. */
  .brand.brand-right {
    position: static;
    height: auto;
    flex-direction: row-reverse;
    padding-right: 18px;
  }

  .brand .logo {
    width: 18px;
    height: 18px;
    color: #818cf8;
    filter: var(--ui-shadow-drop);
  }

  .brand .appname {
    /* The same color as the video title: the muted #b9b9c3 read as
       semi-transparent and disappeared on a bright frame. */
    color: #e8e8ec;
    font-size: 13px;
    font-weight: 500;
    letter-spacing: 0.02em;
    text-shadow: var(--ui-shadow);
  }

  .titlecenter {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: 48px;
    display: flex;
    align-items: center;
    justify-content: center;
    pointer-events: none;
  }

  .topbar.hidden {
    opacity: 0;
    pointer-events: none;
  }

  /* **Right**, and that is not a preference: `.osd` — the popup every volume
     change, seek and speed change raises — is `top: 60px; left: 18px`, which is
     within two pixels of where this sat. A status readout that the next volume
     nudge covers completely is worse than no readout, because it is there right
     up until the moment you look. Nothing else occupies this corner: the window
     buttons and the macOS logo end at the bar's 48px, and the skip button is
     bottom-right.

     Never interactive — it reports, it does not offer anything, and a readout
     that eats clicks over video is the gotcha the top bar's own gradient
     already taught once. */
  .torchip {
    position: absolute;
    top: 58px;
    right: 18px;
    display: flex;
    flex-direction: column;
    gap: 5px;
    /* Outer, like every size here: 148 of content plus 22 of padding and 2 of
       border. Wide enough that the rate and the peer count changing width does
       not make the chip breathe. */
    min-width: 172px;
    padding: 7px 11px 8px;
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 9px;
    background: rgba(16, 16, 22, 0.82);
    box-shadow: 0 2px 10px rgba(0, 0, 0, 0.4);
    color: #d6d6de;
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
    pointer-events: none;
    transition: opacity 0.25s ease;
  }

  .torchip.hidden {
    opacity: 0;
  }

  /* Stalled is the one state that stays up through idle, so it says so in more
     than words: the accent is what the rest of the UI uses for "this is the
     thing to look at". */
  .torchip.waiting {
    color: #e8e8ec;
    border-color: rgba(129, 140, 248, 0.45);
  }

  .torchip-line {
    display: flex;
    align-items: center;
    gap: 7px;
    white-space: nowrap;
  }

  .torchip-icon {
    /* An SVG has no border, so the box-sizing reset is a no-op here — but
       `flex: none` still matters, or a long label squashes the arrow into an
       oval. */
    flex: none;
    width: 12px;
    height: 12px;
    color: #8f8f9c;
  }

  .torchip.waiting .torchip-icon {
    color: #818cf8;
    /* The arrow stops meaning "downloading" when nothing is arriving. */
    animation: torchip-pulse 1.4s ease-in-out infinite;
  }

  .torchip-bar {
    height: 2px;
    border-radius: 1px;
    background: rgba(255, 255, 255, 0.14);
    overflow: hidden;
  }

  .torchip-bar span {
    display: block;
    height: 100%;
    border-radius: 1px;
    /* White, not the accent: this reports a value rather than offering one —
       the same reason `.seekfill` is white. */
    background: rgba(255, 255, 255, 0.75);
    transition: width 0.3s ease;
  }

  /* Same box as `.torchip` and deliberately so — two readouts in one corner
     that did not match would read as two unrelated bugs. Only the position and
     the "why it stays up" rule differ. */
  .roomchip {
    position: absolute;
    top: 58px;
    right: 18px;
    display: flex;
    flex-direction: column;
    gap: 5px;
    padding: 7px 11px 8px;
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 9px;
    background: rgba(16, 16, 22, 0.82);
    box-shadow: 0 2px 10px rgba(0, 0, 0, 0.4);
    color: #d6d6de;
    font-size: 11.5px;
    /* A readout, not a control: the gotcha the top bar's gradient already
       taught once is that anything over the video which eats clicks reads as
       the player ignoring you. */
    pointer-events: none;
    transition: opacity 0.25s ease;
  }

  /* The torrent chip's own height plus the gap between them. A fixed number
     rather than a flex column, because the two are siblings of the page root
     and only one of them is ever conditional on the other. */
  .roomchip.below {
    top: 106px;
  }

  .roomchip.hidden {
    opacity: 0;
  }

  /* Waiting is the state that outlives idle, and it says so in more than words:
     the accent is what the rest of the player uses for "look here". */
  .roomchip.waiting {
    color: #e8e8ec;
    border-color: rgba(129, 140, 248, 0.45);
  }

  .roomchip-line {
    display: flex;
    align-items: center;
    gap: 7px;
    white-space: nowrap;
  }

  .roomchip-icon {
    flex: none;
    width: 13px;
    height: 13px;
    color: #8f8f9c;
  }

  .roomchip.waiting .roomchip-icon {
    color: #818cf8;
    animation: torchip-pulse 1.4s ease-in-out infinite;
  }

  /* With no video the gradient is pointless — there is nothing to darken. It
     has to be targeted through ::before, because `.topbar` itself has no
     background (see the note on the scrim above).

     This used to read `.player.no-video .topbar::before`, with the state on the
     player root. That cannot survive the bar becoming a component — Svelte
     scopes the selector, and `.player` belongs to the page — so the mode comes
     down as a prop and is written on the bar's own element. */
  .topbar.no-video::before {
    background: none;
  }

  .topbar .title {
    transition:
      opacity 0.25s ease,
      transform 0.25s ease;
  }

  /* Slide: "next" sends the old title left and brings the new one in from the
     right; "prev" is mirrored */
  .topbar .title.slide-out-next {
    opacity: 0;
    transform: translateX(-28px);
  }

  .topbar .title.slide-out-prev {
    opacity: 0;
    transform: translateX(28px);
  }

  .topbar .title.slide-prep-next {
    opacity: 0;
    transform: translateX(28px);
    transition: none;
  }

  .topbar .title.slide-prep-prev {
    opacity: 0;
    transform: translateX(-28px);
    transition: none;
  }

  .topbar .title {
    /* Bounded by what is actually free: the window minus the wider of the two
       side clusters on BOTH sides (the title is centered in the window, so the
       reservation has to be symmetric), minus a gap so it never arrives flush
       against the logo. `--bar-side` is measured in JS and already carries that
       gap — see the comment there for why a constant does not survive an update
       button or a translation. A calc() that goes negative is clamped to 0, so
       a window too narrow for both clusters hides the title rather than
       dropping the bound and letting it overlap them. */
    max-width: min(46vw, calc(100vw - 2 * var(--bar-side, 0px)));
    color: #e8e8ec;
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    /* overflow: hidden is needed for the ellipsis, but it also clipped the
       shadow at the ends of the line. Vertical padding gives it room inside the
       block; the element is centered vertically by flex, so the text does not
       shift. */
    padding: 8px 0;
    text-shadow: var(--ui-shadow);
  }

  .chrome {
    position: absolute;
    top: 0;
    right: 0;
    height: 48px;
    display: flex;
    align-items: center;
  }

  /* Full-height title-bar buttons (Spotify style) */
  .winbtns {
    display: flex;
    height: 100%;
  }

  /* Fills as the download lands. The percentage is in the label already; the
     fill is what makes it readable at a glance on a bar the eye passes over. */
  .updbtn.progressing {
    background-image: linear-gradient(rgba(255, 255, 255, 0.28), rgba(255, 255, 255, 0.28));
    background-repeat: no-repeat;
    background-size: var(--pct, 0%) 100%;
    transition: background-size 0.25s linear;
  }

  .updbtn {
    align-self: center;
    margin-right: 8px;
    background: #6366f1;
    color: #ffffff;
    border: none;
    border-radius: 999px;
    padding: 4px 12px;
    font-size: 12px;
    font-weight: 500;
    letter-spacing: 0.02em;
    cursor: pointer;
    white-space: nowrap;
  }

  .updbtn:hover:not(:disabled) {
    background: #818cf8;
  }

  .updbtn:disabled {
    opacity: 0.75;
    cursor: default;
  }

  .winbtn {
    /* a square gives equal gaps around the icon on every side */
    width: 48px;
    height: 100%;
    display: grid;
    place-items: center;
    background: transparent;
    border: none;
    color: #d6d6de;
    padding: 0;
  }

  .winbtn svg {
    width: 10px;
    height: 10px;
    /* The same shadow as the logo and the titles: the glyphs sit directly over
       the frame, and without it the light gray lines are lost on bright video.
       drop-shadow rather than text-shadow — the shadow must follow the shape's
       outline, not the element's rectangle. */
    filter: var(--ui-shadow-drop);
  }

  /* On hover the button gets a background of its own, which provides the
     contrast — a shadow on top of it only muddies the glyph (especially on the
     red close button). */
  .winbtn:hover svg {
    filter: none;
  }

  /* The enter/exit fullscreen glyphs fill ~60% of their viewBox — compensate
     with size so they match their neighbours optically */
  .winbtn.fsx svg {
    width: 17px;
    height: 17px;
  }

  .winbtn:hover {
    background: rgba(255, 255, 255, 0.12);
    color: #fff;
  }

  .winbtn.close:hover {
    background: #c42b1c;
    color: #fff;
  }

  /* Mini: a 420px window is mostly picture, so the chrome has to earn its
     pixels. The title bar goes entirely (its own drag region goes with it, but
     an always-on-top window in a corner is moved by dragging the video, which
     already works), the bar keeps play/pause and the seek row, and everything
     that belongs to a full-sized session — clusters of toggles, menus,
     timecodes — stands down. Nothing is hidden that cannot be reached another
     way: the context menu is untouched. */
  .topbar.mini {
    display: none;
  }
  @keyframes torchip-pulse {
    0%, 100% { opacity: 0.35; }
    50% { opacity: 1; }
  }
</style>
