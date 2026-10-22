/**
 * The frame stepper's conversation with mpv. What these guard is ordering:
 * mpv's absolute seeks replace a queued one, so the direction has to turn
 * before anything else seeks, and the decoder has to come back after it.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';

/** Everything sent to mpv, in order: `set name=value` or `cmd name args`. */
const sent: string[] = [];
const props: Record<string, string | number | null> = {};
let clock = 0;
/// How long the next `frame-back-step` takes, on the fake clock.
let backStepMs = 300;
/// Where `time-pos` lands after the next step backward (null: one frame back).
let nextLanding: number | null = null;

const FRAME = 1 / 60;

vi.mock('tauri-plugin-libmpv-api', () => ({
  command: vi.fn(async (name: string, args: unknown[] = []) => {
    sent.push(`cmd ${name} ${args.join(' ')}`.trim());
    const pos = props['time-pos'] as number;
    if (name === 'frame-back-step') {
      clock += backStepMs;
      props['time-pos'] = pos - FRAME;
    } else if (name === 'frame-step') {
      const back = props['play-direction'] === 'backward';
      props['time-pos'] = back ? (nextLanding ?? pos - FRAME) : pos + FRAME;
      nextLanding = null;
    } else if (name === 'seek') {
      props['time-pos'] = args[0] as number;
    }
  }),
  getProperty: vi.fn(async (name: string) => props[name] ?? null),
  setProperty: vi.fn(async (name: string, value: string) => {
    sent.push(`set ${name}=${value}`);
    const plain = name.replace('file-local-options/', '');
    props[plain] = value;
    if (plain === 'hwdec') props['hwdec-current'] = value === 'no' ? 'no' : 'videotoolbox';
  }),
}));

const probes: Array<{ path: string; pos: number }> = [];
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (cmd: string, args: { path: string; pos: number }) => {
    if (cmd !== 'frame_step_probe') return undefined;
    probes.push(args);
    return { start: 30, next: 40, prev: 20, longest: 10, ram: 36 * 1024 ** 3 };
  }),
}));

const player = {
  hasFile: true,
  filePath: '/films/long-gop.webm' as string | null,
  timePos: 34.5,
  paused: true,
  eofReached: false,
  loopA: null as number | null,
  loopB: null as number | null,
};

vi.mock('./player.svelte', () => ({
  player,
  isNetworkSource: (p: string | null) => !!p && /^https?:/.test(p),
  waitPlaybackSettled: async () => {},
  waitTimePosChange: async () => {},
}));

const fs = await import('./frame-step.svelte');
const { frameStep, frameStepBy, frameStepBeforeSeek, frameStepFileChanged, frameStepNotePause } = fs;

function open(path: string, hwdec = 'videotoolbox') {
  frameStepFileChanged();
  player.filePath = path;
  Object.assign(props, {
    'time-pos': 34.5,
    'container-fps': 60,
    'video-params/w': 1920,
    'video-params/h': 1080,
    'video-params/average-bpp': 12,
    hwdec: hwdec === 'no' ? 'no' : 'auto-safe',
    'hwdec-current': hwdec,
    'play-direction': 'forward',
  });
  sent.length = 0;
  probes.length = 0;
}

beforeEach(() => {
  vi.spyOn(performance, 'now').mockImplementation(() => clock);
  backStepMs = 300;
  nextLanding = null;
  frameStep.enabled = true;
  player.paused = true;
  player.loopA = player.loopB = null;
  open('/films/long-gop.webm');
});

describe('a step back', () => {
  it('takes the decoder to software before the first seek, and only once', async () => {
    await frameStepBy(-1);
    expect(sent.slice(0, 2)).toEqual(['set file-local-options/hwdec=no', 'cmd frame-back-step']);
    expect(probes).toEqual([{ path: '/films/long-gop.webm', pos: 34.5 }]);
  });

  it('decodes backward from the second press on a slow file', async () => {
    await frameStepBy(-1);
    sent.length = 0;
    await frameStepBy(-1);
    expect(sent).toEqual([
      `set file-local-options/video-reversal-buffer=${4 * 1024 ** 3}`,
      'set file-local-options/play-direction=backward',
      'cmd frame-step 1 mute',
    ]);
    expect(frameStep.reverse).toBe(true);
    sent.length = 0;
    await frameStepBy(-1);
    expect(sent).toEqual(['cmd frame-step 1 mute']);
  });

  it('stays native while steps back alternate with steps forward', async () => {
    await frameStepBy(-1);
    await frameStepBy(1);
    await frameStepBy(-1);
    await frameStepBy(1);
    await frameStepBy(-1);
    expect(frameStep.reverse).toBe(false);
    expect(sent.filter((s) => s === 'cmd frame-back-step')).toHaveLength(3);
    await frameStepBy(-1);
    expect(frameStep.reverse).toBe(true);
  });

  it('lets a later slow step overturn a fast first one', async () => {
    backStepMs = 40;
    await frameStepBy(-1);
    backStepMs = 300;
    await frameStepBy(-1);
    expect(frameStep.reverse).toBe(false);
    await frameStepBy(-1);
    expect(frameStep.reverse).toBe(true);
  });

  it('stays native on a file where a step back is cheap', async () => {
    backStepMs = 40;
    await frameStepBy(-1);
    await frameStepBy(-1);
    expect(frameStep.reverse).toBe(false);
    expect(sent.filter((s) => s === 'cmd frame-back-step')).toHaveLength(2);
  });

  it('stays native when the viewer switched it off', async () => {
    frameStep.enabled = false;
    await frameStepBy(-1);
    await frameStepBy(-1);
    expect(frameStep.reverse).toBe(false);
  });

  it('never probes or reverses a stream', async () => {
    open('https://example.invalid/video');
    await frameStepBy(-1);
    await frameStepBy(-1);
    expect(probes).toEqual([]);
    expect(frameStep.reverse).toBe(false);
  });

  it('keeps hardware decoding for a 4K picture, and therefore stays native', async () => {
    props['video-params/w'] = 3840;
    props['video-params/h'] = 2160;
    await frameStepBy(-1);
    await frameStepBy(-1);
    expect(sent).not.toContain('set file-local-options/hwdec=no');
    expect(frameStep.reverse).toBe(false);
  });

  it('does nothing on the first frame', async () => {
    props['time-pos'] = 0;
    await frameStepBy(-1);
    expect(sent).toEqual([]);
  });
});

describe('a bad landing', () => {
  it('turns forward, puts the picture where it belonged, and gives up on the file', async () => {
    await frameStepBy(-1);
    await frameStepBy(-1); // enters
    const before = props['time-pos'] as number;
    nextLanding = before - 5; // the buffer overflowed
    sent.length = 0;
    await frameStepBy(-1);
    expect(sent).toEqual([
      'cmd frame-step 1 mute',
      'set file-local-options/play-direction=forward',
      `cmd seek ${before - FRAME} absolute+exact`,
    ]);
    expect(frameStep.reverse).toBe(false);
    sent.length = 0;
    await frameStepBy(-1);
    expect(sent).toEqual(['cmd frame-back-step']);
  });
});

describe('leaving a session', () => {
  it('turns forward before giving the decoder back, both before the seek', async () => {
    await frameStepBy(-1);
    await frameStepBy(-1);
    sent.length = 0;
    const settle = frameStepBeforeSeek();
    expect(settle).not.toBeNull();
    await settle;
    expect(sent).toEqual([
      'set file-local-options/play-direction=forward',
      'set file-local-options/hwdec=auto-safe',
    ]);
    expect(frameStepBeforeSeek()).toBeNull();
  });

  it('turns forward before a step forward', async () => {
    await frameStepBy(-1);
    await frameStepBy(-1);
    sent.length = 0;
    await frameStepBy(1);
    expect(sent).toEqual(['set file-local-options/play-direction=forward', 'cmd frame-step 1 mute']);
  });

  it('keeps software through playback and gives the decoder back at the next pause', async () => {
    await frameStepBy(-1);
    await frameStepBy(-1);
    sent.length = 0;
    clock += 1000;
    player.paused = false;
    frameStepNotePause();
    // Somebody else started playback while mpv was going backward.
    expect(sent).toEqual(['set file-local-options/play-direction=forward']);
    // Once the turn is in, a seek during playback does not touch the decoder:
    // a change now would feed the new decoder mid-group packets.
    await vi.waitFor(() => expect(frameStepBeforeSeek()).toBeNull());
    sent.length = 0;
    player.paused = true;
    frameStepNotePause();
    await Promise.resolve();
    expect(sent).toEqual(['set file-local-options/hwdec=auto-safe']);
  });

  it('gives the decoder back before our own play control unpauses', async () => {
    await frameStepBy(-1);
    await frameStepBy(-1);
    sent.length = 0;
    const before = fs.frameStepBeforePlay();
    expect(before).not.toBeNull();
    await before;
    expect(sent).toEqual([
      'set file-local-options/play-direction=forward',
      'set file-local-options/hwdec=auto-safe',
    ]);
    // Nothing left for the resume itself to undo, and nothing at the pause after.
    expect(fs.frameStepBeforePlay()).toBeNull();
    sent.length = 0;
    player.paused = false;
    frameStepNotePause();
    player.paused = true;
    frameStepNotePause();
    await Promise.resolve();
    expect(sent).toEqual([]);
  });

  it('gives the decoder back with a seek made while paused', async () => {
    await frameStepBy(-1);
    sent.length = 0;
    await frameStepBeforeSeek();
    expect(sent).toEqual(['set file-local-options/hwdec=auto-safe']);
  });

  it('does not give the decoder back at the pause its own step makes', async () => {
    await frameStepBy(-1);
    sent.length = 0;
    frameStepNotePause();
    expect(sent).toEqual([]);
  });

  it('forgets everything on a new file, where mpv has reset the options itself', async () => {
    await frameStepBy(-1);
    await frameStepBy(-1);
    open('/films/next.webm');
    expect(frameStep.reverse).toBe(false);
    expect(frameStepBeforeSeek()).toBeNull();
  });
});
