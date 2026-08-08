/**
 * The browser globals this codebase reads at *module scope*, stubbed by hand
 * rather than by pulling in jsdom.
 *
 * `i18n.svelte.ts` decides the locale when it is imported (`navigator.language`,
 * `localStorage`), and every module that reaches `t()` therefore drags that
 * along — which is most of them. Without these two the first import throws
 * before a single test runs.
 *
 * Hand-written on purpose. jsdom would supply a hundred APIs the player does not
 * use in node, and the value of a test is partly in what it *cannot* silently
 * depend on: anything beyond these two has to be stubbed deliberately, in the
 * test that needs it, where a reader can see it.
 *
 * `localStorage` is a real implementation rather than a mock, because the
 * history store round-trips JSON through it and a mock returning `undefined`
 * would exercise only the corrupt-store paths.
 */

class MemoryStorage implements Storage {
  #map = new Map<string, string>();

  get length() {
    return this.#map.size;
  }
  key(i: number) {
    return [...this.#map.keys()][i] ?? null;
  }
  getItem(k: string) {
    return this.#map.get(k) ?? null;
  }
  setItem(k: string, v: string) {
    this.#map.set(k, String(v));
  }
  removeItem(k: string) {
    this.#map.delete(k);
  }
  clear() {
    this.#map.clear();
  }
}

Object.defineProperty(globalThis, 'localStorage', {
  value: new MemoryStorage(),
  writable: true,
  configurable: true,
});

// English, so a test asserting on a `t()` string does not depend on the machine
// it runs on. A test that wants Russian calls `setLocale('ru')` itself.
Object.defineProperty(globalThis, 'navigator', {
  value: { language: 'en-US', platform: 'MacIntel', userAgent: 'vitest' },
  writable: true,
  configurable: true,
});

// `i18n` also sets `<html lang>` when it loads, so importing anything that
// reaches `t()` needs somewhere to put it. One property, and deliberately not
// more: a test that wants to touch the DOM is testing the wrong layer — the
// markup lives in components and is checked by the CSS gates and by hand.
Object.defineProperty(globalThis, 'document', {
  value: { documentElement: { lang: 'en' } },
  writable: true,
  configurable: true,
});
