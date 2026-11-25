import { describe, expect, it } from 'vitest';
import { FullscreenRequests } from './fullscreen-request';

/** A page whose veil and window answer only when told to. */
function harness(veil = true) {
  const sent: boolean[] = [];
  const veils: Array<() => void> = [];
  const answers: Array<() => void> = [];
  let mirror = false;
  const fs = new FullscreenRequests({
    mirror: () => mirror,
    setMirror: (on) => (mirror = on),
    prepare: () => (veil ? new Promise<void>((r) => veils.push(r)) : null),
    send: (on) => {
      sent.push(on);
      return new Promise<void>((r) => answers.push(r));
    },
  });
  // What the page's toggle does: decide against `current()`, ask for the other state.
  const toggle = () => fs.request(!fs.current());
  const settle = async () => {
    for (let i = 0; i < 10; i++) {
      veils.splice(0).forEach((r) => r());
      answers.splice(0).forEach((r) => r());
      await new Promise((r) => setTimeout(r, 0));
    }
  };
  const openVeils = () => veils.splice(0).forEach((r) => r());
  return { fs, sent, toggle, settle, openVeils, mirror: () => mirror };
}

describe('fullscreen requests', () => {
  it('in-out-in-out inside one veil asks for each state in turn and sends only the last', async () => {
    const h = harness();
    const decided: boolean[] = [];
    for (let i = 0; i < 4; i++) {
      decided.push(!h.fs.current());
      void h.toggle();
    }
    // The bug this replaces: in, out, in, in (the third and fourth read a stale mirror).
    expect(decided).toEqual([true, false, true, false]);
    expect(h.fs.pending()).toBe(true);
    await h.settle();
    expect(h.sent).toEqual([false]);
    expect(h.mirror()).toBe(false);
    expect(h.fs.pending()).toBe(false);
  });

  it('an odd number of toggles ends in fullscreen', async () => {
    const h = harness();
    for (let i = 0; i < 3; i++) void h.toggle();
    await h.settle();
    expect(h.sent).toEqual([true]);
    expect(h.mirror()).toBe(true);
  });

  it('a toggle while the window is still answering is decided against the request, and sent after it', async () => {
    const h = harness();
    const first = h.toggle();
    await new Promise((r) => setTimeout(r, 0));
    // Let the veil through: the first request is out, its answer still pending.
    h.openVeils();
    await new Promise((r) => setTimeout(r, 0));
    expect(h.sent).toEqual([true]);
    expect(h.fs.current()).toBe(true);
    void h.toggle();
    expect(h.fs.current()).toBe(false);
    await h.settle();
    await first;
    expect(h.sent).toEqual([true, false]);
    expect(h.mirror()).toBe(false);
    expect(h.fs.pending()).toBe(false);
  });

  it('with no veil (a host that queues) the request goes out in the same tick', () => {
    const h = harness(false);
    void h.toggle();
    expect(h.sent).toEqual([true]);
    expect(h.mirror()).toBe(true);
    void h.toggle();
    void h.toggle();
    expect(h.sent).toEqual([true, false, true]);
  });

  it('the mirror decides once nothing is pending', async () => {
    const h = harness();
    void h.toggle();
    await h.settle();
    expect(h.fs.pending()).toBe(false);
    expect(h.fs.current()).toBe(true);
  });
});
