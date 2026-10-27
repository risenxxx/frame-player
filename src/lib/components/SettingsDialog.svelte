<script lang="ts">
  /// The settings dialog: everything the player lets you change, plus the
  /// hotkey editor.
  ///
  /// This file is the shell — the sheet, the tab row and what has to live as
  /// long as the sheet does; each section is its own component under
  /// `settings/`, mounted only while it is showing. All of the state lives in
  /// the dialog — the mirrored mpv.conf values (`MpvSettings`, one per
  /// opening), the HDR readout, the audio device list, the language picker and
  /// the key recorder — because none of it means anything while the dialog is
  /// shut. Mounting is opening: the caller renders this only while the sheet is
  /// up, so reading the current values, polling the display and tearing the key
  /// recorder down are all just this component's lifecycle.
  ///
  /// One deliberate behavior change came with that. The display's HDR state
  /// used to be polled every three seconds for the whole life of the app; it is
  /// only ever shown here, so the timer now runs only while the dialog is open.
  import { invoke } from '@tauri-apps/api/core';
  import { getVersion } from '@tauri-apps/api/app';
  import { getProperty } from 'tauri-plugin-libmpv-api';

  import Dialog from '$lib/components/Dialog.svelte';
  import GeneralTab from '$lib/components/settings/GeneralTab.svelte';
  import VideoTab from '$lib/components/settings/VideoTab.svelte';
  import PlaybackTab from '$lib/components/settings/PlaybackTab.svelte';
  import AudioTab from '$lib/components/settings/AudioTab.svelte';
  import SubsTab from '$lib/components/settings/SubsTab.svelte';
  import TorrentsTab from '$lib/components/settings/TorrentsTab.svelte';
  import TvTab from '$lib/components/settings/TvTab.svelte';
  import KeysTab from '$lib/components/settings/KeysTab.svelte';
  import ConfFoot from '$lib/components/settings/ConfFoot.svelte';
  import { MpvSettings } from '$lib/settings/mpv-settings.svelte';
  import { t } from '$lib/i18n.svelte';
  import type { ActionId } from '$lib/keys.svelte';
  import { player } from '$lib/player.svelte';
  import type { AdjustParam } from '$lib/picture-adjust.svelte';
  import { chrome } from '$lib/chrome.svelte';
  import { torrentPrefs, type Encryption, type Route } from '$lib/torrent.svelte';
  import { showOsd } from '$lib/osd.svelte';

  interface Props {
    onclose: () => void;
    /// Page-owned because they act on the player and the torrent session, not
    /// on a setting: one stops the session and rebuilds it, the other deletes
    /// data off the disk.
    onToggleSeeding: () => void;
    onTogglePortForward: () => void;
    onSetProxy: (url: string) => void;
    onSetEncryption: (mode: Encryption) => void;
    onSetRoute: (route: Route) => void;
    onClearTorrentCache: () => void;
    /// Raises the third-party notices, which are a layer above this sheet. A
    /// callback rather than reaching for `overlays`: no component in this
    /// project opens a dialog by itself — the page owns the surface stack.
    onLicenses: () => void;
    /// Raises the issue report, a layer above this sheet like the notices.
    onReport: () => void;
    /// The section to open on, when the viewer was sent here for one setting.
    initialTab?: string | null;
  }

  let {
    onclose,
    onToggleSeeding,
    onTogglePortForward,
    onSetProxy,
    onSetEncryption,
    onSetRoute,
    onClearTorrentCache,
    onLicenses,
    onReport,
    initialTab = null,
  }: Props =
    $props();

  /// The mpv-backed settings of this opening — see the module for why it is
  /// one per dialog rather than a store.
  const mpv = new MpvSettings();

  /// The decoder mpv actually picked (`hwdec-current`), shown in the settings:
  /// exact-seek speed depends on it — on 4K60 the difference between hardware
  /// and software decoding is plainly visible, and a silent fallback to
  /// software was previously impossible to notice.
  let hwdecCurrent = $state<string | null>(null);
  /// The WebView2 / WebKit build drawing this sheet. Evergreen on Windows, so
  /// it changes under us between releases — see `webview_version` in lib.rs.
  let webviewVersion = $state<string | null>(null);
  let appVersion = $state<string | null>(null);

  /// Settings dialog tab, split by who owns the setting: "Video" is what ends
  /// up in mpv.conf, "General" is what the player keeps itself. "General" by
  /// default: video options are fine-tuning, reached deliberately, whereas the
  /// first tab is more often opened by accident.
  /// Which section of the settings dialog is showing. Topical rather than the
  /// old two-way split by who owns the value — the ownership boundary is still
  /// real (only some of these end up in mpv.conf) but it is a poor map for
  /// someone looking for "subtitle size", which is what a settings dialog is
  /// searched by. What the boundary still owns is the footer: it now names mpv's
  /// settings rather than claiming everything on the page is one of them.
  type SettingsTab = 'general' | 'video' | 'playback' | 'audio' | 'subs' | 'torrents' | 'tv' | 'keys';
  const TABS: readonly SettingsTab[] = ['general', 'video', 'playback', 'audio', 'subs', 'torrents', 'tv', 'keys'];
  // Read once, on the way in: the tab is the viewer's from then on.
  let settingsTab = $state<SettingsTab>(
    TABS.find((tab) => tab === initialTab) ?? 'general',
  );

  /// The picture slider being dragged, which turns the sheet see-through (see
  /// `peek` on Dialog). The control bar is kept away from the first touch until
  /// the sheet closes, as in the context menu.
  let peeking = $state<AdjustParam | null>(null);

  $effect(() => () => {
    chrome.tuning = false;
  });
  // The tab row is nowrap with hidden overflow, and the sheet's width is sized
  // from it (598px fits seven Russian labels with ~50px slack) — which is why
  // this tab is «ТВ», not «Трансляция»: two characters ride in the slack, a
  // ten-character label overflows and the eighth tab is silently gone.
  const SETTINGS_TABS: { id: SettingsTab; label: string }[] = $derived([
    { id: 'general', label: t('set.tab_general') },
    { id: 'video', label: t('set.tab_video') },
    { id: 'playback', label: t('set.tab_playback') },
    { id: 'audio', label: t('set.tab_audio') },
    { id: 'subs', label: t('set.tab_subs') },
    { id: 'torrents', label: t('set.tab_torrents') },
    { id: 'tv', label: t('set.tab_tv') },
    { id: 'keys', label: t('set.tab_keys') },
  ]);
  /// The row scrolls sideways when the labels do not fit — which is invisible
  /// by design (`scrollbar-width: none`), so a tab past the edge is simply
  /// gone. That is fine for a row you can reach with a drag and wrong for the
  /// one tab that has to be legible at all times, hence: whichever section is
  /// showing is brought into the row. `nearest` on both axes, so a tab already
  /// in view moves nothing at all and the sheet's own vertical scroll is never
  /// touched.
  const tabEls: Partial<Record<SettingsTab, HTMLButtonElement>> = {};
  $effect(() => {
    tabEls[settingsTab]?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  });

  // ---- Held for the tabs ----
  // A tab is unmounted when another one is shown, and these were the dialog's
  // own state before the tabs were split out — so they stay the dialog's, and
  // a visit to another tab and back finds them as it left them. What each one
  // is, is written on the tab's prop.
  let proxyVal = $state(torrentPrefs.proxy);
  let torrentDir = $state<string | null>(null);
  let torrentDirDefault = $state(true);
  let castCacheBytes = $state<number | null>(null);
  let keyNote = $state<{ id: ActionId; text: string } | null>(null);

  // Popup elements: measured after render (before paint) to clamp to the window

  async function loadSettings() {
    void mpv.loadAudioDevices();
    try {
      const conf = await invoke<{ path: string; options: [string, string][] }>('user_mpv_conf');
      player.mpvConfPath = conf.path;
      const map: Record<string, string | null> = {};
      for (const s of mpv.SETTINGS) map[s.key] = null;
      for (const [k, v] of conf.options) if (k in map) map[k] = v;
      mpv.settingsValues = map;
      void mpv.refreshHdrStatus();
      void invoke<string | null>('webview_version')
        .then((v) => (webviewVersion = v))
        .catch(() => {});
      void getVersion()
        .then((v) => (appVersion = v))
        .catch(() => {});
      hwdecCurrent = player.hasFile
        ? ((await getProperty('hwdec-current', 'string').catch(() => null)) ?? null)
        : null;
    } catch (e) {
      // The sheet is already on screen — mounting is what opened it — so a
      // failure here has to take it away again. Before the split this simply
      // declined to set `settingsOpen`, which had the same effect.
      showOsd(t('osd.settings_failed'));
      console.warn('reading the settings failed:', e);
      onclose();
    }
  }

  /// Read the current values once, and keep the display's HDR state fresh while
  /// the sheet is up: it reacts to the Windows HDR toggle and to the window
  /// moving to another monitor, and on macOS the EDR headroom genuinely
  /// fluctuates. The call is microseconds.
  $effect(() => {
    void loadSettings();
    const timer = setInterval(() => void mpv.refreshHdrStatus(), 3000);
    return () => clearInterval(timer);
  });

  /// The key recorder's rule (see KeysTab) for the language search, and for the
  /// same reason: a panel left open belongs to the tab that opened it. Reopening the dialog onto a
  /// half-typed query would also be a state nobody asked for.
  $effect(() => {
    void settingsTab;
    return () => {
      mpv.langPickerFor = null;
      mpv.langQuery = '';
    };
  });
</script>

<!-- The tab row is passed to the shell rather than written as the first thing
     in the body, which is the whole of what makes it stay on screen: the shell
     pins its head and this together, so the row that says which of the eight
     sections you are reading cannot scroll away from the section itself. -->
{#snippet tabs(toTop: () => void)}
  <div class="tabs" role="tablist">
    {#each SETTINGS_TABS as tab (tab.id)}
      <button
        class="tab"
        class:sel={settingsTab === tab.id}
        role="tab"
        aria-selected={settingsTab === tab.id}
        bind:this={tabEls[tab.id]}
        onclick={() => {
          settingsTab = tab.id;
          toTop();
        }}
      >
        {tab.label}
      </button>
    {/each}
  </div>
{/snippet}

<Dialog title={t('set.title')} scrollable header={tabs} peek={!!peeking} {onclose}>
  {#if settingsTab === 'general'}
    <GeneralTab {onLicenses} {onReport} {appVersion} {webviewVersion} />
  {:else if settingsTab === 'torrents'}
    <TorrentsTab
      {onToggleSeeding}
      {onTogglePortForward}
      {onSetProxy}
      {onSetEncryption}
      {onSetRoute}
      {onClearTorrentCache}
      bind:proxyVal={proxyVal}
      bind:torrentDir={torrentDir}
      bind:torrentDirDefault={torrentDirDefault}
    />
  {:else if settingsTab === 'tv'}
    <TvTab bind:castCacheBytes={castCacheBytes} />
  {:else if settingsTab === 'playback'}
    <PlaybackTab />
  {:else if settingsTab === 'keys'}
    <KeysTab bind:keyNote={keyNote} />
  {:else}
    {#if settingsTab === 'video'}
      <VideoTab {mpv} {hwdecCurrent} bind:peeking={peeking} />
    {:else if settingsTab === 'audio'}
      <AudioTab {mpv} />
    {:else}
      <SubsTab {mpv} />
    {/if}
    <ConfFoot />
  {/if}
</Dialog>
