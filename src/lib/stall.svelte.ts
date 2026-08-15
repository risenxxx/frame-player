/**
 * Whether this process is being stopped, and for how long.
 *
 * The one measurement mpv structurally cannot make about itself. A freeze of a
 * fraction of a second shows up in its own log as a consequence — an audio
 * device underrun, frames arriving too late to present — and never as a cause,
 * because a thread that is not running cannot record that it is not running.
 * The counters in `player.svelte.ts` say *that* frames were lost; this says
 * whether the whole process was stopped when they were.
 *
 * Which is the question that decides where to look next, and it has exactly two
 * answers. Frames lost while this stays quiet means the block was inside mpv's
 * own threads or the GPU queue — the webview kept running through it. Frames
 * lost with a stall of the same size at the same moment means nothing in this
 * process was running at all, which is memory or the scheduler and not anything
 * about video.
 *
 * The mechanism is a timer that measures its own lateness. A `setInterval` is
 * allowed to fire late and routinely does by a few milliseconds; what it cannot
 * do is fire late by a fifth of a second while the machine is idle. So the
 * baseline is re-taken on every tick rather than accumulated — otherwise one
 * late tick makes every tick after it look late — and only the excess past
 * `STALL_MS` is recorded.
 */

import { player } from './player.svelte';

const TICK_MS = 100;

/// How late a tick has to be before it is worth writing down.
///
/// Ordinary scheduling noise is single-digit milliseconds. This is set from the
/// other end instead: mpv's CoreAudio output here runs on a 200 ms soft buffer,
/// so a block long enough to be *heard* is around that, and one long enough to
/// be *seen* is a handful of 24 fps frames. 120 ms is below both and well above
/// the noise, which is what makes a recorded stall mean something.
const STALL_MS = 120;

class Stalls {
  /// The most recent stall and when it landed, or null if nothing has stopped
  /// this process since the file opened.
  last = $state<{ ms: number; at: number } | null>(null);
  /// The longest one, which is the figure worth comparing against the audio
  /// buffer: anything at or past 200 ms is long enough to have cost the
  /// underrun mpv reports.
  worst = $state(0);
}

export const stalls = new Stalls();

/**
 * Start watching. Called from the page for the reason every `initX` is: a
 * `$effect` at the top level of a module throws the moment it is imported.
 */
export function initStallWatch() {
  $effect(() => {
    let due = performance.now() + TICK_MS;
    const timer = setInterval(() => {
      const now = performance.now();
      const late = now - due;
      due = now + TICK_MS;
      // A hidden window is throttled on purpose by the webview, so its lateness
      // says nothing about the machine — and it would otherwise record a stall
      // of several seconds every time the player is minimized, which is the one
      // reading guaranteed to be meaningless.
      if (document.hidden || late < STALL_MS) return;
      const ms = Math.round(late);
      stalls.last = { ms, at: Date.now() };
      stalls.worst = Math.max(stalls.worst, ms);
    }, TICK_MS);
    return () => clearInterval(timer);
  });

  // Cleared with the file, like the drop counters beside it — opening one costs
  // the main thread a visible pause of its own, and carrying that forward would
  // leave every episode reporting the *previous* one's worst moment.
  $effect(() => {
    void player.filePath;
    stalls.last = null;
    stalls.worst = 0;
  });
}
