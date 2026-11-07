/**
 * `frameplayer://join/<code>` — an invitation somebody clicked.
 *
 * The link exists because a room code alone is a thing to be dictated, and
 * because "click this and you are watching with me" is the whole of what a
 * viewer wants to do with it. What it deliberately is *not* is a way to reach
 * anything else: exactly one shape is understood, and everything else is
 * ignored in silence rather than guessed at. A custom scheme is a surface any
 * page on the internet can aim at, so it may only ever do the one harmless
 * thing — open a dialog with a code typed into it.
 *
 * Which is also why joining is **offered and not performed**. A link that
 * silently pulled a viewer out of what they were watching and into a stranger's
 * room would be a far worse bargain than one extra click, and the dialog is
 * where the room's server address lives anyway — a link naming a relay this
 * player has never heard of has to be looked at rather than obeyed.
 *
 * Two arrivals, one handler. On macOS the link is an Apple Event that the
 * deep-link plugin turns into `deep-link://new-url`; on Windows it is the argv
 * of a second process, which single-instance forwards and `deliver_deep_links`
 * re-emits under the same name. The frontend therefore has one subscription
 * rather than two code paths for one link.
 */

import { getCurrent, onOpenUrl, register } from '@tauri-apps/plugin-deep-link';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import { CODE_LENGTH, normalizeCode } from './protocol';
import { invalidExternalMagnet, openExternalMagnet } from '../open.svelte';

/// The code a link asked for, waiting to be offered. Read by the page, which
/// raises the room dialog; cleared when it does.
class Invite {
  code = $state('');
}

export const invite = new Invite();

/**
 * The one shape understood: `frameplayer://join/<code>`.
 *
 * Returns '' for anything else — another scheme, another action, a code that
 * cannot exist. Exported for its test, because this is the parser that decides
 * what an arbitrary string from the outside world may do to the player.
 */
export function codeFromLink(raw: string): string {
  const text = raw.trim();
  if (!/^frameplayer:\/\//i.test(text)) return '';
  // Deliberately not `new URL`: for a custom scheme it puts the first segment
  // in `hostname` on one engine and in `pathname` on another, and a parser that
  // disagrees with itself across platforms is how a link works on macOS and
  // does nothing on Windows.
  const rest = text.slice('frameplayer://'.length);
  const match = /^join\/+([^/?#]+)/i.exec(rest);
  if (!match) return '';
  // `decodeURIComponent` throws on a malformed escape — a lone `%`, a truncated
  // sequence — and this runs inside the deep-link handler, so an exception here
  // would take down the delivery of every link rather than ignoring one. The
  // undecoded text is worth trying anyway: it is what a sender that escaped
  // nothing produced.
  let decoded = match[1];
  try {
    decoded = decodeURIComponent(decoded);
  } catch {
    // keep it as it arrived
  }
  const code = normalizeCode(decoded);
  return code.length === CODE_LENGTH ? code : '';
}

/** A magnet URL is a torrent identifier, not a file path or an arbitrary URL. */
export function magnetFromLink(raw: string): string {
  const text = raw.trim();
  if (!/^magnet:\?/i.test(text)) return '';
  try {
    const args = new URLSearchParams(text.slice(text.indexOf('?') + 1));
    const hashes = args.getAll('xt');
    if (!hashes.some((hash) => /^urn:btih:([a-f0-9]{40}|[a-z2-7]{32})$/i.test(hash) ||
      /^urn:btmh:1220[a-f0-9]{64}$/i.test(hash))) return '';
    return text;
  } catch {
    return '';
  }
}

// macOS delivers the magnet before the webview and torrent engine are ready.
// Keep it until initPlayer finishes, and avoid double-opening on both
// onOpenUrl and getCurrent returning the same cold-start link.
let readyForMagnets = false;
let queuedMagnet = '';
let lastMagnet = '';
let lastDeliveryAt = 0;

function offerMagnet(magnet: string) {
  if (queuedMagnet === magnet) return;
  if (lastMagnet === magnet && Date.now() - lastDeliveryAt < 2000) return;
  lastMagnet = magnet;
  lastDeliveryAt = Date.now();
  if (readyForMagnets) openExternalMagnet(magnet);
  else queuedMagnet = magnet;
}

async function drainNativeMagnets() {
  const urls = await invoke<string[]>('take_pending_magnets').catch(() => []);
  if (urls.length) offer(urls);
}

export function enableExternalMagnetOpening() {
  readyForMagnets = true;
  void drainNativeMagnets();
  if (queuedMagnet) {
    const magnet = queuedMagnet;
    queuedMagnet = '';
    openExternalMagnet(magnet);
  }
}

function offer(urls: readonly string[]) {
  for (const url of urls) {
    const magnet = magnetFromLink(url);
    if (magnet) {
      offerMagnet(magnet);
      continue;
    }
    if (/^magnet:/i.test(url.trim())) {
      invalidExternalMagnet(url);
      continue;
    }
    const code = codeFromLink(url);
    if (code) {
      invite.code = code;
      return;
    }
  }
}

/**
 * Start listening, and pick up a link that opened the player.
 *
 * `getCurrent` is not the same question as the event: a cold start from a link
 * has already delivered it before any of this runs, and without asking, the
 * first launch from an invitation would be the one case that silently did
 * nothing.
 */
export async function initDeepLinks() {
  // Windows registers its scheme in the registry at runtime rather than at
  // install time, so this is what makes a link work in a development build and
  // after a copy-paste install. It is unsupported on macOS — the scheme is
  // declared in Info.plist there — and the refusal is expected, not an error.
  await register('frameplayer').catch(() => {});

  // One subscription for both arrivals: `onOpenUrl` is a listener on
  // `deep-link://new-url`, which is the event the plugin emits for the macOS
  // Apple Event *and* the one `deliver_deep_links` re-emits for a Windows link
  // that reached an already-running instance. Emitting under the plugin's own
  // name rather than a name of ours is what keeps this side to one path.
  await onOpenUrl((urls) => offer(urls)).catch(() => {});
  // Also subscribe to the native inbox. Unlike the deep-link plugin alone,
  // this survives an Apple Event that arrives before the webview exists.
  await listen('frameplayer://open-magnet', () => void drainNativeMagnets()).catch(() => {});
  void drainNativeMagnets();

  const current = await getCurrent().catch(() => null);
  if (current) offer(current);
}
