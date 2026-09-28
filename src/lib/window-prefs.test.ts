/**
 * The window keeping the picture's shape, against a window with the stricter
 * of the two platforms' manners: it clamps a size under its minimum and says
 * nothing (Win32; macOS takes the size as given), and grows to meet a minimum
 * it is under, as tao makes it on both.
 *
 * Everything here is a way for "match video aspect ratio" to be on over a
 * window that does not match — which is what the setting was turned off for,
 * and none of it shows with one ordinary film on a large screen.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

interface FakeWindow {
  w: number;
  h: number;
  x: number;
  y: number;
  min: { w: number; h: number };
  dpr: number;
  fullscreen: boolean;
  maximized: boolean;
  work: { w: number; h: number };
}

const win: FakeWindow = {
  w: 1100,
  h: 660,
  x: 100,
  y: 100,
  min: { w: 480, h: 320 },
  dpr: 1,
  fullscreen: false,
  maximized: false,
  work: { w: 1728, h: 1000 },
};

/// What the platform was told, in the order it was told.
const calls: string[] = [];
let locked: { width: number; height: number } | null = null;

vi.mock('tauri-plugin-libmpv-api', () => ({
  command: vi.fn(async () => undefined),
  getProperty: vi.fn(async () => null),
  setProperty: vi.fn(async () => undefined),
}));

/// Whether the platform can move a frame as one thing. Off, the two calls of
/// the window API are what is left.
let canGlide = true;
/// How long the last glide was asked to take.
let glided: number | null = null;
/// What the shell was being told while the window was in the air.
let saidWhileMoving: boolean | null = null;

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(
    async (name: string, args: { x: number; y: number; width: number; height: number; ms: number }) => {
      if (name === 'window_frame_glide') {
        if (!canGlide) return false;
        // The refusal is silent: the size is clamped and the call succeeds.
        win.w = Math.max(args.width, win.min.w);
        win.h = Math.max(args.height, win.min.h);
        win.x = args.x;
        win.y = args.y;
        glided = args.ms;
        saidWhileMoving = shapeState.gliding;
        calls.push(`size ${win.w}x${win.h}`);
        return true;
      }
      if (name !== 'window_shape_lock') return undefined;
      locked = args.width > 0 && args.height > 0 ? { width: args.width, height: args.height } : null;
      calls.push(locked ? `lock ${args.width}x${args.height}` : 'unlock');
      return undefined;
    },
  ),
}));

vi.mock('@tauri-apps/api/dpi', () => {
  class Sized {
    constructor(
      public width: number,
      public height: number,
    ) {}
  }
  class LogicalSize extends Sized {
    type = 'Logical';
  }
  class PhysicalSize extends Sized {
    type = 'Physical';
  }
  class PhysicalPosition {
    type = 'Physical';
    constructor(
      public x: number,
      public y: number,
    ) {}
  }
  return { LogicalSize, PhysicalSize, PhysicalPosition };
});

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    isFullscreen: async () => win.fullscreen,
    isMaximized: async () => win.maximized,
    scaleFactor: async () => win.dpr,
    outerSize: async () => ({ width: win.w, height: win.h }),
    innerSize: async () => ({ width: win.w, height: win.h }),
    outerPosition: async () => ({ x: win.x, y: win.y }),
    setMinSize: async (s: { type: string; width: number; height: number }) => {
      const k = s.type === 'Logical' ? win.dpr : 1;
      win.min = { w: s.width * k, h: s.height * k };
      // As tao does: a window under its new minimum is grown to it.
      win.w = Math.max(win.w, win.min.w);
      win.h = Math.max(win.h, win.min.h);
      calls.push(`min ${s.width}x${s.height}`);
    },
    setSize: async (s: { width: number; height: number }) => {
      win.w = Math.max(s.width, win.min.w);
      win.h = Math.max(s.height, win.min.h);
      calls.push(`size ${win.w}x${win.h}`);
    },
    setPosition: async (p: { x: number; y: number }) => {
      win.x = p.x;
      win.y = p.y;
    },
    setAlwaysOnTop: async () => undefined,
    setFullscreen: async (on: boolean) => {
      win.fullscreen = on;
    },
  }),
  currentMonitor: async () => ({
    workArea: { position: { x: 0, y: 0 }, size: { width: win.work.w, height: win.work.h } },
  }),
}));

import { player } from './player.svelte';
import {
  fitWindowToVideo,
  initWindowShape,
  loadWindowPrefs,
  scheduleShape,
  shapeState,
  toggleWindowPref,
  windowPrefs,
} from './window-prefs.svelte';

const page = { owned: false, resting: false };
/// What the window said about the picture having its window, in order.
const settled: number[] = [];

/// What the standing effect does on any change: the tests are compiled the way
/// a server build is, where an effect is not code at all.
function changed() {
  scheduleShape();
}

/// A change, its debounce and the round trips behind it.
async function settle(ms = 200) {
  changed();
  await vi.advanceTimersByTimeAsync(ms);
}

function show(path: string, w: number, h: number, rotate = 0) {
  player.filename = path.split('/').pop() ?? null;
  player.filePath = path;
  player.videoW = w;
  player.videoH = h;
  player.voRotate = rotate;
  // The decoder's word that there is a picture at all.
  player.sourceTransfer = w > 0 ? 'bt.1886' : null;
}

const ratio = () => win.w / win.h;
const center = () => ({ x: win.x + win.w / 2, y: win.y + win.h / 2 });

beforeEach(async () => {
  vi.useFakeTimers();
  Object.assign(win, {
    w: 1100,
    h: 660,
    x: 100,
    y: 100,
    min: { w: 480, h: 320 },
    dpr: 1,
    fullscreen: false,
    maximized: false,
    work: { w: 1728, h: 1000 },
  });
  page.owned = false;
  page.resting = false;
  canGlide = true;
  glided = null;
  windowPrefs.fitToVideo = true;
  show('/nothing', 0, 0);
  player.filename = null;
  player.filePath = null;
  initWindowShape({
    sizeOwned: () => page.owned,
    resting: () => page.resting,
    settled: (ms) => settled.push(ms),
  });
  // The start screen for a moment, which is what empties the record of the
  // last fit — module state that would otherwise run on into the next test.
  page.resting = true;
  await settle();
  page.resting = false;
  await settle();
  calls.length = 0;
  settled.length = 0;
});

afterEach(() => {
  vi.useRealTimers();
  localStorage.clear();
});

describe('the window follows the picture', () => {
  it('takes its shape when a file opens and is held to it', async () => {
    show('/a.mkv', 1920, 1080);
    await settle();
    expect(ratio()).toBeCloseTo(16 / 9, 2);
    expect(locked).toEqual({ width: 1920, height: 1080 });
    // The minimum is a size of the shape, or the ratio takes the window under it.
    expect(win.min).toEqual({ w: 569, h: 320 });
  });

  it('keeps the area it had', async () => {
    show('/a.mkv', 1440, 1080);
    await settle();
    expect(Math.abs(win.w * win.h - 1100 * 660)).toBeLessThan(2000);
  });

  it('waits for the whole shape rather than fitting half of one', async () => {
    show('/a.mkv', 1920, 1080);
    await settle();
    calls.length = 0;
    // The three properties of the next file, one event each.
    player.videoW = 1080;
    changed();
    await vi.advanceTimersByTimeAsync(5);
    player.videoH = 1920;
    await settle();
    expect(calls.filter((c) => c.startsWith('size'))).toHaveLength(1);
    expect(ratio()).toBeCloseTo(9 / 16, 2);
  });

  it('stands an upright clip up', async () => {
    // What mpv reports for a phone clip: the size before the turn.
    show('/phone.mov', 1920, 1080, 270);
    await settle();
    expect(ratio()).toBeCloseTo(9 / 16, 2);
    expect(locked).toEqual({ width: 1080, height: 1920 });
  });

  it('gives a film its shape back after an upright clip', async () => {
    show('/phone.mov', 1080, 1920);
    await settle();
    expect(win.min).toEqual({ w: 480, h: 853 });
    show('/film.mkv', 1920, 1080);
    await settle();
    // Under the clip's minimum this size is clamped, without a word.
    expect(ratio()).toBeCloseTo(16 / 9, 2);
  });

  it('does not lose area to a clip the screen had to shrink', async () => {
    win.work = { w: 1440, h: 900 };
    show('/film.mkv', 1920, 1080);
    await settle();
    const before = win.w * win.h;
    show('/phone.mov', 1080, 1920);
    await settle();
    expect(win.w * win.h).toBeLessThan(before * 0.75);
    show('/film2.mkv', 1920, 1080);
    await settle();
    expect(Math.abs(win.w * win.h - before)).toBeLessThan(2000);
  });

  it('takes a size chosen by hand as the new one', async () => {
    show('/film.mkv', 1920, 1080);
    await settle();
    // Dragged, under the constraint.
    win.w = 800;
    win.h = 450;
    show('/old.avi', 1440, 1080);
    await settle();
    expect(Math.abs(win.w * win.h - 800 * 450)).toBeLessThan(2000);
  });

  it('leaves the next episode at the size the last one was given', async () => {
    show('/e01.mkv', 1920, 1080);
    await settle();
    win.w = 800;
    win.h = 450;
    calls.length = 0;
    show('/e02.mkv', 1920, 1080);
    await settle();
    expect(calls.filter((c) => c.startsWith('size'))).toEqual([]);
  });

  it('keeps hold while the picture is momentarily unavailable', async () => {
    show('/torrent.mkv', 1920, 1080);
    await settle();
    calls.length = 0;
    // The VO reconfiguring: mpv reports both as unavailable.
    player.videoW = 0;
    player.videoH = 0;
    player.sourceTransfer = null;
    await settle();
    expect(calls).toEqual([]);
    expect(locked).toEqual({ width: 1920, height: 1080 });
  });

  it('does not hand a film`s window to the audio file after it', async () => {
    show('/film.mkv', 1920, 1080);
    await settle();
    show('/album.flac', 0, 0);
    await settle();
    expect(locked).toBeNull();
    expect(win.min).toEqual({ w: 480, h: 320 });
  });

  it('does not take the field the VO paints black for a picture', async () => {
    // What mpv reports for a file with no picture, and for every file until
    // its first frame: the size of `force-window`'s own field.
    show('/album.flac', 960, 540);
    player.sourceTransfer = null;
    await settle();
    expect(calls).toEqual([]);
    expect(locked).toBeNull();
  });
});

describe('how the window gets there', () => {
  it('changes shape around its own center', async () => {
    Object.assign(win, { x: 300, y: 150, w: 1200, h: 600 });
    const before = center();
    show('/clip.mp4', 1080, 1080);
    await settle();
    expect(ratio()).toBeCloseTo(1, 2);
    expect(Math.abs(center().x - before.x)).toBeLessThanOrEqual(1);
    expect(Math.abs(center().y - before.y)).toBeLessThanOrEqual(1);
  });

  it('gives way to the screen`s edge', async () => {
    // Close to the top: the square would reach above the work area.
    Object.assign(win, { x: 300, y: 30, w: 1200, h: 600 });
    show('/clip.mp4', 1080, 1080);
    await settle();
    expect(win.y).toBe(24);
    expect(Math.abs(center().x - 900)).toBeLessThanOrEqual(1);
  });

  it('takes its time, as one frame', async () => {
    show('/clip.mp4', 1080, 1080);
    await settle();
    expect(glided).toBeGreaterThan(0);
    // One change, not a size and then a position.
    expect(calls.filter((c) => c.startsWith('size'))).toHaveLength(1);
  });

  it('says so for as long as the window is in the air, and no longer', async () => {
    expect(shapeState.gliding).toBe(false);
    show('/clip.mp4', 1080, 1080);
    await settle();
    expect(saidWhileMoving).toBe(true);
    expect(shapeState.gliding).toBe(false);
  });

  it('still arrives where the frame cannot be moved as one', async () => {
    canGlide = false;
    Object.assign(win, { x: 300, y: 150, w: 1200, h: 600 });
    const before = center();
    show('/clip.mp4', 1080, 1080);
    await settle();
    expect(ratio()).toBeCloseTo(1, 2);
    expect(Math.abs(center().x - before.x)).toBeLessThanOrEqual(1);
    expect(locked).toEqual({ width: 1080, height: 1080 });
  });
});

describe('saying when the picture has its window', () => {
  it('says how long the way there takes, when it starts', async () => {
    show('/clip.mp4', 1080, 1080);
    await settle();
    expect(settled).toHaveLength(1);
    expect(settled[0]).toBeGreaterThan(0);
  });

  it('says so at once where the window does not move', async () => {
    show('/e01.mkv', 1920, 1080);
    await settle();
    settled.length = 0;
    show('/e02.mkv', 1920, 1080);
    await settle();
    expect(settled).toEqual([0]);
  });

  it('says so with the setting off, where a picture is in its window by existing', async () => {
    toggleWindowPref('fitToVideo');
    await settle();
    settled.length = 0;
    show('/a.mkv', 1920, 1080);
    await settle();
    expect(settled).toEqual([0]);
  });

  it('says so in fullscreen, where no fit is coming', async () => {
    page.owned = true;
    win.fullscreen = true;
    show('/a.mkv', 1920, 1080);
    await settle();
    expect(settled).toEqual([0]);
  });

  it('says nothing for a file with no picture', async () => {
    show('/album.flac', 960, 540);
    player.sourceTransfer = null;
    await settle();
    expect(settled).toEqual([]);
  });

  it('says nothing on the start screen', async () => {
    show('/a.mkv', 1920, 1080);
    page.resting = true;
    await settle();
    expect(settled).toEqual([]);
  });

  it('keeps the sizes in the menu to themselves', async () => {
    show('/a.mkv', 1280, 720);
    await settle();
    settled.length = 0;
    await fitWindowToVideo(0.5);
    expect(settled).toEqual([]);
  });
});

describe('modes that own the size', () => {
  it('fits on the way out of fullscreen what changed inside it', async () => {
    show('/e01.mkv', 1920, 1080);
    await settle();
    const windowed = { w: win.w, h: win.h };
    page.owned = true;
    win.fullscreen = true;
    await settle();
    show('/e02.mkv', 1440, 1080);
    await settle();
    expect({ w: win.w, h: win.h }).toEqual(windowed);

    // The mirror flips first and the system's answer follows the animation.
    page.owned = false;
    await settle(300);
    expect({ w: win.w, h: win.h }).toEqual(windowed);
    win.fullscreen = false;
    await settle(600);
    expect(ratio()).toBeCloseTo(4 / 3, 2);
  });

  it('stands down while maximized', async () => {
    page.owned = true;
    win.maximized = true;
    show('/a.mkv', 1920, 1080);
    await settle();
    expect(calls.filter((c) => c.startsWith('size'))).toEqual([]);
  });
});

describe('where the shape cannot be had', () => {
  it('lets the window go rather than hold it to a size off the screen', async () => {
    // A 13-inch laptop: 9:16 needs 853 and 757 is what there is.
    win.work = { w: 1440, h: 805 };
    show('/phone.mov', 1080, 1920);
    await settle();
    expect(locked).toBeNull();
    expect(win.min).toEqual({ w: 480, h: 320 });
    expect(win.h).toBeLessThanOrEqual(757);
  });

  it('counts the minimum in the screen`s own pixels', async () => {
    win.dpr = 2;
    Object.assign(win, { w: 2200, h: 1320, min: { w: 960, h: 640 }, work: { w: 3456, h: 2000 } });
    show('/a.mkv', 640, 360);
    await settle();
    await fitWindowToVideo(0.5);
    // 320x180 asked for; the floor of 16:9 is 569x320 logical.
    expect({ w: win.w, h: win.h }).toEqual({ w: 1138, h: 640 });
  });
});

describe('the setting and the start screen', () => {
  it('lets the window go when turned off, and fits it again when turned on', async () => {
    show('/a.mkv', 1920, 1080);
    await settle();
    toggleWindowPref('fitToVideo');
    await settle();
    expect(locked).toBeNull();
    expect(win.min).toEqual({ w: 480, h: 320 });

    // Free now, and dragged into a shape the picture does not have.
    win.w = 700;
    win.h = 700;
    toggleWindowPref('fitToVideo');
    await settle();
    expect(ratio()).toBeCloseTo(16 / 9, 2);
    expect(locked).toEqual({ width: 1920, height: 1080 });
  });

  it('does nothing at all while off', async () => {
    toggleWindowPref('fitToVideo');
    await settle();
    calls.length = 0;
    show('/a.mkv', 1920, 1080);
    await settle();
    expect(calls).toEqual([]);
  });

  it('still offers the sizes in the menu while off, in the picture`s shape', async () => {
    toggleWindowPref('fitToVideo');
    await settle();
    show('/a.mkv', 1280, 720);
    await settle();
    await fitWindowToVideo(1);
    expect({ w: win.w, h: win.h }).toEqual({ w: 1280, h: 720 });
    expect(locked).toBeNull();
  });

  it('fits the same shape again after the start screen', async () => {
    show('/a.mkv', 1920, 1080);
    await settle();
    page.resting = true;
    player.filename = null;
    player.filePath = null;
    player.videoW = 0;
    player.videoH = 0;
    await settle();
    expect(locked).toBeNull();

    // Nobody's window there, and it was made tall for the list.
    win.w = 600;
    win.h = 900;
    page.resting = false;
    show('/a.mkv', 1920, 1080);
    await settle();
    expect(ratio()).toBeCloseTo(16 / 9, 2);
  });
});

describe('what is read from a saved payload', () => {
  const KEY = 'frameplayer.window';

  it('is on where nothing was saved', () => {
    windowPrefs.fitToVideo = true;
    loadWindowPrefs();
    expect(windowPrefs.fitToVideo).toBe(true);
  });

  it('does not take an "off" that was only the default of its day', () => {
    localStorage.setItem(KEY, JSON.stringify({ v: 2, remember: true, fitToVideo: false, autoHide: 'never' }));
    windowPrefs.fitToVideo = true;
    windowPrefs.remember = false;
    loadWindowPrefs();
    expect(windowPrefs.fitToVideo).toBe(true);
    // The rest of that payload is still somebody's choice.
    expect(windowPrefs.remember).toBe(true);
    expect(windowPrefs.autoHide).toBe('never');
  });

  it('takes one that was chosen', () => {
    localStorage.setItem(KEY, JSON.stringify({ v: 3, fitToVideo: false }));
    windowPrefs.fitToVideo = true;
    loadWindowPrefs();
    expect(windowPrefs.fitToVideo).toBe(false);
  });

  it('writes the choice so that it is read back', async () => {
    windowPrefs.fitToVideo = true;
    toggleWindowPref('fitToVideo');
    await settle();
    windowPrefs.fitToVideo = true;
    loadWindowPrefs();
    expect(windowPrefs.fitToVideo).toBe(false);
  });
});
