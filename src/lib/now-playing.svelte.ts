/**
 * Now Playing on macOS: the player as the system's current media application.
 *
 * What it buys is everything the system does for such an application: the
 * entry in Control Center with the title and the progress, the keyboard's
 * play/pause key, the Now Playing buttons — and the pause that AirPods send
 * when they leave the ears, which is what this was written for.
 *
 * The facts travel down, the commands travel up. This side sends the title,
 * the length, the position and the rate to `now_playing_set` whenever one of
 * them changes; the system extrapolates the position from the rate on its own,
 * so a playing file sends nothing most of the time. What does send is a seek:
 * a position more than `DRIFT_S` away from where the last report said it would
 * be by now. Commands arrive as `frameplayer://remote` and are answered by the
 * verbs in `playback`, the same ones the keys use, so a pause from the AirPods
 * pauses a television the player is casting to.
 *
 * mpv has an integration of its own and it is off (`input-media-keys=no` in
 * `initialOptions`): under libmpv it never learns what is playing, since its
 * event helper refuses to start unless the application is mpv.app's — Control
 * Center showed "mpv" with mpv's icon, and its buttons did nothing.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import { advance, playback, seekFraction, togglePause } from './playback.svelte';
import { player } from './player.svelte';
import { IS_MAC } from './platform';

/// How far the position may stray from the system's extrapolation of it before
/// it is reported again. Wide enough that the clock drift of a long film never
/// trips it; a seek of any size does.
const DRIFT_S = 1.5;

interface Report {
  playing: boolean;
  title: string;
  duration: number;
  position: number;
  rate: number;
  paused: boolean;
  next: boolean;
  previous: boolean;
}

interface RemoteCommand {
  command: 'pause' | 'play' | 'toggle' | 'next' | 'previous' | 'seek';
  position?: number;
}

/// The last report, with the clock it was made on, for the extrapolation.
let sent: (Report & { at: number }) | null = null;

function report(info: Report) {
  sent = { ...info, at: performance.now() };
  void invoke('now_playing_set', { info }).catch((e) => console.warn('now_playing_set failed:', e));
}

function onRemote({ command, position }: RemoteCommand) {
  switch (command) {
    case 'pause':
      if (!playback.paused) togglePause();
      break;
    case 'play':
      if (playback.paused) togglePause();
      break;
    case 'toggle':
      togglePause();
      break;
    case 'next':
      advance(1);
      break;
    case 'previous':
      advance(-1);
      break;
    case 'seek':
      if (position !== undefined && playback.duration > 0) {
        seekFraction((position / playback.duration) * 100);
      }
      break;
  }
}

/// The standing effects: the listener for the system's commands, installed
/// before the native side is asked to send any, and the report of what is
/// playing. macOS only — elsewhere this is a no-op, and the native commands it
/// would call are too.
export function initNowPlaying() {
  if (!IS_MAC) return;

  $effect(() => {
    let gone = false;
    let unlisten: (() => void) | null = null;
    void listen<RemoteCommand>('frameplayer://remote', (e) => onRemote(e.payload)).then((un) => {
      if (gone) un();
      else unlisten = un;
      void invoke('now_playing_start').catch((e) => console.warn('now_playing_start failed:', e));
    });
    return () => {
      gone = true;
      unlisten?.();
    };
  });

  $effect(() => {
    if (!player.hasFile) {
      if (sent?.playing !== false) {
        report({ playing: false, title: '', duration: 0, position: 0, rate: 0, paused: true, next: false, previous: false });
      }
      return;
    }
    const paused = playback.paused;
    const info: Report = {
      playing: true,
      title: player.displayTitle,
      duration: playback.duration,
      position: playback.position,
      rate: paused ? 0 : player.speed,
      paused,
      next: player.playlistPos < player.playlistCount - 1,
      previous: player.playlistPos > 0,
    };
    const last = sent;
    if (last?.playing) {
      const expected = last.position + ((performance.now() - last.at) / 1000) * last.rate;
      const same =
        last.title === info.title &&
        last.duration === info.duration &&
        last.paused === info.paused &&
        last.rate === info.rate &&
        last.next === info.next &&
        last.previous === info.previous;
      if (same && Math.abs(info.position - expected) < DRIFT_S) return;
    }
    report(info);
  });
}
