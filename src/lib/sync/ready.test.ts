/**
 * The readiness rule, which is the one decision in watching together that
 * cannot be seen from outside.
 *
 * Every case below is a room's evening going wrong in one of two directions,
 * and neither prints anything: a member stuck "loading" freezes the film for
 * everybody until the relay's grace expires, and a member who claims to be
 * ready while nothing is on their screen is simply left behind. Both had
 * shipped, at the same time, in the same expression.
 */

import { describe, expect, it } from 'vitest';

import { isBusy, type Readiness } from './ready';

/// A guest in a room that is watching something, with their own copy playing.
function guest(over: Partial<Readiness> = {}): Readiness {
  return {
    hasFile: true,
    playing: true,
    stalled: false,
    opening: false,
    failed: false,
    unopenable: false,
    roomHasContent: true,
    ...over,
  };
}

describe('holding the room up', () => {
  it('does not, while the film is running', () => {
    expect(isBusy(guest())).toBe(false);
  });

  it('does while the room is being caught up with', () => {
    // Nothing open, and the room is watching something we are about to fetch.
    expect(isBusy(guest({ hasFile: false, playing: false }))).toBe(true);
    // ...and while the fetch itself runs, even though the previous film is
    // still on screen and looks perfectly healthy.
    expect(isBusy(guest({ opening: true }))).toBe(true);
  });

  it('does while playback is stopped for the network', () => {
    expect(isBusy(guest({ stalled: true }))).toBe(true);
  });

  it('does when mpv has been handed a file it has not started playing', () => {
    // **The case a viewer found.** `filename` is set the moment a load is
    // issued, so a torrent that never delivers a byte is indistinguishable from
    // one that is playing by every mirror in the player — and this player was
    // announcing itself ready in front of a black window while the room went on
    // without it.
    expect(isBusy(guest({ playing: false }))).toBe(true);
  });

  it('stops once the attempt has definitively failed', () => {
    // The other half of the same report. A magnet that timed out will not
    // resolve on the next tick either, so going on saying "buffering" asks an
    // evening to wait for something that is not coming — which is what it did,
    // for the rest of the session, until the relay stopped listening.
    expect(isBusy(guest({ hasFile: false, playing: false, failed: true }))).toBe(false);
  });

  it('stops for something this player cannot open at all', () => {
    // A local file, or one somebody has hidden: waiting changes nothing, and
    // the panel is what tells the viewer to open their own copy.
    expect(isBusy(guest({ hasFile: false, playing: false, unopenable: true }))).toBe(false);
  });

  it('stops in a room where nothing is playing', () => {
    expect(isBusy(guest({ hasFile: false, playing: false, roomHasContent: false }))).toBe(false);
  });

  it('is decided by the open in flight, not by what is on screen', () => {
    // Opening outranks a failure and an unopenable reference alike: those
    // describe the *previous* answer, and this one has not come back yet.
    expect(isBusy(guest({ opening: true, failed: true, unopenable: true }))).toBe(true);
  });
});
