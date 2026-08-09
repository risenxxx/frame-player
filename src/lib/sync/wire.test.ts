import { beforeEach, describe, expect, it } from 'vitest';

import { DEFAULT_RELAY, relayUrl, setRelayUrl, socketUrl } from './wire.svelte';

// `socketUrl` turns a setting a person typed into the address a socket opens,
// and it is worth a test for the reason the rest of this suite exists: both ways
// of being wrong are silent. Too strict and the relay is simply unreachable with
// nothing on screen explaining why; too lax and the room's contents — a code, a
// name, the title of what everybody is watching — cross the internet in the
// clear.

describe('the room server setting', () => {
  beforeEach(() => localStorage.removeItem('frameplayer.relay'));

  it('is the shipped default until somebody changes it', () => {
    expect(relayUrl()).toBe(DEFAULT_RELAY);
    expect(socketUrl(relayUrl())).toBe(`wss://${DEFAULT_RELAY}/ws`);
  });

  it('remembers one that was set, tidied', () => {
    setRelayUrl('  https://relay.example/  ');
    expect(relayUrl()).toBe('https://relay.example');
  });

  it('falls back to the default when cleared, rather than turning the feature off', () => {
    // The whole reason `setRelayUrl` removes the key instead of storing a blank:
    // the settings field is pre-filled, so "clear it" is the obvious gesture for
    // "go back to normal" — and storing '' would instead leave the player with
    // no relay at all and a dialog refusing to open a room.
    setRelayUrl('https://relay.example');
    setRelayUrl('');
    expect(relayUrl()).toBe(DEFAULT_RELAY);
    setRelayUrl('   ');
    expect(relayUrl()).toBe(DEFAULT_RELAY);
  });
});

describe('socketUrl', () => {
  it('assumes TLS for a bare host, which is what people type', () => {
    expect(socketUrl('relay.frameplayer.app')).toBe('wss://relay.frameplayer.app/ws');
    expect(socketUrl('  relay.frameplayer.app/  ')).toBe('wss://relay.frameplayer.app/ws');
    expect(socketUrl('relay.example:8443')).toBe('wss://relay.example:8443/ws');
  });

  it('accepts either spelling of a secure address', () => {
    expect(socketUrl('https://relay.example')).toBe('wss://relay.example/ws');
    expect(socketUrl('wss://relay.example')).toBe('wss://relay.example/ws');
  });

  it('keeps a path, so a relay behind a prefix works', () => {
    // A reverse proxy putting it under /sync is an ordinary deployment, and
    // silently dropping the prefix would produce a 404 that reads as the relay
    // being down.
    expect(socketUrl('https://example.invalid/sync')).toBe('wss://example.invalid/sync/ws');
    expect(socketUrl('https://example.invalid/sync/')).toBe('wss://example.invalid/sync/ws');
  });

  it('refuses cleartext to anywhere but this machine', () => {
    // The relay carries what everybody in the room is watching. Over `ws://`
    // that is handed to every network between here and there — and the failure
    // would be invisible, because it works.
    expect(socketUrl('http://relay.example')).toBeNull();
    expect(socketUrl('ws://relay.example')).toBeNull();
    expect(socketUrl('http://192.168.1.10:8080')).toBeNull();
  });

  it('allows cleartext on loopback, which is the development case', () => {
    // It never leaves the machine, and requiring a certificate to run
    // `go run ./server` would mean the local case is the awkward one.
    expect(socketUrl('http://127.0.0.1:8080')).toBe('ws://127.0.0.1:8080/ws');
    expect(socketUrl('http://localhost:8080')).toBe('ws://localhost:8080/ws');
    expect(socketUrl('ws://127.0.0.1:8080')).toBe('ws://127.0.0.1:8080/ws');
  });

  it('refuses anything that is not an address at all', () => {
    expect(socketUrl('')).toBeNull();
    expect(socketUrl('   ')).toBeNull();
    expect(socketUrl('file:///etc/passwd')).toBeNull();
    expect(socketUrl('javascript:alert(1)')).toBeNull();
    expect(socketUrl('http://')).toBeNull();
  });
});
