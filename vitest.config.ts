/**
 * Unit tests, separate from the app's own Vite config on purpose.
 *
 * `sveltekit()` brings routing, the service worker and the `$app/*` virtual
 * modules with it, none of which a unit test wants; the plain `svelte()` plugin
 * is what compiles a `.svelte.ts` module so its runes work under node. That is
 * the whole reason this config exists rather than a `test` block in
 * `vite.config.ts`.
 *
 * What is worth testing here is the pure end of the codebase — the arithmetic
 * and the parsing, where a wrong answer is silent and permanent: a source id
 * that changes shape forgets every position ever recorded against it, a
 * mis-scored track picks the wrong dub for a season, and a chapter title read
 * as "the credits" hides a skip button over the film. None of that is visible
 * in a screenshot, which is exactly why it is here and not in the manual pass.
 */

import { svelte } from '@sveltejs/vite-plugin-svelte';
import { resolve } from 'node:path';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  plugins: [svelte({ compilerOptions: { hmr: false } })],
  resolve: {
    alias: { $lib: resolve(import.meta.dirname, 'src/lib') },
    // The browser build of svelte: `svelte/internal/client`, which is what a
    // compiled `.svelte.ts` imports. Without it node resolves the server build
    // and `$state` becomes a no-op that silently reports stale values.
    conditions: ['browser'],
  },
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
    setupFiles: ['src/lib/test-setup.ts'],
  },
});
