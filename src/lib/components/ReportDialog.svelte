<script lang="ts">
  /// The issue report, shown before anything can be done with it.
  ///
  /// The text in the box is the text the three buttons act on — the same
  /// string, already redacted — so what the viewer reads is exactly what is
  /// copied, saved or attached. The issue button sends none of it: it opens a
  /// new issue with the version and the OS and asks for the saved file.
  ///
  /// The verbose switch sits at the top because the sequence it serves starts
  /// there: turn it on, close this, reproduce the problem, open this again.
  /// Opening the dialog *is* collecting, so the second visit has the detail.
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { save } from '@tauri-apps/plugin-dialog';
  import { invoke } from '@tauri-apps/api/core';

  import Dialog from '$lib/components/Dialog.svelte';
  import { t } from '$lib/i18n.svelte';
  import { showOsd } from '$lib/osd.svelte';
  import {
    buildReport,
    debugLog,
    issueUrl,
    reportFileName,
    setDebugLog,
    type Report,
  } from '$lib/report.svelte';

  interface Props {
    onclose: () => void;
  }

  let { onclose }: Props = $props();

  let report = $state<Report | null>(null);
  let busy = $state(false);
  let failed = $state<string | null>(null);
  /// Set once a file has been written, so the issue button can say which one
  /// to attach.
  let savedAs = $state<string | null>(null);

  $effect(() => {
    void collect();
  });

  async function collect() {
    busy = true;
    failed = null;
    try {
      report = await buildReport();
    } catch (e) {
      console.error('report:', e);
      failed = String(e);
    } finally {
      busy = false;
    }
  }

  async function toggleVerbose() {
    await setDebugLog(!debugLog.on);
    // The report says whether the verbose log is on, so it is collected again
    // rather than left contradicting the switch above it.
    await collect();
  }

  async function copy() {
    if (!report) return;
    try {
      await navigator.clipboard.writeText(report.text);
      showOsd(t('report.copied'));
    } catch (e) {
      console.warn('clipboard write failed:', e);
      showOsd(t('report.copy_failed'));
    }
  }

  async function saveFile() {
    if (!report) return;
    const path = await save({
      defaultPath: reportFileName(),
      filters: [{ name: t('report.file_kind'), extensions: ['txt'] }],
    }).catch(() => null);
    if (!path) return;
    try {
      await invoke('diag_save', { path, text: report.text });
      savedAs = path.split(/[\\/]/).pop() ?? path;
      showOsd(t('report.saved'), { sub: savedAs });
    } catch (e) {
      console.warn('report save failed:', e);
      showOsd(t('report.save_failed'), { sub: String(e) });
    }
  }

  function openIssue() {
    if (!report) return;
    void openUrl(issueUrl(report)).catch((e) => console.warn('open issue failed:', e));
  }

  const sinceLabel = $derived(
    debugLog.since
      ? new Date(debugLog.since).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
      : '',
  );
</script>

<Dialog title={t('report.title')} scrollable {onclose}>
  <p class="report-intro">{t('report.intro')}</p>

  <div class="setting">
    <div class="row-toggle">
      <div class="row-text">
        <div class="setting-label">{t('report.verbose')}</div>
        <div class="setting-hint">
          {#if debugLog.on}
            {t(debugLog.fromEnv ? 'report.verbose_on_env' : 'report.verbose_on', { time: sinceLabel })}
          {:else}
            {t('report.verbose_hint')}
          {/if}
        </div>
      </div>
      <button
        class="switch"
        class:on={debugLog.on}
        role="switch"
        aria-checked={debugLog.on}
        aria-label={t('report.verbose')}
        disabled={busy}
        onclick={() => void toggleVerbose()}
      >
        <span class="switch-knob"></span>
      </button>
    </div>
  </div>

  {#if report}
    <pre class="report-text" class:stale={busy}>{report.text}</pre>
  {:else if failed}
    <p class="report-intro">{t('report.failed', { error: failed })}</p>
  {:else}
    <p class="report-intro">{t('report.collecting')}</p>
  {/if}

  <!-- Collecting again sits with the note about the text, not with the three
       buttons that send it somewhere: it changes what is shown, and four
       buttons in one row do not fit the sheet in any of the three languages. -->
  <div class="report-under">
    <p class="report-note">{t('report.privacy')}</p>
    <button class="btn-outline" disabled={!report || busy} onclick={() => void collect()}>
      {t('report.refresh')}
    </button>
  </div>

  <div class="report-actions">
    <button class="btn-outline" disabled={!report || busy} onclick={() => void copy()}>
      {t('report.copy')}
    </button>
    <button class="btn-outline" disabled={!report || busy} onclick={() => void saveFile()}>
      {t('report.save')}
    </button>
    <button class="primary" disabled={!report || busy} onclick={openIssue}>
      {t('report.issue')}
    </button>
  </div>
  {#if savedAs}
    <p class="report-note">{t('report.attach', { file: savedAs })}</p>
  {/if}
</Dialog>

<style>
  .report-intro {
    margin: 0 0 12px;
    font-size: 13px;
    line-height: 1.5;
    color: rgba(255, 255, 255, 0.66);
  }

  /* The licenses' text box, for the same reasons: selectable (the page turns
     selection off), wrapped anywhere (a URL or a JSON line has no spaces to
     break at), and a height of its own so the buttons stay in view under
     several megabytes of log — what the window leaves after the ~400px of
     sheet around it, which in a small window is what decides. */
  .report-text {
    margin: 0;
    padding: 12px;
    max-height: clamp(120px, calc(100vh - 400px), 46vh);
    overflow: auto;
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 8px;
    background: rgba(0, 0, 0, 0.24);
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 11px;
    line-height: 1.45;
    color: rgba(255, 255, 255, 0.78);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
    -webkit-user-select: text;
  }

  .report-text.stale {
    opacity: 0.5;
  }

  .report-under {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-top: 10px;
  }

  .report-under .report-note {
    flex: 1;
    min-width: 0;
    margin: 0;
  }

  .report-note {
    margin: 10px 0 0;
    font-size: 12px;
    line-height: 1.45;
    color: rgba(255, 255, 255, 0.5);
  }

  .report-actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 14px;
  }
</style>
