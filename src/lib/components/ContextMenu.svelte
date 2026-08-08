<script lang="ts">
  /// The right-click menu.
  ///
  /// Grouped rather than listed: anything with more than two related entries
  /// (file, frame, playback, picture, window) is a submenu, which took it from
  /// ~19 rows to 11 — a menu that had to scroll in an ordinary window was the
  /// sign it had outgrown a flat list. What stays at the top level is what has
  /// no group and is reached often.
  ///
  /// The submenu machinery lives here because nothing outside needs it: which
  /// panel is open, where it lands, and the hover bridge that keeps it open
  /// across the gap are all this menu's business. What the menu cannot do for
  /// itself arrives in `actions` — every one of those reaches into the player.
  import type { Snippet } from 'svelte';
  import { tick } from 'svelte';
  import { openUrl, revealItemInDir } from '@tauri-apps/plugin-opener';
  import { setProperty } from 'tauri-plugin-libmpv-api';

  import { flipAxis, shiftAxis } from '$lib/floating';
  import { t } from '$lib/i18n.svelte';
  import { hint } from '$lib/keys.svelte';
  import { showOsd } from '$lib/osd.svelte';
  import { IS_MAC } from '$lib/platform';
  import { ASPECT_AUTO, LOOP_LABEL, isNetworkSource, player } from '$lib/player.svelte';
  import { blockContextMenu } from '$lib/dom';
  import { copyScreenshot, saveScreenshot } from '$lib/screenshot';
  import { openSubsDialog } from '$lib/subs.svelte';
  import {
    fitWindowToVideo,
    mini,
    toggleMini,
    toggleWindowPref,
    windowPrefs,
  } from '$lib/window-prefs.svelte';

  interface Props {
    /// Where the right-click landed. The menu places itself from it.
    at: { x: number; y: number };
    fullscreen: boolean;
    close: () => void;
    /// Everything the menu cannot do by itself. One object rather than a dozen
    /// props: these are all "reach into the player", and naming them
    /// individually would put the same list in three places.
    actions: {
      openFile: () => void;
      openLink: () => void;
      backToStart: () => void;
      toggleInfo: () => void;
      openSettings: () => void;
      toggleFullscreen: () => void;
      cycleLoop: () => void;
      cycleAbLoop: () => void;
      jumpChapter: (delta: -1 | 1) => void;
      setPicture: (
        prop: 'video-rotate' | 'video-aspect-override' | 'panscan',
        value: string,
      ) => void;
    };
  }

  let { at, fullscreen, close, actions }: Props = $props();

  const hasFile = $derived(player.hasFile);

  /// The aspect overrides the menu offers. mpv reports an override as a decimal
  /// (`4:3` reads back as 1.333333), so the menu compares numerically rather
  /// than against the string it sent; anything negative is auto, and mpv's own
  /// default is -2.
  const ASPECTS = [
    { label: '16:9', ratio: 16 / 9 },
    { label: '4:3', ratio: 4 / 3 },
    { label: '2.35:1', ratio: 2.35 },
  ];

  /// The menu's own root, measured to place both the menu and its submenus.
  let ctxEl = $state<HTMLDivElement | undefined>();

  /// The correction applied once the menu has a size to measure, or null before
  /// that. Placed from the menu's real height rather than an estimate —
  /// eyeballed ones go stale as items are added — so it is render → measure →
  /// correct, all before paint. The height cap is the part a clamp alone cannot
  /// do: in the mini window the menu is taller than the whole player, so no
  /// position shows all of it and it has to scroll instead.
  let placed = $state<{ x: number; y: number; maxH: number } | null>(null);

  /// Falls back to the raw click point for the one frame before the
  /// measurement, which is what the page did before this moved into a
  /// component. Derived rather than captured: right-clicking while the menu is
  /// already up hands it a new point, and Svelte may reuse this component
  /// rather than remount it — read once at creation, the menu would stay where
  /// the previous click put it.
  const at_ = $derived(placed ?? { x: at.x, y: at.y, maxH: window.innerHeight });

  $effect(() => {
    const point = at;
    placed = null;
    void tick().then(() => {
      if (!ctxEl) return;
      const v = flipAxis({
        near: point.y,
        far: point.y,
        size: ctxEl.offsetHeight,
        limit: window.innerHeight,
        preferBefore: false,
      });
      placed = {
        x: shiftAxis(point.x, ctxEl.offsetWidth, window.innerWidth),
        y: v.pos,
        maxH: v.room,
      };
    });
  });

  /// Which submenu of the context menu is open. An id rather than a boolean
  /// since there are two of them, and only ever one at a time — so the panel
  /// itself still needs a single ref, while the ANCHORS need one each: every
  /// head is always in the DOM, and sharing a ref would leave it pointing at
  /// whichever rendered last rather than at the open one.
  type CtxSub = 'file' | 'frame' | 'playback' | 'picture' | 'window';
  let ctxSubmenu = $state<CtxSub | null>(null);
  let ctxSubEl = $state<HTMLDivElement | undefined>();
  /// One anchor per head, keyed by submenu. A plain object rather than $state:
  /// it is read imperatively by placeSubmenu after a tick, never rendered.
  const ctxWraps: Partial<Record<CtxSub, HTMLDivElement>> = {};
  /// Submenu offset from its parent item, in pixels. Computed after render
  /// from the actual size: eyeballed estimates go stale as items are added —
  /// the same scheme as the clamping of the context menu itself.
  let ctxSubPos = $state<{ left: number; top: number; maxH: number } | null>(null);

  /// Below this window width a menu and its submenu cannot stand side by side
  /// (two 256px menus plus their margins, with room for content that pushes
  /// one of them wider), so the submenu covers the parent instead.
  const SIDE_BY_SIDE_MIN = 600;

  /// Drill-down mode: the submenu takes the parent's place instead of standing
  /// beside it. Decided once, when the menu opens, because it governs how
  /// submenus are *opened* and not only where they land — and a window resized
  /// underneath an open menu is not a case worth chasing.
  ///
  /// `const`, not `$state`: this component is rendered inside `{#if ctxAt}`, so
  /// it mounts afresh on every right-click and the initializer is what re-reads
  /// the width. Nothing assigns it after that, and saying `$state` would claim
  /// otherwise.
  const ctxDrill = window.innerWidth < SIDE_BY_SIDE_MIN;

  let ctxSubTimer: ReturnType<typeof setTimeout> | undefined;

  function openSubmenu(id: CtxSub) {
    clearTimeout(ctxSubTimer);
    if (ctxSubmenu === id) return;
    ctxSubmenu = id;
    void placeSubmenu();
  }

  /// Delayed close — otherwise the submenu collapses right under the cursor.
  /// The path from the item to the submenu is diagonal and briefly crosses the
  /// gap between them, which belongs to neither, so mouseleave concludes the
  /// cursor left entirely. The delay is enough for the cursor to arrive;
  /// entering either area cancels it.
  function closeSubmenuSoon() {
    clearTimeout(ctxSubTimer);
    ctxSubTimer = setTimeout(() => (ctxSubmenu = null), 220);
  }

  /// Hover opens a submenu only when there is room for it beside the menu.
  /// In the mini player the submenu covers the parent, so opening on hover
  /// means the parent is unusable: every pointer move across it lands on
  /// another submenu head, and the menu cannot even be scrolled. There the
  /// submenu is a place you go to deliberately and come back from.
  function hoverSubmenu(which: CtxSub) {
    if (!ctxDrill) openSubmenu(which);
  }

  function hoverLeaveSubmenu() {
    if (!ctxDrill) closeSubmenuSoon();
  }

  function closeSubmenuNow() {
    clearTimeout(ctxSubTimer);
    ctxSubmenu = null;
  }

  async function placeSubmenu() {
    ctxSubPos = null;
    await tick();
    const wrapEl = ctxSubmenu ? ctxWraps[ctxSubmenu] : undefined;
    if (!ctxSubEl || !wrapEl) return;
    const sub = ctxSubEl.getBoundingClientRect();
    const wrap = wrapEl.getBoundingClientRect();

    // Viewport coordinates, not the wrapper's: the parent menu scrolls when it
    // is taller than the window, and `overflow` clips descendants — an
    // absolutely positioned submenu inside it would be cut off at the parent's
    // own edge. Fixed positioning takes it out of that box, and the parent
    // closes it on scroll rather than dragging it along.
    const h = flipAxis({
      near: wrap.left,
      far: wrap.right,
      size: sub.width,
      limit: window.innerWidth,
      gap: 4,
      preferBefore: false,
    });
    // Neither side had room — which is the normal case in the mini player,
    // where the window is narrower than two menus side by side. Half-overlapping
    // the parent there reads as a menu growing out of its own middle and being
    // clipped by it; so the submenu covers the parent instead, aligned to it,
    // which is the drill-down every small-screen menu uses. `room` is what the
    // chosen side could actually give.
    const menu = ctxEl?.getBoundingClientRect();
    const cramped = (ctxDrill || h.room < sub.width) && !!menu;
    ctxSubPos = {
      left: cramped ? shiftAxis(menu.left, sub.width, window.innerWidth) : h.pos,
      // Level with the item, slid up only as far as it takes to fit — or with
      // the parent's own top when it is standing in for the parent.
      top: shiftAxis(cramped ? menu.top : wrap.top - 6, sub.height, window.innerHeight),
      maxH: window.innerHeight - 16,
    };
  }
</script>

<div
  class="ctxmenu scrollable"
  class:masked={ctxDrill && !!ctxSubmenu}
  bind:this={ctxEl}
  style="left: {at_.x}px; top: {at_.y}px; max-height: {at_.maxH}px"
  role="menu"
  tabindex="-1"
  onscroll={closeSubmenuNow}
  onclick={(e) => e.stopPropagation()}
  ondblclick={(e) => e.stopPropagation()}
  oncontextmenu={blockContextMenu}
>
  <!-- Grouped rather than listed: anything with more than two related
       entries becomes a submenu, which takes the menu from ~19 rows to 11
       and makes it fit windows it used to have to scroll in. What stays at
       the top level is what has no group and is reached often — the two
       size modes, the settings. -->
  {@render submenuHead('file', t('ctx.file'), false)}
  {@render submenuHead('frame', t('ctx.frame'), !hasFile)}
  {@render submenuHead('playback', t('ctx.playback'), !hasFile)}
  {@render submenuHead('picture', t('ctx.picture'), !hasFile)}
  <div class="menu-sep"></div>
  <button class="menu-item" onclick={() => { close(); actions.toggleFullscreen(); }}>
    {t('ctx.fullscreen')} <span class="hint">{hint('fullscreen')}</span>
  </button>
  <button class="menu-item" class:active={mini.on} disabled={!hasFile} onclick={() => { close(); void toggleMini(); }}>
    {t('ctx.mini')} <span class="hint">{hint('mini')}</span>
  </button>
  {@render submenuHead('window', t('ctx.window'), false)}
  <div class="menu-sep"></div>
  <button class="menu-item" onclick={() => { close(); actions.openSettings(); }}>
    {t('ctx.settings')}
  </button>
  <!-- Windows only: ms-settings: is a Windows scheme, and on macOS this
       item silently did nothing. macOS has no single-link equivalent (the
       default app is chosen in a specific file's Get Info panel). -->
  {#if !IS_MAC}
    <button
      class="menu-item"
      onclick={() => { close(); void openUrl('ms-settings:defaultapps'); }}
    >
      {t('ctx.default_apps')}
    </button>
  {/if}
</div>
<!-- The panels are rendered OUTSIDE the menu, not inside the item that
     opens them. The menu scrolls when it is taller than the window, and a
     scroll container is a clip: a panel inside it can only be as wide as
     what is left of the menu. Placement is in viewport coordinates either
     way (placeSubmenu), so nothing but the clip depended on the nesting —
     and the hover bridge is geometric, so it does not either. -->
{@render submenuPanel('file', fileItems)}
{@render submenuPanel('frame', frameItems)}
{@render submenuPanel('playback', playbackItems)}
{@render submenuPanel('picture', pictureItems)}
{@render submenuPanel('window', windowItems)}

{#snippet submenuHead(key: CtxSub, label: string, disabled: boolean)}
  <!-- Opens on hover and on click — the click is for keyboard and for
       trackpads without hover, and it is the ONLY way in when the window is
       too narrow for a panel beside the menu (see hoverSubmenu). -->
  <div
    class="submenu-wrap"
    role="presentation"
    bind:this={ctxWraps[key]}
    onmouseenter={() => hoverSubmenu(key)}
    onmouseleave={hoverLeaveSubmenu}
  >
    <button
      class="menu-item submenu-head"
      class:open={ctxSubmenu === key}
      {disabled}
      onclick={() => { if (ctxSubmenu === key) closeSubmenuNow(); else openSubmenu(key); }}
    >
      <span>{label}</span>
      <svg class="caret" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
        <path
          d="M6 3.5 10.5 8 6 12.5"
          fill="none"
          stroke="currentColor"
          stroke-width="1.6"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
    </button>
  </div>
{/snippet}

{#snippet submenuPanel(key: CtxSub, body: Snippet)}
  {#if ctxSubmenu === key}
    <div
      class="ctxmenu submenu scrollable"
      role="menu"
      tabindex="-1"
      bind:this={ctxSubEl}
      onmouseenter={() => hoverSubmenu(key)}
      onmouseleave={hoverLeaveSubmenu}
      onclick={(e) => e.stopPropagation()}
      ondblclick={(e) => e.stopPropagation()}
      oncontextmenu={blockContextMenu}
      style={ctxSubPos
        ? `left: ${ctxSubPos.left}px; top: ${ctxSubPos.top}px; max-height: ${ctxSubPos.maxH}px`
        : 'left: 0; top: 0; visibility: hidden'}
    >
      {@render submenuBack()}
      {@render body()}
    </div>
  {/if}
{/snippet}

{#snippet fileItems()}
  <button class="menu-item" onclick={() => { close(); actions.openFile(); }}>
    {t('ctx.open')} <span class="hint">{hint('open_file')}</span>
  </button>
  <button class="menu-item" onclick={() => { close(); actions.openLink(); }}>
    {t('ctx.open_link')} <span class="hint">{hint('open_link')}</span>
  </button>
  <button
    class="menu-item"
    disabled={!player.filePath || isNetworkSource(player.filePath)}
    onclick={() => { close(); if (player.filePath) void revealItemInDir(player.filePath); }}
  >
    {t(IS_MAC ? 'ctx.reveal_mac' : 'ctx.reveal_win')}
  </button>
  <button class="menu-item" disabled={!hasFile} onclick={() => { close(); actions.toggleInfo(); }}>
    {t('ctx.info')} <span class="hint">{hint('info')}</span>
  </button>
  <!-- Named for where it goes rather than what it does to mpv: "stop" is the
       mechanism, and the reason anyone reaches for it is to get back to the
       list of what they were watching. Everything else follows from unloading
       the file — the `filename === null` branch of the property hook already
       commits the position, drops the posters, leaves the mini player and
       releases the torrent. -->
  <button class="menu-item" disabled={!hasFile} onclick={() => { close(); actions.backToStart(); }}>
    {t('ctx.back_to_start')}
  </button>
{/snippet}

{#snippet frameItems()}
  <button class="menu-item" disabled={!hasFile} onclick={() => { close(); void saveScreenshot(false); }}>
    {t('ctx.shot')} <span class="hint">{hint('screenshot')}</span>
  </button>
  <!-- Only worth offering while subtitles are actually on: otherwise the
       two variants would save byte-identical files. -->
  {#if player.subTracks.some((track) => track.selected)}
    <button class="menu-item" disabled={!hasFile} onclick={() => { close(); void saveScreenshot(true); }}>
      {t('ctx.shot_subs')} <span class="hint">{hint('screenshot_subs')}</span>
    </button>
  {/if}
  <button class="menu-item" disabled={!hasFile} onclick={() => { close(); void copyScreenshot(); }}>
    {t('ctx.shot_copy')} <span class="hint">{hint('copy_frame')}</span>
  </button>
{/snippet}

{#snippet playbackItems()}
  <button class="menu-item" class:active={player.loopMode !== 'off'} disabled={!hasFile} onclick={() => { close(); actions.cycleLoop(); }}>
    {t(LOOP_LABEL[player.loopMode])} <span class="hint">{hint('loop')}</span>
  </button>
  <!-- Only for files that have chapters, which is a minority — but on
       Windows there is no menu bar, so this is the only place the shortcut
       is ever spelled out. -->
  {#if player.hasChapters}
    <button class="menu-item" onclick={() => { close(); actions.jumpChapter(-1); }}>
      {t('ctx.chapter_prev')} <span class="hint">{hint('chapter_prev')}</span>
    </button>
    <button class="menu-item" onclick={() => { close(); actions.jumpChapter(1); }}>
      {t('ctx.chapter_next')} <span class="hint">{hint('chapter_next')}</span>
    </button>
  {/if}
  <!-- Stays open: A and B are set one after the other, and closing after the
       first mark would mean opening the menu again for the second. -->
  <button class="menu-item" class:active={player.loopA !== null} disabled={!hasFile} onclick={() => actions.cycleAbLoop()}>
    {t(player.loopA === null ? 'ctx.ab_set_a' : player.loopB === null ? 'ctx.ab_set_b' : 'ctx.ab_clear')}
    <span class="hint">{hint('ab_loop')}</span>
  </button>
  <div class="menu-title">{t('ctx.speed')}</div>
  <div class="speedrow">
    {#each [0.5, 1, 1.25, 1.5, 2] as s (s)}
      <button
        class="speedopt"
        class:sel={Math.abs(player.speed - s) < 0.01}
        onclick={() => { close(); void setProperty('speed', s); showOsd(t('osd.speed', { value: s }), { progress: (s - 0.25) / 3.75 }); }}
      >
        {s}×
      </button>
    {/each}
  </div>
{/snippet}

{#snippet pictureItems()}
  <!-- Every option here stays open: these are tried against the picture and
       compared, not chosen once from a list — the same reasoning as the
       delay stepper in the track menus. -->
  <div class="menu-title">{t('ctx.rotate')}</div>
  <div class="speedrow">
    {#each [0, 90, 180, 270] as deg (deg)}
      <button
        class="speedopt"
        class:sel={player.videoRotate === deg}
        onclick={() => actions.setPicture('video-rotate', String(deg))}
      >
        {deg}°
      </button>
    {/each}
  </div>
  <div class="menu-title">{t('ctx.aspect')}</div>
  <div class="speedrow">
    <!-- Compared numerically: mpv answers "4:3" with 1.333333, so the
         string that was sent is not what comes back. -->
    <button
      class="speedopt"
      class:sel={player.aspectOverride < 0}
      onclick={() => actions.setPicture('video-aspect-override', ASPECT_AUTO)}
    >
      {t('ctx.aspect_auto')}
    </button>
    {#each ASPECTS as a (a.label)}
      <button
        class="speedopt"
        class:sel={Math.abs(player.aspectOverride - a.ratio) < 0.01}
        onclick={() => actions.setPicture('video-aspect-override', a.label)}
      >
        {a.label}
      </button>
    {/each}
  </div>
  <div class="menu-sep"></div>
  <button
    class="menu-item"
    class:active={player.panscan > 0}
    onclick={() => actions.setPicture('panscan', player.panscan > 0 ? '0' : '1')}
  >
    {t('ctx.panscan')}
  </button>
{/snippet}

{#snippet windowItems()}
  <button class="menu-item" class:active={windowPrefs.remember} onclick={() => toggleWindowPref('remember')}>
    {t('ctx.win_remember')}
  </button>
  <button class="menu-item" class:active={windowPrefs.fitToVideo} onclick={() => toggleWindowPref('fitToVideo')}>
    {t('ctx.win_fit')}
  </button>
  <button class="menu-item" class:active={windowPrefs.alwaysOnTop} onclick={() => toggleWindowPref('alwaysOnTop')}>
    {t('ctx.win_ontop')}
  </button>
  <button class="menu-item" class:active={windowPrefs.snapMini} onclick={() => toggleWindowPref('snapMini')}>
    {t('ctx.win_snap')}
  </button>
  <div class="menu-sep"></div>
  <div class="menu-title">{t('ctx.win_size')}</div>
  <div class="speedrow">
    {#each [0.5, 1, 2] as z (z)}
      <button
        class="speedopt"
        disabled={!hasFile || player.videoW <= 0}
        onclick={() => { close(); closeSubmenuNow(); void fitWindowToVideo(z); }}
      >
        {z * 100}%
      </button>
    {/each}
  </div>
{/snippet}

{#snippet submenuBack()}
  <!-- Only in drill-down: the parent is hidden behind this panel, so without
       a way back the only exit is closing the menu and opening it again. -->
  {#if ctxDrill}
    <button class="menu-item back" onclick={closeSubmenuNow}>
      <svg class="caret" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
        <path d="M10 3.5 5.5 8 10 12.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>
      </svg>
      <span>{t('ctx.back')}</span>
    </button>
    <div class="menu-sep"></div>
  {/if}
{/snippet}

<style>
  .ctxmenu {
    position: fixed;
    z-index: 60;
    /* One minimum for every menu in the app; content widens them from there.
       Wide enough for the five-option speed row, which at 230px gave each
       option 37px and let "1.25×" fill it edge to edge. */
    min-width: 256px;
    /* Taller than the window in the mini player, and on a short screen with a
       long chapter list. The cap comes from the placement (floating.ts), which
       computes it from the room on screen and therefore means the whole box —
       the global border-box reset is what makes the `max-height` it hands over
       mean the same thing. Before the reset this menu rendered 14px taller than
       it was told (6+6 padding, 1+1 border) and ate the 8px margin it was
       meant to keep from the window edge.
       `overflow-x` must be set too: leaving it `visible` next to a scrolling
       axis computes it to `auto`, and a row a fraction of a pixel too wide then
       raises a horizontal bar that eats a row's worth of height. Nothing here
       is meant to scroll sideways. */
    overflow-y: auto;
    overflow-x: hidden;
    background: rgba(16, 16, 22, 0.96);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 10px;
    padding: 6px;
  }

  /* Nested menu. Opens to the right; with no room there it flips left (the
     browser clamps to the window edge itself through right: 100% on .flip). */
  .submenu-wrap {
    position: relative;
  }

  /* Mantine style: the item is a flex row, the chevron pushed to the right by
     space-between and centered on the line. `margin-left: auto` would not work
     here at all — .menu-item is display: block (the neighbouring .hint gets by
     with float, but float gives an icon a crooked baseline).
     The selector must use two classes: the rule `.menu-item { display: block }`
     is declared LOWER in the file, and at equal specificity it would win — flex
     silently did not apply, the chevron stayed right after the text and sat on
     the baseline, i.e. above the center of the line. */
  .menu-item.back {
    display: flex;
    align-items: center;
    gap: 8px;
    color: #a8a8b3;
  }

  .menu-item.back .caret {
    flex: none;
  }

  .menu-item.submenu-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .menu-item.submenu-head .caret {
    flex: none;
    color: #77777f;
    transition: color 0.12s ease;
  }

  .menu-item.submenu-head:hover .caret,
  .menu-item.submenu-head.open .caret {
    color: #d6d6de;
  }

  .menu-item.submenu-head.open {
    background: rgba(255, 255, 255, 0.09);
  }

  /* Drill-down: the submenu stands in for the parent rather than beside it, so
     the parent gets out of the way. `visibility` and not `display`, because the
     placement still measures it — and it works only because the submenu is no
     longer one of its descendants. */
  .ctxmenu.masked {
    visibility: hidden;
  }

  .ctxmenu.submenu {
    /* Fixed, not absolute: the parent scrolls, and a scroll container clips
       what sticks out of it. Placed in viewport coordinates by placeSubmenu. */
    position: fixed;
    /* left/top come from JS, computed from the actual size (placeSubmenu) */
    min-width: 256px;
  }

  /* An invisible bridge across the gap between the item and the submenu:
     extends the hover area by 10px on both sides, so ordinary cursor movement
     never leaves the submenu and the close delay is not even needed. It goes
     under the block itself (z-index: -1) so it does not intercept clicks on the
     items. Works both ways — the submenu can also open to the left when there
     is no room on the right. */
  .ctxmenu.submenu::before {
    content: '';
    position: absolute;
    inset: 0 -10px;
    z-index: -1;
  }

  .menu-item .hint {
    float: right;
    color: #77777f;
    font-size: 12px;
  }

  .menu-item.active {
    color: #818cf8;
  }

  /* segmented control */
  /* 5px of inset and an 11px outer radius, which is the concentric rule the mini
     player's exit button records: an inner radius plus the gap around it. The
     inset used to be 3px and only became visible once a cell filled the row's
     full height — before that the reset's hover floated in the middle and
     nothing touched the edge. Both pill rows carry the same numbers because
     they are one control shape; changing only the one being looked at is how
     two identical-looking rows drift apart. */
  .speedrow {
    display: flex;
    gap: 2px;
    margin: 2px 8px 6px;
    padding: 5px;
    background: rgba(255, 255, 255, 0.07);
    border-radius: 11px;
  }

  /* `.speedopt` itself lives in app.css: the delay stepper in the track menus
     is a sibling component using the same class, and a scoped copy here left
     that one unstyled. */
</style>
