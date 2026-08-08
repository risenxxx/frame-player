<script lang="ts">
  /// The third-party notices, shown rather than pointed at.
  ///
  /// This is the notice LGPL-2.1 section 6 asks for, and the obligation is to
  /// *accompany the distribution* with it — which the bundled file already does.
  /// What this dialog adds is the part a viewer can act on: revealing a 560 KB
  /// markdown file in Finder is a location, not an answer, and on Windows a `.md`
  /// often has no handler to open it with at all.
  ///
  /// Loaded on mount, so nothing is read while the dialog is shut — the file is
  /// large and is opened rarely. Mounting *is* opening: the page renders this
  /// only while `overlays.licenses` is set.
  import { invoke } from '@tauri-apps/api/core';
  import { revealItemInDir } from '@tauri-apps/plugin-opener';
  import { resolveResource } from '@tauri-apps/api/path';

  import Dialog from '$lib/components/Dialog.svelte';
  import { IS_MAC } from '$lib/platform';
  import { t } from '$lib/i18n.svelte';

  interface Props {
    onclose: () => void;
  }

  let { onclose }: Props = $props();

  let notices = $state<string | null>(null);
  let failed = $state(false);

  $effect(() => {
    void load();
  });

  async function load() {
    try {
      notices = await invoke<string>('third_party_notices');
    } catch (e) {
      console.error('notices:', e);
      failed = true;
    }
  }

  /// Kept beside the text rather than instead of it: someone who wants to keep,
  /// print or diff the notices needs the file, not a scroll area.
  async function reveal() {
    try {
      await revealItemInDir(await resolveResource('THIRD-PARTY-NOTICES.md'));
    } catch (e) {
      console.error('notices reveal:', e);
    }
  }
</script>

<Dialog title={t('lic.title')} scrollable {onclose}>
  <p class="lic-intro">{t('lic.intro')}</p>
  {#if notices !== null}
    <pre class="lic-text">{notices}</pre>
  {:else if failed}
    <p class="lic-intro">{t('lic.failed')}</p>
  {:else}
    <p class="lic-intro">{t('lic.loading')}</p>
  {/if}
  <div class="lic-actions">
    <button class="btn-outline" onclick={reveal}>
      {t(IS_MAC ? 'lic.reveal_mac' : 'lic.reveal_win')}
    </button>
  </div>
</Dialog>

<style>
  .lic-intro {
    margin: 0 0 12px;
    font-size: 13px;
    line-height: 1.5;
    color: rgba(255, 255, 255, 0.66);
  }

  /*
   * `white-space: pre-wrap` rather than `pre`: the notices are hard-wrapped at
   * about 80 columns already, but a license text is not guaranteed to be, and a
   * single long line in a fixed-width sheet would give the whole dialog a
   * horizontal scrollbar. `overflow-wrap` catches the URLs, which are the one
   * thing in here with no spaces to break at.
   */
  .lic-text {
    margin: 0;
    padding: 12px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 8px;
    background: rgba(0, 0, 0, 0.24);
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 11px;
    line-height: 1.45;
    color: rgba(255, 255, 255, 0.78);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    /* The page sets `user-select: none`, which every child inherits — and a
       license you cannot select is one you cannot quote or check. WebKit still
       needs the prefixed property, so both are set. */
    user-select: text;
    -webkit-user-select: text;
  }

  .lic-actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 14px;
  }
</style>
