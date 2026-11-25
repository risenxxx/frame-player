// SvelteKit's shell, by hand: `+layout.ts`'s side effects and `load()`, then `+layout` around
// `+page` (Root.svelte). The interface itself is v1's `src/`, unchanged.
// No top-level await: Skathi's production bundle refuses it (SK0004).
import './sk-port/tauri-stub';
import { mount } from '@skathi-native/runtime/svelte';
import { load } from '../../../src/routes/+layout';
import Root from './sk-port/Root.svelte';
import { loadDeclaredFonts } from './sk-port/fonts';
import { installFocusWithin } from './sk-port/focus-within';

installFocusWithin();
void load().then(() => {
  mount(Root);
  loadDeclaredFonts();
});
