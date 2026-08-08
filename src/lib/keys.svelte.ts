/**
 * Player key bindings — the one place that knows what they are.
 *
 * Bindings stay on `e.code`, the *physical* key, so they work in any keyboard
 * layout; that rule predates this module and the editor must not weaken it.
 * What changes is that the code is no longer written into a `switch`: an action
 * is a row in `ACTIONS`, its chord is looked up here, and everything that
 * *displays* a hotkey reads the same table instead of spelling the key out in
 * markup. There were twenty-two places doing the latter — twelve in markup and
 * eight with the key baked into a translated sentence.
 *
 * Three properties moved here with it, because all three used to be lists of
 * key codes and none of them is a property of a *key* once the user can move it:
 *
 *   - `noRepeat` — a held key must not re-fire a toggle. Seeks, volume and the
 *     delay nudges deliberately do repeat, which is why this is per action and
 *     not a blanket rule.
 *   - `quiet` — the action raises a popup of its own, so lighting up the whole
 *     OSC on top of it is noise.
 *   - the default chord, which differs per platform for two families: chapter
 *     navigation (plain Ctrl+arrow is Mission Control on macOS and never
 *     reaches the webview) and the two Ctrl/Cmd actions.
 *
 * **Modifier overloads are separate actions, not one action reading a flag.**
 * `KeyS` used to be `saveScreenshot(e.shiftKey)` and `ArrowRight` used to be
 * three different things depending on modifiers. Under an editor that is
 * unanswerable — "rebind screenshot" would have to mean rebinding a key *and* a
 * modifier convention — so each ended up a row of its own. That is also what
 * makes conflict detection possible at all.
 *
 * **Labels are US-QWERTY and identical on both platforms.** `e.code` names a
 * physical key and the badge names it back; `navigator.keyboard.getLayoutMap()`
 * would give the active layout's label (Я for `KeyZ` on ЙЦУКЕН) but it is a
 * Blink API with no WebKit implementation, so using it would make the same
 * build show different badges on Windows and macOS. mpv, IINA and VLC all print
 * the US name for the same reason.
 */

import { t, type MessageKey } from './i18n.svelte';
import { IS_MAC } from './platform';

export type ActionId =
  | 'pause'
  | 'seek_back'
  | 'seek_fwd'
  | 'seek_back_precise'
  | 'seek_fwd_precise'
  | 'seek_back_10'
  | 'seek_fwd_10'
  | 'seek_start'
  | 'seek_end'
  | 'frame_prev'
  | 'frame_next'
  | 'chapter_prev'
  | 'chapter_next'
  | 'playlist_prev'
  | 'playlist_next'
  | 'speed_down'
  | 'speed_up'
  | 'loop'
  | 'ab_loop'
  | 'ab_clear'
  | 'volume_up'
  | 'volume_down'
  | 'mute'
  | 'audio_delay_down'
  | 'audio_delay_up'
  | 'sub_delay_down'
  | 'sub_delay_up'
  | 'fullscreen'
  | 'mini'
  | 'info'
  | 'reset_zoom'
  | 'open_file'
  | 'open_link'
  | 'screenshot'
  | 'screenshot_subs'
  | 'copy_frame';

export type ActionGroup = 'playback' | 'seek' | 'audio' | 'subs' | 'view' | 'file';

export interface ActionDef {
  id: ActionId;
  group: ActionGroup;
  /** Default chords. More than one only where a second is idiomatic (J/K/L). */
  def: string[];
  /** Replaces `def` entirely on macOS, where a modifier is unavailable. */
  defMac?: string[];
  /** Holding the key must not re-fire this. */
  noRepeat?: boolean;
  /** Raises a popup of its own — do not also wake the OSC. */
  quiet?: boolean;
  /**
   * Accelerator the macOS menu bar provides for this action. Not a binding of
   * ours and not editable here: the system swallows it before the webview sees
   * it, and `macos_menu.rs` is where it is declared. Listed so the editor does
   * not look as though it is missing something the menu plainly does.
   */
  menuMac?: string;
}

/**
 * Order is the order of the editor, and it is also the tie-break when two
 * actions somehow claim one chord — `assign()` prevents that, but a hand-edited
 * localStorage entry can still produce it and the dispatcher must be
 * deterministic rather than dependent on object key order.
 */
export const ACTIONS: ActionDef[] = [
  // Playback
  { id: 'pause', group: 'playback', def: ['Space', 'KeyK'], noRepeat: true },
  { id: 'speed_down', group: 'playback', def: ['BracketLeft'] },
  { id: 'speed_up', group: 'playback', def: ['BracketRight'] },
  { id: 'loop', group: 'playback', def: ['shift+KeyL'], noRepeat: true, quiet: true },
  { id: 'ab_loop', group: 'playback', def: ['KeyA'], noRepeat: true },
  { id: 'ab_clear', group: 'playback', def: ['shift+KeyA'], noRepeat: true },
  { id: 'playlist_prev', group: 'playback', def: ['PageUp'], noRepeat: true },
  { id: 'playlist_next', group: 'playback', def: ['PageDown'], noRepeat: true },

  // Seeking
  { id: 'seek_back', group: 'seek', def: ['ArrowLeft'], quiet: true },
  { id: 'seek_fwd', group: 'seek', def: ['ArrowRight'], quiet: true },
  { id: 'seek_back_precise', group: 'seek', def: ['shift+ArrowLeft'], quiet: true },
  { id: 'seek_fwd_precise', group: 'seek', def: ['shift+ArrowRight'], quiet: true },
  { id: 'seek_back_10', group: 'seek', def: ['KeyJ'], quiet: true },
  { id: 'seek_fwd_10', group: 'seek', def: ['KeyL'], quiet: true },
  { id: 'seek_start', group: 'seek', def: ['Home'], quiet: true },
  { id: 'seek_end', group: 'seek', def: ['End'], quiet: true },
  { id: 'frame_prev', group: 'seek', def: ['Comma'], quiet: true },
  { id: 'frame_next', group: 'seek', def: ['Period'], quiet: true },
  {
    id: 'chapter_prev',
    group: 'seek',
    def: ['ctrl+ArrowLeft'],
    defMac: ['ctrl+meta+ArrowLeft'],
    noRepeat: true,
    quiet: true,
  },
  {
    id: 'chapter_next',
    group: 'seek',
    def: ['ctrl+ArrowRight'],
    defMac: ['ctrl+meta+ArrowRight'],
    noRepeat: true,
    quiet: true,
  },

  // Audio
  { id: 'volume_up', group: 'audio', def: ['ArrowUp'], quiet: true },
  { id: 'volume_down', group: 'audio', def: ['ArrowDown'], quiet: true },
  { id: 'mute', group: 'audio', def: ['KeyM'], noRepeat: true },
  { id: 'audio_delay_down', group: 'audio', def: ['ctrl+Minus'], quiet: true },
  { id: 'audio_delay_up', group: 'audio', def: ['ctrl+Equal'], quiet: true },

  // Subtitles
  { id: 'sub_delay_down', group: 'subs', def: ['KeyZ'], quiet: true },
  { id: 'sub_delay_up', group: 'subs', def: ['shift+KeyZ'], quiet: true },

  // View
  { id: 'fullscreen', group: 'view', def: ['KeyF'], noRepeat: true, menuMac: 'ctrl+meta+KeyF' },
  { id: 'mini', group: 'view', def: ['KeyP'], noRepeat: true, menuMac: 'shift+meta+KeyM' },
  { id: 'info', group: 'view', def: ['KeyI'], noRepeat: true, menuMac: 'meta+KeyI' },
  { id: 'reset_zoom', group: 'view', def: ['ctrl+Digit0'], quiet: true },

  // File and frames
  { id: 'open_file', group: 'file', def: ['KeyO'], noRepeat: true, menuMac: 'meta+KeyO' },
  {
    id: 'open_link',
    group: 'file',
    def: ['ctrl+KeyL'],
    // ⌘L is the menu accelerator, so on macOS the menu delivers this and the
    // frontend binding never fires — which is why the default *is* ⌘L there:
    // the hint then reads what actually works, and Ctrl+L on a Mac is a
    // combination nobody reaches for while the item sits in the File menu.
    defMac: ['meta+KeyL'],
    quiet: true,
  },
  { id: 'screenshot', group: 'file', def: ['KeyS'], noRepeat: true },
  { id: 'screenshot_subs', group: 'file', def: ['shift+KeyS'], noRepeat: true },
  // ⌘C is deliberately absent from the Edit menu (it is this player's copy-frame
  // key, and a menu item would swallow it), so it is ours to bind on macOS.
  { id: 'copy_frame', group: 'file', def: ['ctrl+KeyC'], defMac: ['meta+KeyC'], noRepeat: true },
];

export const GROUP_ORDER: ActionGroup[] = ['playback', 'seek', 'audio', 'subs', 'view', 'file'];

const BY_ID = new Map(ACTIONS.map((a) => [a.id, a]));

/** Default chords for this platform. */
function defaultChords(def: ActionDef): string[] {
  return IS_MAC && def.defMac ? def.defMac : def.def;
}

// ---------------------------------------------------------------------------
// Chords
// ---------------------------------------------------------------------------

/**
 * A chord is `ctrl+alt+shift+meta+<code>` with the modifiers in that fixed
 * order and only the ones held. The order is canonical rather than the order
 * they were pressed in, so the string can be compared and used as a map key.
 */
export function chordOf(e: KeyboardEvent): string {
  let mods = '';
  if (e.ctrlKey) mods += 'ctrl+';
  if (e.altKey) mods += 'alt+';
  if (e.shiftKey) mods += 'shift+';
  if (e.metaKey) mods += 'meta+';
  return mods + e.code;
}

/** The physical key of a chord, without its modifiers. */
export function codeOf(chord: string): string {
  return chord.slice(chord.lastIndexOf('+') + 1);
}

/** Pressing a modifier on its own is not a chord — the recorder waits. */
export function isModifierCode(code: string): boolean {
  return (
    code === 'ShiftLeft' ||
    code === 'ShiftRight' ||
    code === 'ControlLeft' ||
    code === 'ControlRight' ||
    code === 'AltLeft' ||
    code === 'AltRight' ||
    code === 'MetaLeft' ||
    code === 'MetaRight' ||
    code === 'CapsLock'
  );
}

/**
 * US-QWERTY labels for the keys that are not simply their own letter. Anything
 * absent falls through to the code with its `Key`/`Digit` prefix removed, which
 * covers A–Z and 0–9 exactly.
 */
const LABELS: Record<string, string> = {
  Comma: ',',
  Period: '.',
  Slash: '/',
  Backslash: '\\',
  Semicolon: ';',
  Quote: "'",
  BracketLeft: '[',
  BracketRight: ']',
  Minus: '-',
  Equal: '=',
  Backquote: '`',
  IntlBackslash: '\\',
  IntlRo: '\\',
  IntlYen: '¥',
  ArrowLeft: '←',
  ArrowRight: '→',
  ArrowUp: '↑',
  ArrowDown: '↓',
  NumpadAdd: 'Num +',
  NumpadSubtract: 'Num −',
  NumpadMultiply: 'Num *',
  NumpadDivide: 'Num /',
  NumpadDecimal: 'Num .',
  NumpadEnter: IS_MAC ? '⌤' : 'Num Enter',
};

/** Keys Apple prints as a glyph and Windows prints as a word. */
const PLATFORM_LABELS: Record<string, [mac: string, win: string]> = {
  Enter: ['⏎', 'Enter'],
  Escape: ['⎋', 'Esc'],
  Backspace: ['⌫', 'Backspace'],
  Delete: ['⌦', 'Del'],
  Tab: ['⇥', 'Tab'],
  Home: ['↖', 'Home'],
  End: ['↘', 'End'],
  PageUp: ['⇞', 'PgUp'],
  PageDown: ['⇟', 'PgDn'],
};

/** Label of one physical key. `Space` is the only one worth translating. */
export function keyLabel(code: string): string {
  if (code === 'Space') return t('key.space');
  const platform = PLATFORM_LABELS[code];
  if (platform) return IS_MAC ? platform[0] : platform[1];
  const label = LABELS[code];
  if (label) return label;
  if (code.startsWith('Key')) return code.slice(3);
  if (code.startsWith('Digit')) return code.slice(5);
  if (code.startsWith('Numpad')) return `Num ${code.slice(6)}`;
  return code;
}

/**
 * A chord as the user reads it: Apple's own modifier glyphs in Apple's own
 * order (⌃⌥⇧⌘) and no separator on macOS, spelled-out and `+`-joined elsewhere.
 */
export function chordLabel(chord: string): string {
  const code = codeOf(chord);
  const has = (m: string) => chord.includes(`${m}+`);
  if (IS_MAC) {
    let out = '';
    if (has('ctrl')) out += '⌃';
    if (has('alt')) out += '⌥';
    if (has('shift')) out += '⇧';
    if (has('meta')) out += '⌘';
    return out + keyLabel(code);
  }
  const parts: string[] = [];
  if (has('ctrl')) parts.push('Ctrl');
  if (has('alt')) parts.push('Alt');
  if (has('shift')) parts.push('Shift');
  if (has('meta')) parts.push('Win');
  parts.push(keyLabel(code));
  return parts.join('+');
}

// ---------------------------------------------------------------------------
// What the editor may not take
// ---------------------------------------------------------------------------

/**
 * Accelerators the macOS menu bar owns, from `macos_menu.rs` plus AppKit's own
 * predefined items. The system swallows every one of these before the webview
 * sees a keydown, so binding one here would produce a shortcut that silently
 * never fires — the single trap most likely to make this feature look broken.
 * Keep in sync with `macos_menu::build`.
 */
const MAC_MENU_ACCELERATORS = new Set([
  'meta+Comma',
  'meta+KeyH',
  'alt+meta+KeyH',
  'meta+KeyQ',
  'meta+KeyO',
  'meta+KeyL',
  'meta+KeyI',
  'meta+KeyW',
  'meta+KeyM',
  'meta+KeyZ',
  'shift+meta+KeyZ',
  'meta+KeyX',
  'meta+KeyV',
  'meta+KeyA',
  'ctrl+meta+KeyF',
  'shift+meta+KeyM',
  'meta+Digit1',
  'meta+Digit2',
  'meta+Digit3',
]);

export function isMenuAccelerator(chord: string): boolean {
  return IS_MAC && MAC_MENU_ACCELERATORS.has(chord);
}

/**
 * Chords the player answers on its own terms and will not hand over.
 *
 *   - `digits` — a bare 0–9 jumps to that tenth of the file. Ten keys for one
 *     parameterised action; exposing them as ten rows would say nothing, and
 *     letting one be taken would silently punch a hole in the family.
 *   - `contextual` — Escape unwinds whichever dialog is open and Enter takes
 *     whatever the player is currently offering. Neither is a player action, so
 *     neither has a row to rebind.
 *   - `menu` — see `MAC_MENU_ACCELERATORS`.
 */
export type ReservedReason = 'digits' | 'contextual' | 'menu';

export function reservedReason(chord: string): ReservedReason | null {
  const code = codeOf(chord);
  if (chord === code && /^Digit[0-9]$/.test(code)) return 'digits';
  if (chord === code && (code === 'Escape' || code === 'Enter' || code === 'NumpadEnter'))
    return 'contextual';
  if (code === 'Tab') return 'contextual';
  if (isMenuAccelerator(chord)) return 'menu';
  return null;
}

/** True for the bare digits the player keeps for percentage jumps. */
export function isDigitJump(e: KeyboardEvent): boolean {
  return (
    !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey && /^Digit[0-9]$/.test(e.code)
  );
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

const STORAGE_KEY = 'frameplayer.keys';

/**
 * Only what the user changed, never the whole table. A full snapshot would
 * freeze this build's defaults into every profile, so a default corrected in a
 * later version would reach nobody who had ever opened the editor. Same shape
 * as `frameplayer.loop` and `frameplayer.normalize`: state that is ours, written
 * and never read back from anywhere else.
 */
type Overrides = Partial<Record<ActionId, string[]>>;

function load(): Overrides {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== 'object') return {};
    const out: Overrides = {};
    for (const [id, chords] of Object.entries(parsed as Record<string, unknown>)) {
      if (!BY_ID.has(id as ActionId)) continue;
      if (!Array.isArray(chords)) continue;
      // Deduplicated, not merely filtered: the editor keys its chord badges by
      // the chord string, and Svelte throws on a duplicate key — so one
      // hand-edited `["KeyF","KeyF"]` would take the whole panel down rather
      // than render a repeated badge.
      const clean = new Set(
        chords.filter((c): c is string => typeof c === 'string' && c.length > 0),
      );
      out[id as ActionId] = [...clean];
    }
    return out;
  } catch {
    return {};
  }
}

class KeyBindings {
  overrides = $state<Overrides>(load());

  /**
   * chord → action, built once per change rather than per keypress. Table order
   * wins a duplicate, which `assign()` makes unreachable but a hand-edited
   * store does not.
   */
  map = $derived.by(() => {
    const out = new Map<string, ActionId>();
    for (const def of ACTIONS) {
      for (const chord of this.overrides[def.id] ?? defaultChords(def)) {
        if (!out.has(chord)) out.set(chord, def.id);
      }
    }
    return out;
  });
}

const state = new KeyBindings();

function persist() {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state.overrides));
  } catch {
    // not critical: the bindings simply will not survive a restart
  }
}

/** Chords currently bound to an action. */
export function chordsOf(id: ActionId): string[] {
  const def = BY_ID.get(id);
  if (!def) return [];
  return state.overrides[id] ?? defaultChords(def);
}

/** True when this action no longer carries its shipped default. */
export function isCustom(id: ActionId): boolean {
  return id in state.overrides;
}

/** Which action a chord runs, if any. */
export function actionFor(chord: string): ActionDef | null {
  const id = state.map.get(chord);
  return id ? (BY_ID.get(id) ?? null) : null;
}

/** Which action holds this chord — for the editor's conflict line. */
export function holderOf(chord: string): ActionId | null {
  return state.map.get(chord) ?? null;
}

/**
 * Typed rather than cast on purpose: `keys.${ActionId}` is a union of 36
 * literals, so this only compiles while the dictionary names every action. A
 * cast here would have turned a missing translation into an `undefined` in the
 * middle of the panel, found by looking at it.
 */
export function actionLabel(id: ActionId): string {
  const key: MessageKey = `keys.${id}`;
  return t(key);
}

/**
 * Give a chord to an action, taking it off whoever had it. Returns the action
 * it was taken from so the editor can say so: a silently stolen binding is how
 * a rebind ends up looking like two features breaking at once.
 */
export function assign(id: ActionId, chord: string): ActionId | null {
  const previous = state.map.get(chord) ?? null;
  const next: Overrides = { ...state.overrides };
  // Strip the chord from every other action, not merely from the one `map`
  // named. Two actions can only end up holding one chord through a hand-edited
  // store, and then `map` reports the table-order winner — taking it off only
  // that one leaves the loser's row still showing a chord it does not answer,
  // which is precisely what an editor exists to make impossible.
  for (const def of ACTIONS) {
    if (def.id === id) continue;
    const held = chordsOf(def.id);
    if (held.includes(chord)) next[def.id] = held.filter((c) => c !== chord);
  }
  const own = chordsOf(id);
  next[id] = own.includes(chord) ? own : [...own, chord];
  state.overrides = next;
  persist();
  return previous === id ? null : previous;
}

export function unassign(id: ActionId, chord: string) {
  state.overrides = { ...state.overrides, [id]: chordsOf(id).filter((c) => c !== chord) };
  persist();
}

/** Back to this build's default for one action. */
export function resetAction(id: ActionId) {
  const next: Overrides = { ...state.overrides };
  delete next[id];
  state.overrides = next;
  persist();
}

export function resetAll() {
  state.overrides = {};
  persist();
}

/** Whether anything at all has been changed — gates the reset button. */
export function hasCustomBindings(): boolean {
  return Object.keys(state.overrides).length > 0;
}

/**
 * The hotkey to advertise for an action: its first chord, or nothing at all.
 *
 * Callers put it in a tooltip or a menu hint, so an unbound action has to yield
 * an empty string rather than a placeholder — a menu row reading "Полный экран
 * —" is worse than one reading nothing.
 */
export function hint(id: ActionId): string {
  const [first] = chordsOf(id);
  return first ? chordLabel(first) : '';
}

/**
 * Two opposite actions as one badge — `⌃⌘←/→` rather than `⌃⌘← / ⌃⌘→`, which is
 * how the chapter and other paired hints have always read. Collapses only while
 * the two chords share their modifiers; rebind one of them apart from the other
 * and it falls back to spelling both out, because a shared prefix that is no
 * longer shared would print a key combination that does not exist.
 */
export function hintPair(a: ActionId, b: ActionId): string {
  const [first] = chordsOf(a);
  const [second] = chordsOf(b);
  if (!first || !second) return hint(a) || hint(b);
  const modsA = first.slice(0, first.lastIndexOf('+') + 1);
  const modsB = second.slice(0, second.lastIndexOf('+') + 1);
  if (modsA !== modsB) return `${chordLabel(first)} / ${chordLabel(second)}`;
  return `${chordLabel(first)}/${keyLabel(codeOf(second))}`;
}

/**
 * A label with its current hotkey after it, for tooltips.
 *
 * An unbound action yields the bare label: "Полный экран ()" is worse than a
 * tooltip that simply says what the button does, and every action here can be
 * unbound now.
 */
export function withKey(label: string, id: ActionId): string {
  const key = hint(id);
  return key ? t('osc.with_key', { label, key }) : label;
}
