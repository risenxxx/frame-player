<script lang="ts">
  /// "The torrent is about to go through a VPN — go past it?"
  ///
  /// Asked at the first torrent of a run, and only when the system route is
  /// really a tunnel (see `askAboutVpn`) — a VPN that is installed but not
  /// routing asks nothing. It leads with the fact, then the cost of the
  /// alternative, because the alternative has one that is easy to miss: past the
  /// VPN, the swarm sees this machine's own address.
  ///
  /// **Neither answer is the default button.** Enter does nothing here on
  /// purpose: one of the two choices is about where traffic goes and what it
  /// exposes, and a keystroke meant for something else must not make it.
  import Dialog from '$lib/components/Dialog.svelte';
  import { t } from '$lib/i18n.svelte';

  interface Props {
    /// What the system route goes through now — "utun4", "WireGuard".
    via: string;
    /// The way out past it — "Wi-Fi".
    direct: string;
    onAnswer: (choice: 'bypass' | 'vpn', remember: boolean) => void;
  }

  let { via, direct, onAnswer }: Props = $props();

  let remember = $state(false);
</script>

<Dialog title={t('torrent.vpn_ask_title')} variant="link" onclose={() => onAnswer('vpn', false)}>
  <div class="setting-hint">{t('torrent.vpn_ask_body', { via, direct })}</div>
  <div class="setting-hint vpn-exposed">{t('torrent.vpn_ask_exposed')}</div>
  <label class="vpn-remember">
    <input type="checkbox" bind:checked={remember} />
    <span>{t('torrent.vpn_ask_remember')}</span>
  </label>
  <div class="link-actions">
    <button class="btn-outline" onclick={() => onAnswer('vpn', remember)}>
      {t('torrent.vpn_ask_keep')}
    </button>
    <button class="primary" onclick={() => onAnswer('bypass', remember)}>
      {t('torrent.vpn_ask_bypass')}
    </button>
  </div>
</Dialog>

<style>
  .vpn-exposed {
    color: #f0a0a0;
  }

  .vpn-remember {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 12px;
    font-size: 12.5px;
    color: rgba(255, 255, 255, 0.8);
    cursor: pointer;
    user-select: none;
  }

  .vpn-remember input {
    margin: 0;
    width: 14px;
    height: 14px;
    accent-color: #6366f1;
    cursor: pointer;
  }
</style>
