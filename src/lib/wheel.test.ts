import { describe, expect, it } from 'vitest';

import { WheelGesture, wheelChannel, type WheelInput, type WheelMode } from './wheel';

const ev = (deltaX: number, deltaY: number, shiftKey = false, deltaMode = 0): WheelInput => ({
  deltaX,
  deltaY,
  deltaMode,
  shiftKey,
});

/// A gesture fed a list of events on Windows with a file open, step 5 s.
function run(events: WheelInput[], mode: WheelMode = 'volume', step = 5, mac = false) {
  const g = new WheelGesture();
  return events.map((e) => g.feed(e, mode, step, mac, true));
}

describe('wheelChannel', () => {
  it('reads the vertical wheel as the setting says', () => {
    expect(wheelChannel(ev(0, -100), 'volume')).toBe('volume');
    expect(wheelChannel(ev(0, -100), 'seek')).toBe('notch');
  });

  it('gives Shift+wheel the other one', () => {
    expect(wheelChannel(ev(0, 100, true), 'volume')).toBe('notch');
    expect(wheelChannel(ev(0, 100, true), 'seek')).toBe('volume');
  });

  it('keeps a real horizontal delta a continuous scrub in either mode', () => {
    expect(wheelChannel(ev(30, 2), 'volume')).toBe('scrub');
    expect(wheelChannel(ev(30, 2), 'seek')).toBe('scrub');
  });

  it('stands aside for Shift when the event has its own horizontal delta', () => {
    // A trackpad swipe with Shift held is still a swipe.
    expect(wheelChannel(ev(30, 2, true), 'volume')).toBe('scrub');
    expect(wheelChannel(ev(30, 2, true), 'seek')).toBe('scrub');
  });

  it('waits out motion too small to tell', () => {
    expect(wheelChannel(ev(1, 1), 'volume')).toBeNull();
    expect(wheelChannel(ev(0, 1.5), 'seek')).toBeNull();
  });
});

describe('WheelGesture', () => {
  it('keeps five volume units per Windows notch, up is louder', () => {
    expect(run([ev(0, -100), ev(0, 100)])).toEqual([{ volume: 5 }, { volume: -5 }]);
  });

  it('turns the macOS sign around for volume', () => {
    expect(run([ev(0, 100)], 'volume', 5, true)).toEqual([{ volume: 5 }]);
  });

  it('seeks the chosen seconds per notch in seek mode, up is forward', () => {
    expect(run([ev(0, -100)], 'seek', 1)).toEqual([{ seek: 1 }]);
    expect(run([ev(0, -100)], 'seek', 5)).toEqual([{ seek: 5 }]);
    expect(run([ev(0, -100), ev(0, 100)], 'seek', 30)).toEqual([{ seek: 30 }, { seek: -30 }]);
    expect(run([ev(0, 100)], 'seek', 10, true)).toEqual([{ seek: 10 }]);
  });

  it('collects a trackpad\'s small deltas into whole seconds', () => {
    // 1 s per notch is 100 px a second: four 30 px events make one second and
    // carry the rest.
    expect(run([ev(0, -30), ev(0, -30), ev(0, -30), ev(0, -30)], 'seek', 1)).toEqual([
      null,
      null,
      null,
      { seek: 1 },
    ]);
  });

  it('applies the notch step to Shift+wheel in volume mode, down is forward', () => {
    // Shift+wheel is the mouse's horizontal scroll: down becomes right, on
    // both platforms, since `deltaY` is not flipped by natural scrolling.
    expect(run([ev(0, 100, true)], 'volume', 10)).toEqual([{ seek: 10 }]);
    expect(run([ev(0, 100, true)], 'volume', 10, true)).toEqual([{ seek: 10 }]);
    expect(run([ev(0, -100, true)], 'volume', 1)).toEqual([{ seek: -1 }]);
  });

  it('makes Shift+wheel the volume in seek mode', () => {
    expect(run([ev(0, -100, true)], 'seek')).toEqual([{ volume: 5 }]);
  });

  it('scrubs a real horizontal delta at 20 px a second whatever the step', () => {
    expect(run([ev(100, 0)], 'seek', 30)).toEqual([{ seek: 5 }]);
    expect(run([ev(100, 0)], 'volume', 1)).toEqual([{ seek: 5 }]);
    // macOS: a rightward swipe arrives as deltaX < 0.
    expect(run([ev(-100, 0)], 'volume', 5, true)).toEqual([{ seek: 5 }]);
  });

  it('holds the channel through diagonal noise', () => {
    // Starts horizontal, then drifts vertical: still a scrub, never volume.
    const out = run([ev(40, 0), ev(10, 30), ev(5, 40)]);
    expect(out.every((r) => r === null || 'seek' in r)).toBe(true);
  });

  it('re-decides at once when Shift goes down mid-gesture, dropping what was collected', () => {
    const g = new WheelGesture();
    // 15 px of volume collected — not yet a unit.
    expect(g.feed(ev(0, -15), 'volume', 5, false, true)).toBeNull();
    // Shift: now a seek, and the 15 px do not leak into it.
    expect(g.feed(ev(0, 20, true), 'volume', 5, false, true)).toEqual({ seek: 1 });
    expect(g.channel).toBe('notch');
  });

  it('re-decides again when Shift comes back up', () => {
    const g = new WheelGesture();
    g.feed(ev(0, 100, true), 'seek', 5, false, true);
    expect(g.channel).toBe('volume');
    expect(g.feed(ev(0, -100), 'seek', 5, false, true)).toEqual({ seek: 5 });
    expect(g.channel).toBe('notch');
  });

  it('does not let Shift take over a swipe that brings its own horizontal delta', () => {
    const g = new WheelGesture();
    g.feed(ev(40, 0), 'volume', 5, false, true);
    g.feed(ev(40, 2, true), 'volume', 5, false, true);
    expect(g.channel).toBe('scrub');
  });

  it('locks to a seek with nothing to seek, rather than falling through to volume', () => {
    const g = new WheelGesture();
    expect(g.feed(ev(0, -100), 'seek', 5, false, false)).toBeNull();
    expect(g.channel).toBe('notch');
    expect(g.feed(ev(100, 0, true), 'volume', 5, false, false)).toBeNull();
  });

  it('counts lines and pages in pixels', () => {
    // Three lines of 16 px.
    expect(run([ev(0, -3, false, 1)], 'volume')).toEqual([{ volume: 2 }]);
  });

  it('starts afresh after reset', () => {
    const g = new WheelGesture();
    g.feed(ev(100, 0), 'volume', 5, false, true);
    g.reset();
    expect(g.feed(ev(0, -100), 'volume', 5, false, true)).toEqual({ volume: 5 });
  });
});
