<script lang="ts">
  /// Settings → Torrents: the catalog, the swarm's network, and where the data
  /// is kept.
  import { invoke } from '@tauri-apps/api/core';

  import { t } from '$lib/i18n.svelte';
  import {
    net,
    refreshNet,
    refreshPortStatus,
    sameRoute,
    setVpnAsk,
    torrent,
    torrentPrefs,
    type Encryption,
    type Route,
  } from '$lib/torrent.svelte';
  import { pickTorrentDir, resetTorrentDir } from '$lib/open.svelte';
  import { proxyLooksValid } from '$lib/source';
  import {
    DEFAULT_TMDB,
    INDEXER_EXAMPLE,
    catalog,
    indexerApiKey,
    indexerUrl,
    setCatalogEnabled,
    setIndexerApiKey,
    setIndexerUrl,
    setTmdbUrl,
    tmdbUrl,
  } from '$lib/catalog.svelte';

  interface Props {
    /// Page-owned because they act on the torrent session, not on a setting:
    /// one stops the session and rebuilds it, the other deletes data off the
    /// disk.
    onToggleSeeding: () => void;
    onTogglePortForward: () => void;
    onSetProxy: (url: string) => void;
    onSetEncryption: (mode: Encryption) => void;
    onSetRoute: (route: Route) => void;
    onClearTorrentCache: () => void;
    /// The proxy field's text. The dialog's rather than this tab's: an address
    /// that cannot work is kept in the field and not applied (see `saveProxy`),
    /// and it has to still be there after a visit to another tab.
    proxyVal: string;
    /// Where torrents are written and whether that is the default, as last
    /// read — the dialog's, so a second visit shows the last answer while it
    /// asks again.
    torrentDir: string | null;
    torrentDirDefault: boolean;
  }

  let {
    onToggleSeeding,
    onTogglePortForward,
    onSetProxy,
    onSetEncryption,
    onSetRoute,
    onClearTorrentCache,
    proxyVal = $bindable(),
    torrentDir = $bindable(),
    torrentDirDefault = $bindable(),
  }: Props = $props();

  // ---- Torrents ----
  /// The three encryption modes, in the order they escalate. A table rather
  /// than three hand-written buttons: the labels and the values then cannot
  /// drift apart, which is the whole failure a pill row has.
  const ENCRYPTION_MODES: readonly {
    id: Encryption;
    label: 'torrent.enc_off' | 'torrent.enc_on' | 'torrent.enc_only';
  }[] = [
    { id: 'off', label: 'torrent.enc_off' },
    { id: 'on', label: 'torrent.enc_on' },
    { id: 'only', label: 'torrent.enc_only' },
  ];

  // The same shape as the relay in General: written on `change` rather than on every
  // keystroke, since applying it tears the session down and a half-typed host
  // would do that for a value nobody meant to save.
  const proxyOk = $derived(proxyLooksValid(proxyVal));

  function saveProxy(next: string) {
    const url = next.trim();
    proxyVal = url;
    // Judged from the local rather than from the derived beside it. A `$derived`
    // does re-evaluate the moment it is read again, so `proxyOk` here would be
    // the new value — but that is a rule to lean on knowingly, and this reads
    // the same either way. A shape that cannot work is kept in the field and
    // not applied: the session would refuse it, and losing a working proxy over
    // a typo in progress is worse than waiting for the address to be finished.
    if (proxyLooksValid(url)) onSetProxy(url);
  }

  // ---- The catalog ----
  // Same shape as the relay in General and for the same reason: a half-typed host is
  // a host, and the field has to show what will actually be used rather than
  // what was typed — an empty entry falls back to the default rather than
  // turning the feature off, which is what the switch beside it is for.
  let indexerVal = $state(indexerUrl());
  let indexerKeyVal = $state(indexerApiKey());
  let tmdbVal = $state(tmdbUrl());

  function saveIndexer(next: string) {
    setIndexerUrl(next);
    indexerVal = indexerUrl();
  }

  function saveIndexerKey(next: string) {
    setIndexerApiKey(next);
    indexerKeyVal = indexerApiKey();
  }

  function saveTmdb(next: string) {
    setTmdbUrl(next);
    tmdbVal = tmdbUrl();
  }


  /// Ask the router when the section that shows the answer comes up, and only
  /// then: it is an SSDP search plus a SOAP round trip, and asking on every
  /// settings opening would spend it on six tabs out of seven. Re-reads on each
  /// visit rather than caching, because the answer legitimately changes — a
  /// mapping appears once the first torrent builds the session. This tab is
  /// mounted only while it is showing, so mounting is the visit.
  $effect(() => {
    void refreshPortStatus();
    void readTorrentDir();
    void refreshNet();
  });

  /// Where torrents are written, read from Rust rather than kept here: the
  /// preference lives beside the data (a dot-file in the state directory) so
  /// that a command arriving before this dialog has ever been opened still puts
  /// files in the right place.
  async function readTorrentDir() {
    const answer = await invoke<[string, boolean]>('torrent_dir').catch(() => null);
    if (!answer) return;
    // Assigned one at a time rather than destructured: `check-runes` reads
    // writes syntactically, and a destructuring assignment is not one it sees —
    // which would report this state as never written and make the gate a
    // warning people learn to ignore.
    torrentDir = answer[0];
    torrentDirDefault = answer[1];
  }

  /// What the session is built with: a one-run answer to the VPN question
  /// wins over the setting, and the list has to show what is really in use.
  const routeNow = $derived<Route>(net.runRoute ?? torrentPrefs.route);

  /// A named interface that is not there right now — unplugged, a VPN that is
  /// off. Still listed, still selected: silently dropping it would change a
  /// choice about where traffic goes behind the viewer's back, and a torrent
  /// refused with "that interface is not connected" is the honest outcome.
  const missingIface = $derived.by(() => {
    const r = routeNow;
    if (r.kind !== 'iface' || !net.view) return null;
    return net.view.interfaces.some((i) => i.name === r.name) ? null : r.name;
  });

  /// Past the VPN: the swarm sees this machine's own address. A named tunnel
  /// is not that, and neither is an interface that is not there to use.
  const exposed = $derived.by(() => {
    const r = routeNow;
    if (r.kind === 'direct') return true;
    if (r.kind !== 'iface') return false;
    const iface = net.view?.interfaces.find((i) => i.name === r.name);
    return !!iface && !iface.vpn;
  });

  function ifaceLabel(name: string | null): string {
    if (!name) return '';
    return net.view?.interfaces.find((i) => i.name === name)?.label ?? name;
  }

  /// Where the system would send the swarm right now, read off the system
  /// rather than guessed from names — see `net_route::default_route_iface`.
  const routeLine = $derived.by(() => {
    const v = net.view;
    if (!v?.via) return null;
    return v.via_vpn
      ? t('torrent.route_via_vpn', { name: ifaceLabel(v.via) })
      : t('torrent.route_via_direct', { name: ifaceLabel(v.via) });
  });

  /// The sentence under the switch. `null` while it is off — a row that is off
  /// has nothing to report, and a line saying so would be noise on the setting
  /// most people will never turn on.
  const portLine = $derived.by(() => {
    if (!torrentPrefs.portForward) return null;
    if (torrent.portChecking) return t('torrent.port_checking');
    const s = torrent.portStatus;
    if (!s) return null;
    if (s.state === 'mapped') {
      return t('torrent.port_mapped', { port: s.port, detail: s.detail ?? '' });
    }
    if (s.state === 'unmapped') return t('torrent.port_unmapped', { port: s.port });
    if (s.state === 'no_router') return t('torrent.port_no_router');
    return t('torrent.port_no_session');
  });
</script>

<div class="set-group">{t('sec.catalog')}</div>
<div class="set-panel">
  <!-- First in the tab, because "how do I find a release" comes before
       "what happens to one I have". On by default, but the switch stays:
       this is the only surface in the player that tells a third party what
       somebody is *looking for* rather than acting on a file they already
       hold, which is an argument for a way out rather than for hiding the
       feature from everyone who never opens this sheet. -->
  <div class="setting">
    <div class="row-toggle">
      <div class="row-text">
        <div class="setting-label">{t('catalog.setting')}</div>
        <div class="setting-hint">{t('catalog.setting_hint')}</div>
      </div>
      <button
        class="switch"
        class:on={catalog.enabled}
        role="switch"
        aria-checked={catalog.enabled}
        aria-label={t('catalog.setting')}
        onclick={() => setCatalogEnabled(!catalog.enabled)}
      >
        <span class="switch-knob"></span>
      </button>
    </div>
  </div>

  <!-- Shown only once the catalog is on: an address for a feature nobody
       has enabled is a question about something that is not happening. -->
  {#if catalog.enabled}
    <!-- The metadata service first, because it is the one the player itself
         depends on: without it the panel still works but has no pictures,
         which is the difference a viewer notices immediately. -->
    <div class="setting">
      <div class="setting-label">{t('catalog.tmdb_label')}</div>
      <div class="setting-hint">{t('catalog.tmdb_hint')}</div>
      <input
        class="link-input"
        value={tmdbVal}
        placeholder={DEFAULT_TMDB}
        spellcheck="false"
        autocapitalize="off"
        aria-label={t('catalog.tmdb_label')}
        onchange={(e) => saveTmdb(e.currentTarget.value)}
      />
    </div>

    <div class="setting">
      <div class="setting-label">{t('catalog.indexer_label')}</div>
      <div class="setting-hint">{t('catalog.indexer_hint')}</div>
      <!-- The placeholder is whatever is *actually* being used when the
           viewer has set nothing — the service's suggestion, when there is
           one — and otherwise the shape of the address to paste, which is
           the one thing the hint cannot show as well as an example can. -->
      <input
        class="link-input"
        value={indexerVal}
        placeholder={catalog.suggested || INDEXER_EXAMPLE}
        spellcheck="false"
        autocapitalize="off"
        aria-label={t('catalog.indexer_label')}
        onchange={(e) => saveIndexer(e.currentTarget.value)}
      />
      <!-- Its own field because that is where both servers show it: the
           feed link in one place, the key on the dashboard or in the
           settings. A key left inside the pasted address works as well. -->
      <input
        class="link-input indexer-key"
        value={indexerKeyVal}
        placeholder={t('catalog.indexer_key')}
        spellcheck="false"
        autocapitalize="off"
        autocomplete="off"
        aria-label={t('catalog.indexer_key')}
        onchange={(e) => saveIndexerKey(e.currentTarget.value)}
      />
    </div>
  {/if}
</div>

<div class="set-group">{t('sec.network')}</div>
<div class="set-panel">

  <!-- Seeding was under the privacy controls, on the argument that it is
       the same kind of decision as an excluded folder — what leaves this
       machine. That argument still holds and is not why it moved: a
       viewer looking for anything about torrents had no section to look
       in, and found the switch and the cache button by reading a tab
       named "Основные" to the bottom. The two are here together because
       they are the whole of what this player decides about a torrent —
       what goes out, and what stays on the disk. A switch, because it is
       on/off; the hint carries the legal weight. -->
  <div class="setting">
    <div class="row-toggle">
      <div class="row-text">
        <div class="setting-label">{t('torrent.seed')}</div>
        <div class="setting-hint">{t('torrent.seed_hint')}</div>
      </div>
      <button
        class="switch"
        class:on={torrentPrefs.seeding}
        role="switch"
        aria-checked={torrentPrefs.seeding}
        aria-label={t('torrent.seed')}
        onclick={onToggleSeeding}
      >
        <!-- `.switch-knob`, like every other switch in this dialog. A
             bare `.knob` has no rule anywhere, so this one rendered as a
             track with nothing in it — on/off told apart only by the
             background. -->
        <span class="switch-knob"></span>
      </button>
    </div>
  </div>

  <!-- **A pill and not a switch, because the middle value is the answer.**
       Off / when possible / encrypted only: the middle one prefers MSE and
       redials in the clear for a peer that will not do it, which is why it is
       the default and why nobody should have to find this setting. The strict
       one is for a network where the plaintext handshake is precisely what
       gets cut — measured against the live Sintel swarm, a magnet still
       resolved from the swarm in 840 ms and the file streamed at 2.2 MB/s
       from 24 peers with every unencrypted peer refused, so it is a usable
       setting rather than a way to sit alone. -->
  <div class="setting">
    <div class="setting-label">{t('torrent.enc')}</div>
    <div class="setting-hint">{t('torrent.enc_hint')}</div>
    <div class="segmented">
      {#each ENCRYPTION_MODES as mode (mode.id)}
        <button
          class="segopt"
          class:sel={torrentPrefs.encryption === mode.id}
          onclick={() => onSetEncryption(mode.id)}
        >
          {t(mode.label)}
        </button>
      {/each}
    </div>
  </div>

  <!-- The only large lever left on peer count, and it is measured rather
       than believed: of ~30 addresses one tracker's announce returned, 20–22
       never answered a SYN — peers behind NAT, which can only ever dial us.
       Public trackers, `numwant`, IPv6 and PEX were all measured and give
       nothing here.

       It carries a status line because the switch alone would be a claim:
       librqbit's forwarder reports to nobody, and a router with UPnP
       disabled swallows the request in silence. "On" and "working" are
       different facts, so the row says both. -->
  <div class="setting">
    <div class="row-toggle">
      <div class="row-text">
        <div class="setting-label">{t('torrent.port')}</div>
        <div class="setting-hint">{t('torrent.port_hint')}</div>
        {#if portLine}
          <div class="setting-hint port-state" class:ok={torrent.portStatus?.state === 'mapped'}>
            {portLine}
          </div>
        {/if}
      </div>
      <button
        class="switch"
        class:on={torrentPrefs.portForward}
        role="switch"
        aria-checked={torrentPrefs.portForward}
        aria-label={t('torrent.port')}
        onclick={onTogglePortForward}
      >
        <span class="switch-knob"></span>
      </button>
    </div>
  </div>

  <!-- **Which way out the swarm takes**, and a list rather than a switch
       because "past the VPN" is not always one interface: it is the one the
       machine would use with no tunnel *now*, and somebody with Wi-Fi and a
       cable may want the other. Auto is first and the default — the system's
       own choice, which is what every release before this one did.

       The line above the list says where the traffic goes right now, as the
       system answers it; the red line below says what leaving the VPN costs,
       and is shown only while it is being paid. -->
  <div class="setting">
    <div class="setting-label">{t('torrent.route')}</div>
    <div class="setting-hint">{t('torrent.route_hint')}</div>
    {#if routeLine}
      <div class="setting-hint">{routeLine}</div>
    {/if}
    <div class="segmented vertical">
      <button
        class="segopt"
        class:sel={routeNow.kind === 'auto'}
        onclick={() => onSetRoute({ kind: 'auto' })}
      >
        {t('torrent.route_auto')}
      </button>
      <button
        class="segopt"
        class:sel={routeNow.kind === 'direct'}
        disabled={!!net.view && !net.view.direct && routeNow.kind !== 'direct'}
        onclick={() => onSetRoute({ kind: 'direct' })}
      >
        {net.view?.direct
          ? t('torrent.route_direct_now', { name: ifaceLabel(net.view.direct) })
          : t('torrent.route_direct')}
      </button>
      {#each net.view?.interfaces ?? [] as iface (iface.name)}
        {@const route = { kind: 'iface', name: iface.name } as const}
        <button
          class="segopt"
          class:sel={sameRoute(routeNow, route)}
          onclick={() => onSetRoute(route)}
        >
          {iface.label}{iface.label !== iface.name ? ` (${iface.name})` : ''}{iface.addr
            ? ` · ${iface.addr}`
            : ''}{iface.vpn ? ` · ${t('torrent.route_vpn_tag')}` : ''}
        </button>
      {/each}
      {#if missingIface}
        <button class="segopt sel" disabled>
          {t('torrent.route_missing_row', { name: missingIface })}
        </button>
      {/if}
    </div>
    {#if net.runRoute}
      <div class="setting-hint">{t('torrent.route_this_run')}</div>
    {/if}
    {#if exposed}
      <div class="link-error">{t('torrent.route_exposed')}</div>
    {/if}
  </div>

  <!-- The question asked at the first torrent of a run when the system route
       is a tunnel. A switch here because "remember" in that dialog is the
       only other way to turn it off, and there has to be a way back. -->
  <div class="setting">
    <div class="row-toggle">
      <div class="row-text">
        <div class="setting-label">{t('torrent.vpn_ask_setting')}</div>
        <div class="setting-hint">{t('torrent.vpn_ask_setting_hint')}</div>
      </div>
      <button
        class="switch"
        class:on={torrentPrefs.vpnAsk}
        role="switch"
        aria-checked={torrentPrefs.vpnAsk}
        aria-label={t('torrent.vpn_ask_setting')}
        onclick={() => setVpnAsk(!torrentPrefs.vpnAsk)}
      >
        <span class="switch-knob"></span>
      </button>
    </div>
  </div>

  <!-- **The other control that decides where the traffic goes**, and a
       field rather than a list because the address is somebody's own — a
       VPN provider's SOCKS5 endpoint, a VPS — and there is nothing sensible
       to guess on their behalf. Written on `change` like the relay and the
       indexer above, and for the same reason: a half-typed host is a host,
       and this one costs the running torrent to apply.

       The hint carries what the proxy does *not* cover, because both halves
       are found the hard way otherwise — the DHT and the UDP trackers keep
       their own sockets, and BitTorrent inside a SOCKS5 tunnel is still
       plaintext BitTorrent, so a network filtering on the protocol rather
       than on the destination sees exactly what it saw before. -->
  <div class="setting">
    <div class="setting-label">{t('torrent.proxy')}</div>
    <div class="setting-hint">{t('torrent.proxy_hint')}</div>
    <input
      class="link-input"
      value={proxyVal}
      placeholder="socks5://127.0.0.1:1080"
      spellcheck="false"
      autocapitalize="off"
      autocorrect="off"
      aria-label={t('torrent.proxy')}
      onchange={(e) => saveProxy(e.currentTarget.value)}
    />
    <!-- Only the shape, and only for a value that cannot work at all:
         `http://…` is what people paste out of habit and librqbit takes
         SOCKS5 alone. Whether the proxy is *running* is the session's
         question, and it answers it by refusing to start. -->
    {#if !proxyOk}
      <div class="link-error">{t('torrent.proxy_bad')}</div>
    {/if}
  </div>
</div>

  <!-- **A setting rather than a question asked at the first torrent.** The
       player has to work with nothing configured, and stopping somebody who
       wants to watch an episode to ask about storage is a modal at the worst
       moment — qBittorrent asks because it *is* a download manager. What the
       default costs is discoverability: the cache directory is one the system
       may empty and nobody browses by hand, so the path is printed here in
       full and the folder button on the start screen opens it. -->
<div class="set-group">{t('sec.storage')}</div>
<div class="set-panel">
  <div class="setting">
    <div class="setting-label">{t('torrent.dir')}</div>
    <div class="setting-hint">{t('torrent.dir_hint')}</div>
    <!-- **Shaped as a field with its action inside it**, rather than as a
         row of text with a button underneath. What it shows is a value that
         can be changed, which is what a field looks like — and the excluded
         folders' row, borrowed here first, is built for a *list*: short, and
         inset by an icon it needs to distinguish one entry from the next.
         There is one entry here and a label above it saying what it is. -->
    <div class="torrent-dir">
      <!-- The container is rtl so the ellipsis eats the head of the path
           rather than its tail — the meaningful part of a path is its last
           components — and `bdi` isolates the direction, or the slashes,
           being neutral characters, drift and "/Users/…" draws as "Users/…/". -->
      <span class="torrent-dir-path" title={torrentDir ?? ''}><bdi>{torrentDir ?? ''}</bdi></span>
      <!-- **Both actions belong to the field, so both sit in it.** This was a
           `.btn-outline` in `.link-actions` first — which is a dialog
           *footer*: full size, pushed to the bottom right, reading as the
           main action of the page rather than as an undo for one control.
           Beside the picker it is unmistakably about this path, and the pair
           needs no explanation of what it resets. One word for the same
           reason the delay stepper's reset is one: the noun is already in the
           label above, and a sentence here would squeeze the path.
           Quieter than the picker on purpose — choosing is the action, going
           back to the default is the correction. -->
      {#if !torrentDirDefault}
        <button
          class="torrent-dir-reset"
          onclick={async () => {
            await resetTorrentDir();
            await readTorrentDir();
          }}
        >
          {t('torrent.dir_reset')}
        </button>
      {/if}
      <button
        class="torrent-dir-pick"
        onclick={async () => {
          if (await pickTorrentDir()) await readTorrentDir();
        }}
      >
        {t('torrent.dir_change')}
      </button>
    </div>
    {#if torrentDirDefault}
      <div class="setting-hint">{t('torrent.dir_default_note')}</div>
    {/if}
  </div>

  <!-- Streaming a torrent writes the pieces to disk, so a few films fill
       a directory the viewer never chose to fill. -->
  <div class="setting">
    <div class="setting-label">{t('torrent.cache_clear')}</div>
    <div class="setting-hint">{t('torrent.cache_hint')}</div>
    <button class="btn-danger" onclick={onClearTorrentCache}>{t('torrent.cache_clear')}</button>
  </div>
</div>

<style>
  /* The key sits under the address as the second half of one setting. */
  .indexer-key {
    margin-top: 8px;
  }

  /* A field, and the same one `.link-input` is: same border, fill, radius and
     outer height, so the two read as one control shape across the sheet. The
     height is reproduced by the button plus this padding and border (28 + 8 +
     2) rather than by copying the input's padding, because here the button is
     what sets it — and every box is a border box (the reset in app.css), which
     is what makes the two numbers comparable at all.

     **Measured against the built stylesheet in both engines, and they do not
     agree about the input**: Chromium puts `.link-input` at 38.00 and WebKit at
     37.00, because its default line box for 13px text is a pixel shorter. So
     there is no height that matches on both, 38 matches Chromium exactly and
     WebKit by a pixel, and the two controls are never adjacent anyway — the
     relay field is in «Основные» and this is in «Раздачи». */
  .torrent-dir {
    display: flex;
    align-items: center;
    /* Tighter than the 8px it was: this gap now also falls *between* the two
       chips, and at 8px they read as two separate offers rather than as one
       pair of actions belonging to the path beside them. */
    gap: 6px;
    margin-top: 6px;
    /* Asymmetric on purpose: the right side is the gap around the button and
       obeys the concentric-corner rule below, while the left is `.link-input`'s
       own 12px, so text in the two fields starts at the same offset when they
       are stacked. */
    padding: 4px 4px 4px 12px;
    border: 1px solid rgba(255, 255, 255, 0.14);
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.06);
  }

  .torrent-dir-path {
    flex: 1;
    min-width: 0;
    /* Truncate the start, not the tail: for a path the meaningful part is its
       last components. Direction isolated by <bdi> in the markup. */
    direction: rtl;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #d6d6de;
    font-size: 12px;
  }

  /* Inner radius = outer minus the gap (8 − 4), the concentric rule a rounded
     box inside another rounded box has to obey or the two curves fight. */
  .torrent-dir-pick {
    flex: none;
    height: 28px;
    padding: 0 10px;
    border: none;
    border-radius: 4px;
    background: rgba(255, 255, 255, 0.08);
    color: #d6d6de;
    font-size: 12px;
    cursor: pointer;
    transition: background 0.12s ease, color 0.12s ease;
  }

  .torrent-dir-pick:hover {
    background: rgba(255, 255, 255, 0.14);
    color: #fff;
  }

  /* The picker's twin, one step quieter: same box, no fill until the pointer
     arrives. Two filled chips side by side would be two equal offers, and they
     are not — one chooses, the other undoes. */
  .torrent-dir-reset {
    flex: none;
    height: 28px;
    padding: 0 9px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: #8a8a95;
    font-size: 12px;
    cursor: pointer;
    transition: background 0.12s ease, color 0.12s ease;
  }

  .torrent-dir-reset:hover {
    background: rgba(255, 255, 255, 0.08);
    color: #e8e8ec;
  }

  /* A live readout, not a second sentence of the hint — so it gets air above
     it and reads brighter once the router has confirmed the mapping. Brighter
     rather than colored: the accent means selected/on/primary, and a passive
     readout wearing it inverts that (the OSD bar made exactly this mistake).
     Written as `.row-text .port-state` so it beats the `margin-top: 0` above,
     which is a descendant rule of the same shape and would otherwise win on
     source order. */
  .row-text .port-state {
    margin-top: 8px;
  }

  .port-state.ok {
    color: #b9b9c3;
  }
</style>
