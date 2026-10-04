/**
 * Whether this copy of the player keeps everything beside its own executable,
 * and whether the system points at it.
 *
 * The mode itself is decided natively and is not a preference: a `.portable`
 * file next to the executable, read once, before any path is resolved (see
 * portable.rs). All this module does is report it to the interface and own the
 * one choice that goes with it.
 *
 * That choice is the shell registration. An ordinary installation gets its
 * `frameplayer://` protocol and its video associations from the installer; a
 * portable copy has no installer, and Windows has no folder-local way to
 * register either — so it registers itself on launch unless asked not to. The
 * preference lives in localStorage, which for a portable copy is inside its own
 * folder, so the answer travels with the copy rather than with the machine.
 */

import { invoke } from '@tauri-apps/api/core';

import { IS_MAC } from './platform';

const HANDLER_KEY = 'frameplayer.handler';

interface NativeState {
  portable: boolean;
  installed: boolean;
  location: string;
}

class Portable {
  /// Whether this copy keeps its state beside its executable.
  active = $state(false);
  /// Where that state is — shown in the settings sheet either way, so that
  /// which of the two modes a given folder is in is never a mystery.
  location = $state('');
  /// Whether an installer put this copy here — a different question from
  /// where it keeps its state, and the one that decides who owns the shell
  /// registration. The installer's own checkbox can produce an installation
  /// that is portable, and such a copy must not re-register what its installer
  /// already wrote.
  installed = $state(true);
  /// Whether the shell points at *this* copy right now. Read back from the
  /// registry rather than from the preference: with an ordinary installation
  /// beside a portable copy, whichever ran last owns the association, so the
  /// switch has to show what is true and not what was last asked for.
  handler = $state(false);
}

export const portable = new Portable();

export async function initPortable() {
  if (IS_MAC) return;
  const state = await invoke<NativeState>('portable_state').catch((e) => {
    console.warn('portable state failed:', e);
    return null;
  });
  if (!state) return;
  portable.active = state.portable;
  portable.installed = state.installed;
  portable.location = state.location;
  // Only a copy nobody installed has to register itself. An installation —
  // portable or not — got its protocol and its associations from the installer,
  // and writing them again would be the player quietly taking over something
  // that is already correct.
  if (!state.portable || state.installed) return;

  // Off only if it was turned off: a first run has nothing stored and a
  // portable copy that registers nothing cannot open an invitation link.
  const wanted = localStorage.getItem(HANDLER_KEY) !== 'off';
  await readHandler();
  // Both ways, so the preference is what is true rather than what was once
  // asked for: a copy that holds the registration while the switch says off
  // gives it back, which is what makes turning it off reliable even if the
  // moment it was turned off went wrong.
  if (wanted !== portable.handler) {
    await invoke('shell_handler_set', { on: wanted }).catch((e) => {
      console.warn('could not set the shell handler:', e);
    });
    await readHandler();
  }
}

export async function setHandler(on: boolean) {
  localStorage.setItem(HANDLER_KEY, on ? 'on' : 'off');
  await invoke('shell_handler_set', { on }).catch((e) => {
    console.warn('could not change the shell handler:', e);
  });
  await readHandler();
}

async function readHandler() {
  portable.handler = await invoke<boolean>('shell_handler_state').catch(() => false);
}
