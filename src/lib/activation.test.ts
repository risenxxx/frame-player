import { describe, expect, it } from 'vitest';

import { ActivationGate } from './activation';

const INTERVAL = 500;

describe('ActivationGate', () => {
  it('is inert where the platform delivers the activating click itself', () => {
    const gate = new ActivationGate(false);
    expect(gate.swallows(1000, false, INTERVAL)).toBe(false);
    gate.activated(1000);
    expect(gate.swallows(1010, true, INTERVAL)).toBe(false);
  });

  it('lets an ordinary click on an active window through', () => {
    const gate = new ActivationGate(true);
    gate.activated(0);
    expect(gate.swallows(10_000, true, INTERVAL)).toBe(false);
  });

  it('swallows the click that arrives before the page knows it is focused, and the double click on it', () => {
    const gate = new ActivationGate(true);
    gate.activated(0);
    // The activating click: the page still says it is not focused.
    expect(gate.swallows(10_000, false, INTERVAL)).toBe(true);
    // The focus event lands a moment later, then the second click of a double
    // click — the page is focused by now, and the click still must not pause.
    gate.activated(10_005);
    expect(gate.swallows(10_200, true, INTERVAL)).toBe(true);
    // Past the interval the window is simply active, and a click is a click.
    expect(gate.swallows(10_700, true, INTERVAL)).toBe(false);
  });

  it('covers the other order too: focus first, then the click', () => {
    const gate = new ActivationGate(true);
    gate.activated(20_000);
    expect(gate.swallows(20_003, true, INTERVAL)).toBe(true);
    expect(gate.swallows(20_150, true, INTERVAL)).toBe(true);
    expect(gate.swallows(20_600, true, INTERVAL)).toBe(false);
  });

  it('honours the interval it is given rather than a constant', () => {
    const gate = new ActivationGate(true);
    gate.activated(0);
    expect(gate.swallows(300, true, 250)).toBe(false);
    expect(gate.swallows(300, true, 1000)).toBe(true);
  });
});
