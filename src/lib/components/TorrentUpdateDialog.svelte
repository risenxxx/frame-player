<script lang="ts">
  /// Point a remembered torrent at its replacement magnet.
  ///
  /// The dialog leads with the *reason*, not the request. "Update" over an input
  /// box is mysterious; "BitTorrent cannot add a file to a torrent" is the fact
  /// that makes the whole errand make sense, and it is why the player cannot do
  /// this by itself.
  ///
  /// The field focuses and selects itself here rather than being reached from
  /// outside. It was a `bind:this` back to the page before this dialog became a
  /// component, and the binding did not travel with the markup — leaving the
  /// caller focusing an element nobody ever assigned, which is silent: an
  /// optional chain on `undefined` is not an error, so the dialog simply opened
  /// with nothing focused. Owning it here is also the only version that cannot
  /// come apart again, since the element and the call are in one file.
  import Dialog from '$lib/components/Dialog.svelte';
  import { displayName } from '$lib/format';
  import { t } from '$lib/i18n.svelte';
  import type { RememberedTorrent } from '$lib/torrent.svelte';

  interface Props {
    known: RememberedTorrent;
    /// True when the link box recognized a pasted magnet as this torrent's
    /// successor, rather than the viewer having asked to update it.
    suggested: boolean;
    busy: boolean;
    error: string | null;
    value: string;
    onValue: (v: string) => void;
    onclose: () => void;
    onSubmit: () => void;
    onOpenAsNew: (magnet: string) => void;
  }

  let {
    known,
    suggested,
    busy,
    error,
    value,
    onValue,
    onclose,
    onSubmit,
    onOpenAsNew,
  }: Props = $props();

  let inputEl = $state<HTMLInputElement | undefined>();

  // On mount, which is the only moment this dialog exists for: it is rendered
  // only while open, so there is nothing to guard against re-running. A
  // suggested magnet is selected rather than merely focused, so typing replaces
  // it — that is the case where the viewer disagrees with the guess.
  $effect(() => {
    inputEl?.focus();
    inputEl?.select();
  });
</script>


<Dialog
  title={t('torrent.update')}
  variant="link"
  closeDisabled={busy}
  onclose={() => (onclose())}
>
  <div class="setting-hint">
    {suggested
      ? t('torrent.update_suggested', { name: known.name ?? displayName(known.magnet) })
      : t('torrent.update_why')}
  </div>
  <input
    bind:this={inputEl}
    class="link-input"
    type="text"
    spellcheck="false"
    autocapitalize="off"
    autocorrect="off"
    placeholder="magnet:?xt=urn:btih:…"
    disabled={busy}
    {value}
    oninput={(e) => onValue(e.currentTarget.value)}
    onkeydown={(e) => {
      if (e.key === 'Enter') { e.preventDefault(); onSubmit(); }
      if (e.key === 'Escape') { e.preventDefault(); onclose(); }
      e.stopPropagation();
    }}
  />
  {#if error}
    <div class="link-error">{error}</div>
  {:else}
    <div class="setting-hint">{t('torrent.update_keeps')}</div>
  {/if}
  <div class="link-actions">
    <!-- The way out for a link that only LOOKS like an update: opening
         it separately has to stay one click away, or a wrong guess by
         `findSupersededTorrent` becomes a wall. -->
    {#if suggested}
      <button
        class="btn-outline"
        disabled={busy}
        onclick={() => {
          const magnet = value.trim();
          onclose();
          onOpenAsNew(magnet);
        }}
      >
        {t('torrent.update_as_new')}
      </button>
    {/if}
    <button
      class="primary"
      disabled={!value.trim() || busy}
      onclick={() => onSubmit()}
    >
      {busy ? t('torrent.resolving') : t('torrent.update_go')}
    </button>
  </div>
</Dialog>
