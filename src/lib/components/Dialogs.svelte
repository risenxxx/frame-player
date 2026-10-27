<script lang="ts">
  /// Everything that opens over the player: the context menu, then the
  /// dialogs, in the order they stack — later in this file paints on top,
  /// which several of the notes below depend on (and `closeTopmost` in
  /// overlays.svelte.ts unwinds in the same order). No wrapper element: the
  /// page's DOM is what it was before this left it.
  import CatalogDialog from '$lib/components/CatalogDialog.svelte';
  import ContextMenu from '$lib/components/ContextMenu.svelte';
  import DiagnosisDialog from '$lib/components/DiagnosisDialog.svelte';
  import FeedPickDialog from '$lib/components/FeedPickDialog.svelte';
  import LicensesDialog from '$lib/components/LicensesDialog.svelte';
  import LinkDialog from '$lib/components/LinkDialog.svelte';
  import MediaInfoDialog from '$lib/components/MediaInfoDialog.svelte';
  import ReportDialog from '$lib/components/ReportDialog.svelte';
  import RoomDialog from '$lib/components/RoomDialog.svelte';
  import SettingsDialog from '$lib/components/SettingsDialog.svelte';
  import SubsDialog from '$lib/components/SubsDialog.svelte';
  import TorrentPickDialog from '$lib/components/TorrentPickDialog.svelte';
  import TorrentUpdateDialog from '$lib/components/TorrentUpdateDialog.svelte';
  import VpnRouteDialog from '$lib/components/VpnRouteDialog.svelte';
  import { copyDiagnosis, diag } from '$lib/cast-diagnosis.svelte';
  import { catalog, closeCatalog } from '$lib/catalog.svelte';
  import { chrome, toggleFullscreen } from '$lib/chrome.svelte';
  import {
    applyEncryption,
    applyProxy,
    applyRoute,
    clearTorrentCache,
    dropLink,
    fixYtdlp,
    openFileDialog,
    opening,
    openLinkDialog,
    openTorrent,
    pickFeedItem,
    pickTorrentFile,
    playTorrentFile,
    submitLink,
    submitUpdate,
    togglePortForward,
    toggleSeeding,
  } from '$lib/open.svelte';
  import { closeSettings, openSettingsAt, overlays, toggleInfo } from '$lib/overlays.svelte';
  import { cycleAbLoop, cycleLoop, jumpChapter, player, setPicture } from '$lib/player.svelte';
  import { backToStart } from '$lib/start-screen.svelte';
  import { subs } from '$lib/subs.svelte';
  import { answerVpnAsk, net, torrentPositions, watchedFiles } from '$lib/torrent.svelte';
</script>

{#if overlays.ctxAt}
  <ContextMenu
    at={overlays.ctxAt}
    fullscreen={chrome.fullscreen}
    close={() => (overlays.ctxAt = null)}
    actions={{
      openFile: () => void openFileDialog(),
      openLink: () => void openLinkDialog(),
      backToStart: () => void backToStart(),
      toggleInfo: () => toggleInfo(player.hasFile),
      openSettings: () => (overlays.settings = true),
      openRoom: () => (overlays.room = true),
      toggleFullscreen: () => void toggleFullscreen(),
      cycleLoop,
      cycleAbLoop,
      jumpChapter,
      setPicture,
    }}
  />
{/if}

{#if diag.open}
  <DiagnosisDialog
    device={diag.device}
    lines={diag.lines}
    busy={diag.busy}
    onclose={() => (diag.open = false)}
    onCopy={(device) => void copyDiagnosis(device)}
  />
{/if}

{#if opening.linkOpen}
  <LinkDialog
    link={opening.box}
    ytdlpBusy={opening.ytdlpBusy}
    ytdlpPct={opening.ytdlpPct}
    feedBusy={opening.feedBusy}
    onclose={() => (opening.linkOpen = false)}
    onSubmit={(url) => submitLink(url)}
    onForget={(url) => dropLink(url)}
    onPickTorrentFile={pickTorrentFile}
    onFixYtdlp={fixYtdlp}
  />
{/if}

<!-- The dialog leads with the *reason*, not the request. "Update" over an
     input box is mysterious; "BitTorrent cannot add a file to a torrent" is
     the fact that makes the whole errand make sense, and it is why the
     player cannot do this by itself. -->
{#if opening.updateFor}
  {@const known = opening.updateFor}
  <TorrentUpdateDialog
    {known}
    suggested={opening.updateSuggested}
    feedItem={opening.updateFeedItem}
    note={opening.updateNote}
    busy={opening.updateBusy}
    error={opening.updateError}
    value={opening.updateValue}
    onValue={(v) => (opening.updateValue = v)}
    onclose={() => { if (!opening.updateBusy) opening.updateFor = null; }}
    onSubmit={() => void submitUpdate()}
    onOpenAsNew={(magnet) => { opening.updateFor = null; void openTorrent(magnet); }}
  />
{/if}

<!-- A torrent holding more than one video is a question only the viewer can
     answer. The rest still become queue entries, which is free: nothing is
     downloaded until mpv reads one (see torrent/mod.rs). -->
<!-- A feed holding more than one release. Picking one resolves it into an
     ordinary torrent, whose own file picker may follow. -->
{#if opening.feedPick}
  {@const pick = opening.feedPick}
  <FeedPickDialog
    feed={pick.feed}
    items={pick.items}
    onclose={() => (opening.feedPick = null)}
    onPick={(item) => void pickFeedItem(pick.url, item)}
  />
{/if}

{#if opening.pick}
  {@const info = opening.pick}
  <TorrentPickDialog
    {info}
    positions={torrentPositions(info.info_hash)}
    finished={watchedFiles(info.info_hash)}
    onclose={() => (opening.pick = null)}
    onPick={(i, file) => void playTorrentFile(i, file)}
  />
{/if}

<!-- Above the start screen and below everything a chosen release raises:
     `playRelease` shuts this before handing the magnet over, so the torrent
     picker that follows never has to sit on top of it. -->
{#if catalog.open}
  <CatalogDialog
    onclose={closeCatalog}
    onSettings={() => {
      closeCatalog();
      openSettingsAt('torrents');
    }}
  />
{/if}

{#if subs.open}
  <SubsDialog />
{/if}

{#if overlays.info}
  <MediaInfoDialog onclose={() => (overlays.info = false)} />
{/if}

{#if overlays.room}
  <RoomDialog onclose={() => (overlays.room = false)} />
{/if}

{#if overlays.settings}
  <SettingsDialog
    initialTab={overlays.settingsTab}
    onclose={closeSettings}
    onToggleSeeding={() => void toggleSeeding()}
    onTogglePortForward={() => void togglePortForward()}
    onSetProxy={(url) => void applyProxy(url)}
    onSetEncryption={(mode) => void applyEncryption(mode)}
    onSetRoute={(route) => void applyRoute(route)}
    onClearTorrentCache={() => void clearTorrentCache()}
    onLicenses={() => (overlays.licenses = true)}
    onReport={() => (overlays.report = true)}
  />
{/if}

<!-- The one dialog an open *waits on*: `addTorrent` holds the resolve until
     it is answered, so it sits above everything, including the link box
     that may have raised it. -->
{#if net.ask}
  <VpnRouteDialog
    via={net.ask.via}
    direct={net.ask.direct}
    onAnswer={(choice, remember) => void answerVpnAsk(choice, remember)}
  />
{/if}

<!-- Above the settings sheet rather than instead of it: closing the notices
     has to give the settings back, which is what the order in `closeTopmost`
     encodes. Rendered after, so it also paints on top. -->
{#if overlays.licenses}
  <LicensesDialog onclose={() => (overlays.licenses = false)} />
{/if}

{#if overlays.report}
  <ReportDialog onclose={() => (overlays.report = false)} />
{/if}
