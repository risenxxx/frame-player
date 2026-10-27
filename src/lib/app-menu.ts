/**
 * What the native menu bar asks for (`frameplayer://menu`, macOS — the items
 * are built in `macos_menu.rs`), answered with the same verbs the buttons and
 * the hotkeys use. Out of the page's script: it is a table from an item's id to
 * a call, and nothing in it is the page's.
 */

import { revealItemInDir } from '@tauri-apps/plugin-opener';
import { toggleFullscreen } from './chrome.svelte';
import { openFileDialog, openLinkDialog } from './open.svelte';
import { overlays, toggleInfo } from './overlays.svelte';
import { isNetworkSource, jumpChapter, player } from './player.svelte';
import {
  fitWindowToVideo,
  setWindowFit,
  toggleMini,
  toggleWindowPref,
  windowFit,
} from './window-prefs.svelte';

export function runMenuAction(id: string) {
  switch (id) {
    case 'settings': overlays.settings = true; break;
    case 'open': void openFileDialog(); break;
    case 'open_link': void openLinkDialog(); break;
    case 'info': toggleInfo(player.hasFile); break;
    case 'mini': void toggleMini(); break;
    case 'chapter_prev': jumpChapter(-1); break;
    case 'chapter_next': jumpChapter(1); break;
    case 'reveal':
      if (player.filePath && !isNetworkSource(player.filePath)) {
        void revealItemInDir(player.filePath);
      }
      break;
    case 'fullscreen': void toggleFullscreen(); break;
    case 'win_remember': toggleWindowPref('remember'); break;
    // Two check marks over one choice of three (`WindowFit`): each one
    // unticked goes back to the middle of the three, or to nothing.
    case 'win_fit': setWindowFit(windowFit() === 'free' ? 'shape' : 'free'); break;
    case 'win_video_size': setWindowFit(windowFit() === 'size' ? 'shape' : 'size'); break;
    case 'win_ontop': toggleWindowPref('alwaysOnTop'); break;
    case 'win_snap': toggleWindowPref('snapMini'); break;
    case 'win_size_50': void fitWindowToVideo(0.5); break;
    case 'win_size_100': void fitWindowToVideo(1); break;
    case 'win_size_200': void fitWindowToVideo(2); break;
  }
}
