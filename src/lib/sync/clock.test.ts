import { describe, expect, it } from 'vitest';

import { SAMPLE_WINDOW, estimateOffset, offsetUncertainty, pushSample, sampleOf } from './clock';

// The clock offset is the number the whole shared timeline is stated in, and
// being wrong about it is silent: every viewer sits at the wrong position while
// drift correction dutifully holds them there, because as far as it can tell
// they are exactly where they should be. Hence a test.

describe('sampleOf', () => {
  it('measures the round trip and splits it', () => {
    // Sent at 1000, the relay answered saying 5500, back at 1200. The trip took
    // 200 ms, so the relay's reading corresponds to about 1100 of ours.
    expect(sampleOf(1000, 5500, 1200)).toEqual({ rtt: 200, offset: 5500 + 100 - 1200 });
  });

  it('handles a perfectly symmetric zero-latency trip', () => {
    expect(sampleOf(1000, 4000, 1000)).toEqual({ rtt: 0, offset: 3000 });
  });

  it('refuses a round trip that cannot have happened', () => {
    // A clock stepped mid-flight — NTP, or a laptop waking — produces this, and
    // believing it would poison the window with an offset that is pure artefact.
    expect(sampleOf(2000, 5000, 1000)).toBeNull();
    expect(sampleOf(NaN, 5000, 1000)).toBeNull();
  });
});

describe('pushSample', () => {
  it('keeps the window to its length, oldest out first', () => {
    let samples: ReturnType<typeof pushSample> = [];
    for (let i = 0; i < SAMPLE_WINDOW + 4; i++) {
      samples = pushSample(samples, { rtt: i, offset: i });
    }
    expect(samples).toHaveLength(SAMPLE_WINDOW);
    // The window has to follow a route that changes — a laptop moving from
    // Wi-Fi to a hotspot mid-film — so the oldest go, not the slowest.
    expect(samples[0].rtt).toBe(4);
    expect(samples.at(-1)?.rtt).toBe(SAMPLE_WINDOW + 3);
  });
});

describe('estimateOffset', () => {
  it('is zero before anything has been measured', () => {
    expect(estimateOffset([])).toBe(0);
  });

  it('takes one sample at its word', () => {
    expect(estimateOffset([{ rtt: 40, offset: 250 }])).toBe(250);
  });

  it('ignores a slow round trip, which is where asymmetry lives', () => {
    // Four fast samples agreeing on 100, and one slow one that a mean would let
    // drag the answer by 180 ms.
    const samples = [
      { rtt: 20, offset: 100 },
      { rtt: 22, offset: 102 },
      { rtt: 24, offset: 98 },
      { rtt: 26, offset: 100 },
      { rtt: 900, offset: 1000 },
    ];
    // The fastest half of five is two samples, [100, 102], whose median is 101.
    // The mean of all five would be 280.
    expect(estimateOffset(samples)).toBe(101);
  });

  it('uses a median, so one outlier among the fast samples cannot drag it', () => {
    const samples = [
      { rtt: 10, offset: 100 },
      { rtt: 11, offset: 5000 },
      { rtt: 12, offset: 104 },
      { rtt: 13, offset: 106 },
      { rtt: 14, offset: 108 },
      { rtt: 15, offset: 110 },
    ];
    // The fastest half is [100, 5000, 104]; its median is 104, while its mean
    // would be over 1700.
    expect(estimateOffset(samples)).toBe(104);
  });

  it('averages the two middle values on an even window', () => {
    // Picking either one instead would make the estimate jump by the gap between
    // them whenever a sample ages out.
    const samples = [
      { rtt: 10, offset: 100 },
      { rtt: 11, offset: 110 },
      { rtt: 12, offset: 200 },
      { rtt: 13, offset: 300 },
    ];
    expect(estimateOffset(samples)).toBe(105);
  });

  it('follows a clock that has moved rather than averaging over the change', () => {
    // Eight samples where the last four sit 500 ms away: the window is short
    // enough that once they are the fastest half, they win.
    let samples: ReturnType<typeof pushSample> = [];
    for (let i = 0; i < SAMPLE_WINDOW; i++) samples = pushSample(samples, { rtt: 200, offset: 0 });
    for (let i = 0; i < SAMPLE_WINDOW / 2; i++) samples = pushSample(samples, { rtt: 20, offset: 500 });
    expect(estimateOffset(samples)).toBe(500);
  });
});

describe('offsetUncertainty', () => {
  it('is half the fastest round trip', () => {
    expect(offsetUncertainty([{ rtt: 200, offset: 0 }, { rtt: 60, offset: 0 }])).toBe(30);
  });

  it('is unbounded before anything has been measured', () => {
    expect(offsetUncertainty([])).toBe(Infinity);
  });
});
