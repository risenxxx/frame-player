/**
 * What is under the interface when there is no picture: the start screen, and
 * the dark fill that stands in for mpv until it paints.
 *
 * Out of the page's script, and the two together on purpose — they are the
 * two answers to gotcha 10 (the window is transparent, so nothing painted is
 * the desktop), and they differ in exactly one case: launching with a file
 * hides the start screen but must keep the fill. Going back to the start
 * screen lives here too, since it is the one way in that knows where it is
 * going (see `backToStart`).
 */

import { command } from 'tauri-plugin-libmpv-api';
import { liftCurtain } from './curtain.svelte';
import { flushPosition, loadRecent } from './history.svelte';
import { abandonOpening, refreshTorrents } from './open.svelte';
import { player } from './player.svelte';
import { title } from './title.svelte';
import { releaseTorrent, torrent } from './torrent.svelte';

class StartScreen {
  // Debounced "empty" state: switching files in the playlist blanks filename
  // for a moment, and the file-picker screen must not flash. The debounce is
  // for that involuntary blank only — a deliberate close goes through
  // `enterStartScreen` at once, see `backToStart`.
  showEmpty = $state(true);
  emptyTimer: ReturnType<typeof setTimeout> | undefined;

  /// mpv is up and paints its own area — until then a dark fill sits under the
  /// UI. Separate from showEmpty: launching with a file has to hide the start
  /// screen but not the background, or the transparent window shows the desktop.
  videoReady = $state(false);
}

export const startScreen = new StartScreen();

/// Drops the dark fill. Driven by vo-configured (the mirror is fed by the
/// property event and by resyncState — the initial event races the listener
/// registration and can be lost), with playback-restart as the backstop: once
/// playback started, mpv is definitely painting. This used to be a fixed
/// delay after init(), which is a guess — init() resolves when libmpv is
/// initialized, not when its VO has presented anything, and on a heavy 4K
/// open the first present came later than the slack: the fill dropped while
/// nobody painted and the desktop flashed through the transparent window.
/// The double-rAF + delay waits from a correct anchor instead: a couple of
/// frames for the VO to present the field it just configured. Erring long is
/// invisible (our black over mpv's black field), erring short is the desktop.
export function armVideoReady() {
  if (startScreen.videoReady) return;
  requestAnimationFrame(() =>
    requestAnimationFrame(() => setTimeout(() => (startScreen.videoReady = true), 50)),
  );
}

/// The fill's standing effect. Call during a component's initialization — see
/// the `$effect` rule in docs/rules/frontend.md.
export function initVideoReady() {
  $effect(() => {
    if (player.voConfigured) armVideoReady();
  });
}

/// Put the picker on screen and give it something current to show. Reached
/// two ways, and the difference is only *when*: on a debounce from the
/// `filename` handler, because a playlist transition blanks the name for a
/// moment and the picker must not flash through it; and immediately from
/// `backToStart`, where there is nothing to wait and see about.
export function enterStartScreen() {
  clearTimeout(startScreen.emptyTimer);
  startScreen.showEmpty = true;
  // Nothing is coming that it could be waiting for.
  liftCurtain(0);
  void loadRecent();
  void refreshTorrents();
  title.barTitleText = '';
  title.titleSlide = '';
}

/// Close what is playing and return to the start screen.
///
/// `stop` rather than `playlist-remove` or a flag of our own: it unloads the
/// file *and* empties the playlist, which is what "back to the start screen"
/// has to mean — leaving a queue behind would have the next episode start
/// itself the moment anything nudged playback.
export async function backToStart() {
  // The picker goes up BEFORE mpv is told to stop, rather than 300 ms after
  // it already has. That debounce exists for a playlist transition, where the
  // blank is involuntary and momentary; here the destination is known, and
  // waiting it out left the player's own chrome standing over a stopped VO —
  // a black frame under a seekbar reading 00:00, which is what going back
  // used to look like. `.player.no-video` is opaque, so raising it first
  // covers the video and nothing in between is ever seen.
  //
  // The position is flushed first so the card this file is about to appear on
  // carries the moment it was actually left at, not the previous write.
  flushPosition();
  // Nothing is being opened any more, and saying so is this path's job: the
  // indicator goes up in `beforeLoad` and comes down on `file-loaded`, so a
  // load abandoned in between — a torrent still looking for peers, which is
  // exactly when somebody gives up on it — has nothing else to take it down.
  // The `stop` below does not: its `end-file` carries the reason `stop`, and
  // only `error` reaches the failure path.
  abandonOpening();
  // And the torrent is let go here rather than left to the `filename === null`
  // handler, for the same reason the picker is raised here: this path *knows*
  // it is leaving, while that one is inferring it from an event that may be
  // dropped (a queue overflow costs property changes — see the mirrors' 1 s
  // resync). Getting it wrong leaves the swarm running and the download
  // readout standing over the start screen, which is exactly what it looked
  // like. Idempotent: the handler's own call finds nothing left to release.
  if (torrent.info) void releaseTorrent();
  enterStartScreen();
  await command('stop').catch((e) => console.warn('stop failed:', e));
}
