// The Skathi build of the interface. The Skathi CLI builds through Vite when the app root has this
// file, which is what lets the app root sit outside `src/`: the interface stays v1's own `src/`,
// reached through the aliases below, and nothing in `src/` knows about Skathi.
//
// `dedupe`: files in the repository's `src/` would otherwise resolve these packages from the
// repository root's node_modules (v1's own, with an older Svelte), and the bundle would carry two
// copies of Svelte — the runtime's and the components' — which fails at mount.
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';

const repo = (path: string) => fileURLToPath(new URL(`../../${path}`, import.meta.url));
const here = (path: string) => fileURLToPath(new URL(path, import.meta.url));

export default defineConfig({
  resolve: {
    alias: [
      { find: /^\$lib$/, replacement: repo('src/lib') },
      { find: /^\$lib\/(.*)$/, replacement: `${repo('src/lib')}/$1` },
      { find: '$app/paths', replacement: here('src/sk-port/app-paths.ts') },
    ],
    dedupe: [
      'svelte',
      '@tauri-apps/api',
      '@tauri-apps/plugin-deep-link',
      '@tauri-apps/plugin-dialog',
      '@tauri-apps/plugin-opener',
      '@tauri-apps/plugin-process',
      '@tauri-apps/plugin-updater',
      'tauri-plugin-libmpv-api',
    ],
  },
});
