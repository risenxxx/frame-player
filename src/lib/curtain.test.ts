/**
 * The dark between two pictures, and above all its ways back.
 *
 * What it covers is a film that is playing. A curtain that misses its moment
 * is a transition that looks the way it used to; one that stays down is a
 * player showing black over sound, which nobody can tell from a broken one.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('tauri-plugin-libmpv-api', () => ({
  command: vi.fn(async () => undefined),
  getProperty: vi.fn(async () => null),
  setProperty: vi.fn(async () => undefined),
}));
// The Mac, where the dark is the video view's to make: the platform under test
// is otherwise whatever the user agent of the test runner happens to say.
vi.mock('./platform', () => ({ IS_MAC: true }));

/// Whether there is a video view to fade, and what was asked of it.
let viewThere = true;
const fades: { to: number; ms: number }[] = [];

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (name: string, args: { to: number; ms: number }) => {
    if (name !== 'window_video_fade') return undefined;
    if (!viewThere) throw new Error('no view');
    fades.push(args);
    return true;
  }),
}));

import {
  CAP_MS,
  LIFT_MS,
  NO_PICTURE_MS,
  RAISE_MS,
  curtain,
  curtainCovered,
  curtainFileLoaded,
  curtainFileStarted,
  curtainSettled,
  initCurtain,
  liftCurtain,
  raiseCurtain,
} from './curtain.svelte';
import { player } from './player.svelte';

let loading = false;

function film() {
  player.filename = 'film.mkv';
  player.filePath = '/film.mkv';
  player.videoW = 1920;
  player.videoH = 1080;
  player.sourceTransfer = 'bt.1886';
}

function nothing() {
  player.filename = null;
  player.filePath = null;
  player.videoW = 0;
  player.videoH = 0;
  player.sourceTransfer = null;
}

/// Down, with the clock the fade waits on moved past it.
async function down() {
  const dark = raiseCurtain();
  await vi.advanceTimersByTimeAsync(RAISE_MS + 40);
  await dark;
}

beforeEach(async () => {
  vi.useFakeTimers();
  loading = false;
  viewThere = true;
  initCurtain({ loading: () => loading });
  liftCurtain(0);
  // Past anything the last test left waiting, the web view's turn included.
  await vi.advanceTimersByTimeAsync(CAP_MS + 100);
  film();
  fades.length = 0;
});

afterEach(() => {
  vi.useRealTimers();
});

describe('coming down', () => {
  it('answers only once the picture is dark', async () => {
    let landed = false;
    void raiseCurtain().then(() => (landed = true));
    expect(curtain.on).toBe(true);
    expect(curtain.ms).toBe(RAISE_MS);
    await vi.advanceTimersByTimeAsync(RAISE_MS - 1);
    expect(landed).toBe(false);
    await vi.advanceTimersByTimeAsync(40);
    expect(landed).toBe(true);
  });

  it('is down at once where there is no picture to fade from', async () => {
    nothing();
    let landed = false;
    void raiseCurtain().then(() => (landed = true));
    await vi.advanceTimersByTimeAsync(0);
    expect(landed).toBe(true);
    expect(curtain.on).toBe(true);
    expect(curtain.ms).toBe(0);
  });

  it('does not take a size for a picture', async () => {
    // An audio file: the VO reports its own 960x540 field, the decoder nothing.
    player.videoW = 960;
    player.videoH = 540;
    player.sourceTransfer = null;
    void raiseCurtain();
    expect(curtain.ms).toBe(0);
  });

  it('does not hold the second of two changes back', async () => {
    await down();
    let landed = false;
    void raiseCurtain().then(() => (landed = true));
    await vi.advanceTimersByTimeAsync(0);
    expect(landed).toBe(true);
  });

  it('comes down for a file nobody announced', () => {
    curtainFileStarted();
    expect(curtain.on).toBe(true);
  });
});

describe('coming off', () => {
  it('takes its time from the window', async () => {
    await down();
    curtainFileLoaded();
    curtainSettled(240);
    expect(curtain.on).toBe(false);
    expect(curtain.ms).toBe(240);
  });

  it('has a time of its own where the window does not move', async () => {
    await down();
    curtainFileLoaded();
    curtainSettled(0);
    expect(curtain.on).toBe(false);
    expect(curtain.ms).toBe(LIFT_MS);
  });

  it('does not lift for the picture it came down over', async () => {
    await down();
    // The previous film, reporting itself in its window: the new one is not open.
    curtainSettled(0);
    expect(curtain.on).toBe(true);
    curtainFileLoaded();
    curtainSettled(0);
    expect(curtain.on).toBe(false);
  });

  it('waits for the newest of two files', async () => {
    await down();
    curtainFileLoaded();
    await down();
    // The first file's picture, arriving after the second was asked for.
    curtainSettled(240);
    expect(curtain.on).toBe(true);
  });

  it('is not re-armed by the event that follows a raise', async () => {
    await down();
    curtainFileLoaded();
    // `path` reported after `file-loaded`: the order is mpv's.
    curtainFileStarted();
    curtainSettled(0);
    expect(curtain.on).toBe(false);
  });
});

describe('who draws it', () => {
  it('is the video view, on macOS', async () => {
    await down();
    expect(fades).toEqual([{ to: 0, ms: RAISE_MS }]);
    expect(curtain.drawn).toBe(false);
    curtainFileLoaded();
    curtainSettled(240);
    expect(fades.at(-1)).toEqual({ to: 1, ms: 240 });
  });

  it('is the web view for a dark the video view could not make, and for that one only', async () => {
    viewThere = false;
    await down();
    expect(curtain.drawn).toBe(true);

    // Lifted by whoever made it, and still there to be lifted while it fades.
    viewThere = true;
    curtainFileLoaded();
    curtainSettled(240);
    expect(curtain.drawn).toBe(true);
    expect(fades).toEqual([]);
    await vi.advanceTimersByTimeAsync(300);
    expect(curtain.drawn).toBe(false);

    await down();
    expect(curtain.drawn).toBe(false);
    expect(fades).toEqual([{ to: 0, ms: RAISE_MS }]);
  });

  it('is not decided by the call made before there is a view at all', async () => {
    // Launch: mpv has not made its view, and the page puts the picture back
    // in case it was reloaded with the dark down.
    viewThere = false;
    initCurtain({ loading: () => loading });
    await vi.advanceTimersByTimeAsync(50);
    expect(curtain.drawn).toBe(false);
    viewThere = true;
    await down();
    expect(curtain.drawn).toBe(false);
    expect(fades).toEqual([{ to: 0, ms: RAISE_MS }]);
  });
});

describe('ways back', () => {
  it('lifts for a file that shows no picture', async () => {
    await down();
    curtainFileLoaded();
    await vi.advanceTimersByTimeAsync(NO_PICTURE_MS - 1);
    expect(curtain.on).toBe(true);
    await vi.advanceTimersByTimeAsync(2);
    expect(curtain.on).toBe(false);
  });

  it('lifts when nothing is heard at all', async () => {
    await down();
    await vi.advanceTimersByTimeAsync(CAP_MS + 1);
    expect(curtain.on).toBe(false);
  });

  it('stays down over a load that is still under way, and no longer', async () => {
    loading = true;
    await down();
    await vi.advanceTimersByTimeAsync(CAP_MS * 3);
    expect(curtain.on).toBe(true);
    loading = false;
    await vi.advanceTimersByTimeAsync(CAP_MS + 1);
    expect(curtain.on).toBe(false);
  });

  it('counts from the last change, not the first', async () => {
    await down();
    await vi.advanceTimersByTimeAsync(CAP_MS - 100);
    await down();
    await vi.advanceTimersByTimeAsync(200);
    expect(curtain.on).toBe(true);
  });

  it('leaves nothing armed behind it', async () => {
    await down();
    curtainFileLoaded();
    liftCurtain(0);
    // Timers of the change that was called off must not act on the next one —
    // here one that nobody announced, which resets nothing on its way in.
    await vi.advanceTimersByTimeAsync(NO_PICTURE_MS - 100);
    curtainFileStarted();
    await vi.advanceTimersByTimeAsync(200);
    expect(curtain.on).toBe(true);
  });

  it('is lifted by the start screen at once', async () => {
    await down();
    liftCurtain(0);
    expect(curtain.on).toBe(false);
    expect(curtain.ms).toBe(0);
  });
});

// Opening from the start screen: the window is about to move to a shape nobody
// knows yet, and what floats over the picture waits for it. A `bare` that
// outlives its curtain hides the control bar for the rest of the session, so
// every way back has to end it.
describe('over no picture', () => {
  it('is bare only when there was nothing to cover', async () => {
    await down();
    expect(curtain.bare).toBe(false);
    liftCurtain(0);
    nothing();
    void raiseCurtain();
    expect(curtain.bare).toBe(true);
  });

  it('stops being bare when the curtain lifts, by any way back', async () => {
    nothing();
    void raiseCurtain();
    curtainFileLoaded();
    curtainSettled(240);
    expect(curtain.bare).toBe(false);

    void raiseCurtain();
    curtainFileLoaded();
    await vi.advanceTimersByTimeAsync(NO_PICTURE_MS + 10);
    expect(curtain.bare).toBe(false);

    void raiseCurtain();
    await vi.advanceTimersByTimeAsync(CAP_MS + 10);
    expect(curtain.bare).toBe(false);
  });

  it('is not bare when the loading plate was up before it came down', () => {
    nothing();
    loading = true;
    void raiseCurtain();
    expect(curtain.bare).toBe(false);
    expect(curtain.on).toBe(true);
  });

  it('stops being bare once the loading plate stands over it, curtain or not', () => {
    nothing();
    void raiseCurtain();
    curtainCovered();
    expect(curtain.bare).toBe(false);
    expect(curtain.on).toBe(true);
  });
});
