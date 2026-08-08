/**
 * The hotkey table, where the failures are all of the same kind: a binding that
 * looks accepted and never fires, or one that quietly takes a key from
 * somewhere else.
 *
 * Two properties carry most of the weight. **Overrides are stored sparsely** —
 * only what the viewer changed — so a default corrected in a later version still
 * reaches anyone who has opened the editor; storing the whole table once would
 * freeze every default at whatever it was that day, silently and for good.
 * And **assigning a chord takes it off every other action**, not only off the one
 * the lookup map names, or a row keeps advertising a key it no longer answers.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';

import { setLocale } from './i18n.svelte';
import {
  ACTIONS,
  actionFor,
  actionLabel,
  assign,
  chordLabel,
  chordOf,
  chordsOf,
  codeOf,
  hasCustomBindings,
  hint,
  isCustom,
  isDigitJump,
  isModifierCode,
  reservedReason,
  resetAction,
  resetAll,
  unassign,
} from './keys.svelte';

const STORAGE_KEY = 'frameplayer.keys';

const key = (code: string, mods: Partial<KeyboardEvent> = {}) =>
  ({ code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...mods }) as KeyboardEvent;

beforeEach(() => {
  localStorage.clear();
  resetAll();
});

describe('chordOf', () => {
  it('writes the modifiers in one fixed order, so chords compare as strings', () => {
    expect(chordOf(key('KeyS'))).toBe('KeyS');
    expect(
      chordOf(key('KeyS', { shiftKey: true, ctrlKey: true, altKey: true, metaKey: true })),
    ).toBe('ctrl+alt+shift+meta+KeyS');
    // Same set, and the order of the flags in the event cannot change the string.
    expect(chordOf(key('KeyS', { metaKey: true, ctrlKey: true }))).toBe('ctrl+meta+KeyS');
  });

  it('uses the physical key, so a binding survives a layout change', () => {
    // `e.code`, never `e.key`: on a Russian layout the same physical key
    // reports "я", and a binding stored as a letter would stop working.
    expect(chordOf(key('KeyZ'))).toBe('KeyZ');
  });

  it('codeOf strips the modifiers back off', () => {
    expect(codeOf('ctrl+alt+shift+meta+KeyS')).toBe('KeyS');
    expect(codeOf('KeyS')).toBe('KeyS');
  });
});

describe('reserved chords', () => {
  it('keeps the bare digits as a family', () => {
    // Ten keys for one parameterised action: letting one be taken would punch a
    // hole in the family with nothing in the editor to show for it.
    expect(reservedReason('Digit0')).toBe('digits');
    expect(reservedReason('Digit7')).toBe('digits');
    // With a modifier they are ordinary chords again.
    expect(reservedReason('ctrl+Digit7')).toBeNull();
  });

  it('keeps Escape and Enter, which are about the dialog stack', () => {
    expect(reservedReason('Escape')).toBe('contextual');
    expect(reservedReason('Enter')).toBe('contextual');
    expect(reservedReason('NumpadEnter')).toBe('contextual');
    // Tab is reserved with or without modifiers: it moves focus.
    expect(reservedReason('Tab')).toBe('contextual');
    expect(reservedReason('shift+Tab')).toBe('contextual');
  });

  it('lets an ordinary key through', () => {
    expect(reservedReason('KeyJ')).toBeNull();
    expect(reservedReason('ctrl+shift+KeyP')).toBeNull();
  });
});

describe('isDigitJump', () => {
  it('is the bare digit only', () => {
    expect(isDigitJump(key('Digit3'))).toBe(true);
    expect(isDigitJump(key('Digit3', { ctrlKey: true }))).toBe(false);
    expect(isDigitJump(key('Digit3', { shiftKey: true }))).toBe(false);
    expect(isDigitJump(key('KeyA'))).toBe(false);
  });
});

describe('isModifierCode', () => {
  it('knows a lone modifier is not yet a chord', () => {
    // The recorder waits rather than storing "shift+".
    for (const c of ['ShiftLeft', 'ControlRight', 'AltLeft', 'MetaLeft'])
      expect(isModifierCode(c)).toBe(true);
    expect(isModifierCode('KeyA')).toBe(false);
  });
});

describe('assign / unassign', () => {
  it('takes the chord off whoever had it, and says who that was', () => {
    // A silently stolen binding reads as two features breaking at once.
    // The victim is read from the table rather than named, so the test says
    // what it means — "whoever held it" — and does not rot when a default moves.
    const victim = ACTIONS.find((a) => a.id !== 'mute' && chordsOf(a.id).length > 0)!;
    const chord = chordsOf(victim.id)[0];
    const previous = assign('mute', chord);
    expect(previous).toBe(victim.id);
    expect(actionFor(chord)?.id).toBe('mute');
    expect(chordsOf(victim.id)).not.toContain(chord);
  });

  it('reports nothing when the action already held the chord', () => {
    expect(assign('mute', 'KeyM')).toBeNull();
  });

  it('stores only what changed', () => {
    // Sparse overrides are what let a corrected default still reach someone who
    // has opened the editor.
    assign('mute', 'KeyQ');
    const stored = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '{}');
    expect(Object.keys(stored).sort()).toEqual(['mute']);
    expect(isCustom('mute')).toBe(true);
    expect(isCustom('pause')).toBe(false);
  });

  it('reads the store back on the next launch', async () => {
    // The load happens when the module is evaluated, so this re-evaluates it
    // against a seeded store — the real path, not a re-implementation of it.
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ mute: ['KeyQ'] }));
    vi.resetModules();
    const fresh = await import('./keys.svelte');
    expect(fresh.actionFor('KeyQ')?.id).toBe('mute');
    expect(fresh.isCustom('mute')).toBe(true);
  });

  it('does not let a corrupt store take the bindings down', async () => {
    for (const raw of ['not json', '[]', 'null', '{"nosuchaction":["KeyQ"]}', '{"mute":"KeyQ"}']) {
      localStorage.setItem(STORAGE_KEY, raw);
      vi.resetModules();
      const fresh = await import('./keys.svelte');
      // Every default still answers.
      expect(fresh.actionFor('Space')?.id).toBe('pause');
    }
  });

  it('deduplicates a hand-edited chord list', async () => {
    // Svelte keys the editor's badges by the chord string and throws on a
    // duplicate — one `["KeyQ","KeyQ"]` would take the whole panel down.
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ mute: ['KeyQ', 'KeyQ'] }));
    vi.resetModules();
    const fresh = await import('./keys.svelte');
    expect(fresh.chordsOf('mute')).toEqual(['KeyQ']);
  });

  it('unassign leaves the action with no chord rather than its default', () => {
    unassign('mute', 'KeyM');
    expect(chordsOf('mute')).not.toContain('KeyM');
    expect(actionFor('KeyM')).toBeNull();
    // ...and `hint` yields an empty string, so a menu row does not read
    // "Звук ()".
    expect(hint('mute')).toBe('');
  });

  it('resetAction puts back this build’s default, not the one that was stored', () => {
    const original = chordsOf('mute');
    assign('mute', 'KeyQ');
    resetAction('mute');
    expect(chordsOf('mute')).toEqual(original);
    expect(isCustom('mute')).toBe(false);
  });

  it('resetAll clears everything and the button that offers it', () => {
    expect(hasCustomBindings()).toBe(false);
    assign('mute', 'KeyQ');
    expect(hasCustomBindings()).toBe(true);
    resetAll();
    expect(hasCustomBindings()).toBe(false);
  });
});

describe('the shipped table', () => {
  it('has no two actions on one chord', () => {
    // A collision means one of them silently never fires.
    const seen = new Map<string, string>();
    for (const def of ACTIONS) {
      for (const chord of chordsOf(def.id)) {
        expect(seen.get(chord), `${chord} held by ${seen.get(chord)} and ${def.id}`).toBeUndefined();
        seen.set(chord, def.id);
      }
    }
  });

  it('binds nothing the player has reserved', () => {
    // A default on a reserved chord would look bound in the editor and never
    // fire, because the reserved family is checked first in `onKeydown`.
    for (const def of ACTIONS) {
      for (const chord of chordsOf(def.id)) {
        expect(reservedReason(chord), `${def.id} on ${chord}`).toBeNull();
      }
    }
  });

  it('gives every action a non-empty label in both languages', () => {
    // `actionLabel` is typed against the dictionary, so a *missing* key is a
    // compile error — but an empty string is not, and it renders as a blank row
    // in the editor with a key badge beside it.
    for (const locale of ['en', 'ru'] as const) {
      setLocale(locale);
      for (const def of ACTIONS) {
        expect(actionLabel(def.id).trim(), `${def.id} in ${locale}`).not.toBe('');
      }
    }
    setLocale('en');
  });
});

describe('chordLabel', () => {
  it('prints the US-QWERTY name of the physical key', () => {
    // Deliberately not the active layout: `navigator.keyboard` is Blink-only, so
    // using it would print different badges on Windows and macOS from one build.
    expect(chordLabel('KeyZ')).toContain('Z');
    expect(chordLabel('Digit1')).toContain('1');
  });

  it('spells out the modifiers', () => {
    const label = chordLabel('ctrl+shift+KeyS');
    expect(label).toMatch(/S$/);
    expect(label.length).toBeGreaterThan(1);
  });
});
