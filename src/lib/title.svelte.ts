/**
 * The name of what is playing: the title bar's centred text, the slide that
 * carries it through a playlist step, and the window's own title (the Dock,
 * the window menu, the taskbar).
 *
 * Out of the page's script. The bar's text is driven by hand rather than
 * straight from `displayTitle`, which is why its state lives here and not in
 * `TopBar.svelte`: what changes it is the mpv bridge (`filename`,
 * `playlist-pos`) and the start screen, neither of which is the bar.
 */

import { getCurrentWindow } from '@tauri-apps/api/window';
import { opening } from './open.svelte';
import { player } from './player.svelte';

class Title {
  /// mpv's own title when it has one; failing that, the name the site gave us
  /// (`resolvedTitle`), and only then the tidied file name — which for a link
  /// is a video id and tells the viewer nothing.
  displayTitle = $derived(
    player.mediaTitle ? player.displayTitle : (opening.resolvedTitle ?? player.displayTitle),
  );
  // The video title (centered in the title bar) is driven by hand rather than
  // straight from displayTitle — the text only changes inside the slide.
  barTitleText = $state('');
  // Slide phases: out (leaving) / prep (starting pose of the new one) / ''
  titleSlide = $state('');
  slideDir: 'next' | 'prev' = 'next';
  /// The playlist moved, so the next title change is a step through it rather
  /// than a file being opened.
  playlistStepped = false;
  /// Previous playlist position, kept because the slide direction is decided by
  /// comparing against it — and by the time this runs the mirror is already the
  /// new value.
  lastPlaylistPos = 0;
}

export const title = new Title();

let slideTimer: ReturnType<typeof setTimeout> | undefined;

/// The title slide says "the playlist moved" — one name leaves in the
/// direction of travel and the next arrives from the other side. Opening a
/// file is not that: there is nothing to push out of the way, and the slide
/// both misreads as a playlist step and delays the name by a quarter second.
/// So it only runs when the playlist actually stepped and there is an old
/// name on screen to replace.
export function slideTitle() {
  clearTimeout(slideTimer);
  if (!title.playlistStepped || !title.barTitleText) {
    title.playlistStepped = false;
    title.titleSlide = '';
    title.barTitleText = title.displayTitle;
    return;
  }
  title.playlistStepped = false;
  const dir = title.slideDir;
  title.titleSlide = dir === 'next' ? 'out-next' : 'out-prev';
  slideTimer = setTimeout(() => {
    title.barTitleText = title.displayTitle;
    title.titleSlide = dir === 'next' ? 'prep-next' : 'prep-prev';
    requestAnimationFrame(() => {
      requestAnimationFrame(() => (title.titleSlide = ''));
    });
  }, 260);
}

/// Debounced on purpose: opening a file changes the title two or three times
/// in quick succession (cleared, then the filename, then the container's own
/// title), and each one is a native title-bar relayout on the main thread
/// while it is already busy opening the file. One call after things settle is
/// all the window menu and the Dock ever needed.
///
/// This used to matter far more — every setTitle knocked the macOS traffic
/// lights back to their default spots and the correction was visible. That is
/// handled structurally now (macos_chrome.rs), so the debounce is a courtesy,
/// not a workaround.
let titleTimer: ReturnType<typeof setTimeout> | undefined;

/// The window title's standing effect. Call during a component's
/// initialization — see the `$effect` rule in docs/rules/frontend.md.
export function initWindowTitle() {
  $effect(() => {
    const next = player.hasFile ? `${title.displayTitle} — Frame Player` : 'Frame Player';
    clearTimeout(titleTimer);
    titleTimer = setTimeout(() => {
      void getCurrentWindow().setTitle(next).catch(() => {});
    }, 250);
  });
}

/// A title write still waiting out its debounce is dropped with the page.
export function clearTitleTimer() {
  clearTimeout(titleTimer);
}
