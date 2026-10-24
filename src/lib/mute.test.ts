/**
 * The mute button is the one control whose failure is invisible from the code
 * that calls it: the popup changes either way. Both ways it went wrong are
 * pinned here — toggling the mirror's opposite (a stale mirror sets what mpv
 * already has) and announcing a mute over a bitstream, which no mute reaches.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';

let props: Record<string, unknown> = {};
const commands: string[][] = [];
const osd: string[] = [];
const writes: [string, unknown][] = [];

vi.mock('tauri-plugin-libmpv-api', () => ({
  command: vi.fn(async (name: string, args: string[]) => {
    commands.push([name, ...args]);
  }),
  getProperty: vi.fn(async (name: string) => props[name] ?? null),
  setProperty: vi.fn(async (name: string, value: unknown) => {
    writes.push([name, value]);
  }),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => undefined) }));
vi.mock('./osd.svelte', async (orig) => ({
  ...(await orig<typeof import('./osd.svelte')>()),
  showOsd: vi.fn((text: string) => osd.push(text)),
}));

import { t } from './i18n.svelte';
import { player, toggleMute, unmute } from './player.svelte';

beforeEach(() => {
  props = {};
  commands.length = 0;
  osd.length = 0;
  writes.length = 0;
});

describe('toggleMute', () => {
  it('cycles rather than writing the stale mirror back', async () => {
    // The mirror says muted, mpv says not: the old code set mute=false again.
    player.muted = true;
    props = { mute: false, 'audio-out-params/format': 'floatp' };
    await toggleMute();
    expect(commands).toEqual([['cycle', 'mute']]);
    expect(osd).toEqual([t('osd.sound_off')]);
  });

  it('says where the volume is while the sound is a bitstream', async () => {
    props = { mute: false, 'audio-out-params/format': 'spdif-eac3' };
    await toggleMute();
    expect(commands).toEqual([]);
    expect(osd).toEqual([t('osd.volume_spdif')]);
  });
});

describe('unmute', () => {
  // The bar of a muted player stands at zero, so raising it has to bring the
  // sound back, or the bar snaps back to zero under the pointer.
  it('takes the mute off as an absolute write', () => {
    player.muted = true;
    unmute();
    expect(writes).toEqual([['mute', false]]);
  });

  it('writes nothing when the sound is already on', () => {
    player.muted = false;
    unmute();
    expect(writes).toEqual([]);
  });
});
