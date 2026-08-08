<script lang="ts">
  /// What a given television answers to the questions a cast already asks and
  /// throws away: our address on its subnet, the Chromecast port and handshake,
  /// the renderer's description and accepted formats, whether it offers Seek.
  ///
  /// A sheet rather than a panel inside the picker: the picker is 280px wide and
  /// scrolls, and eight lines of prose in there is a column of two-word
  /// fragments. It deliberately stops short of LAUNCH — answering a diagnostic
  /// question by starting an app on somebody's TV is a surprise.
  import Dialog from '$lib/components/Dialog.svelte';
  import { t } from '$lib/i18n.svelte';
  import { checkLabel, checkNote, type CheckLine, type TvDevice } from '$lib/cast.svelte';


  interface Props {
    device: TvDevice | null;
    lines: CheckLine[];
    busy: string | null;
    onclose: () => void;
    onCopy: (device: TvDevice) => void;
  }

  let { device, lines, busy, onclose, onCopy }: Props = $props();
</script>

<Dialog title={t('cast.diagnose_title')} variant="diag" {onclose}>
  <div class="diag-device">
    <span class="diag-device-name">{device?.name}</span>
    <span class="diag-device-sub">
      {device?.ip}{device?.model && device.model !== device.name
        ? ` · ${device.model}`
        : ''}
    </span>
  </div>
  {#if busy}
    <div class="diag-waiting">{t('cast.diagnosing')}</div>
  {:else}
    <ul class="diag-list">
      {#each lines as check (check.id)}
        <li class="diag-line {check.state}">
          <span class="diag-name">{checkLabel(check)}</span>
          {#if checkNote(check)}
            <span class="diag-note-line">{checkNote(check)}</span>
          {/if}
          {#if check.detail}
            <span class="diag-detail">{check.detail}</span>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
  <div class="diag-actions">
    <button
      class="diag-copy"
      disabled={busy !== null || !lines.length}
      onclick={() => device && onCopy(device)}
    >
      <svg viewBox="0 0 24 24" width="13" height="13" fill="none" aria-hidden="true">
        <rect
          x="9"
          y="9"
          width="11"
          height="11"
          rx="2.5"
          stroke="currentColor"
          stroke-width="1.7"
        />
        <path
          d="M15 5.5H6.5A2.5 2.5 0 0 0 4 8v8.5"
          stroke="currentColor"
          stroke-width="1.7"
          stroke-linecap="round"
        />
      </svg>
      {t('cast.diagnose_copy')}
    </button>
    <span class="diag-note">{t('cast.diagnose_hint')}</span>
  </div>
</Dialog>

<style>
  .diag-device {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding-bottom: 2px;
  }

  .diag-device-name {
    font-size: 14px;
    font-weight: 600;
    color: #e8e8ec;
  }

  .diag-device-sub {
    font-size: 11px;
    color: rgba(232, 232, 236, 0.45);
  }

  .diag-waiting {
    font-size: 12px;
    color: rgba(232, 232, 236, 0.55);
    padding: 12px 0;
  }

  .diag-actions {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
    padding-top: 12px;
    border-top: 1px solid rgba(255, 255, 255, 0.08);
  }

  /* The same size as the check that opened this window, for the same reason:
     it is not the point of the dialog, the report is. The glyph is what makes
     it findable once it is this small. */
  .diag-copy {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    flex: none;
    background: transparent;
    border: 1px solid rgba(255, 255, 255, 0.18);
    border-radius: 6px;
    padding: 4px 10px;
    color: #d6d6de;
    font-size: 11px;
    cursor: pointer;
  }

  .diag-copy:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.08);
    border-color: rgba(255, 255, 255, 0.3);
    color: #e8e8ec;
  }

  .diag-copy:disabled {
    color: #6a6a74;
    cursor: default;
  }

  .diag-note {
    flex: 1;
    min-width: 0;
    font-size: 11px;
    line-height: 1.4;
    color: rgba(232, 232, 236, 0.45);
  }

  /* The margin used to be a `.diag-dialog .diag-list` override that beat this
     rule's `margin: 0` on specificity. The sheet is the shared Dialog shell
     now, so a descendant selector written here could not reach it (a different
     style scope) — and it never needed to: `.diag-list` appears in that one
     dialog and nowhere else, so the winning value is simply written here. */
  .diag-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 6px 0 10px;
    padding: 0;
    list-style: none;
    width: 100%;
  }

  .diag-line {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 12px;
    line-height: 1.4;
    /* The state is carried by a bar rather than by coloring the text: a report
       is read as prose, and eight colored lines read as an error list. */
    border-left: 2px solid rgba(232, 232, 236, 0.25);
    padding-left: 10px;
  }

  .diag-line.ok {
    border-left-color: #4ade80;
  }
  .diag-line.warn {
    border-left-color: #fbbf24;
  }
  .diag-line.fail,
  .diag-line.timeout {
    border-left-color: #f87171;
  }

  .diag-name {
    color: #e8e8ec;
    /* The check's name is a label, not a sentence — first letter up, and the
       Rust side writes them in lower case so this is the only place it is
       decided. */
    text-transform: capitalize;
  }

  /* Three strengths: the label, the sentence that explains it, and the raw
     answer from the device — which is data and reads as data. */
  .diag-note-line {
    color: rgba(232, 232, 236, 0.65);
  }

  .diag-detail {
    font-size: 11px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    color: rgba(232, 232, 236, 0.4);
    word-break: break-word;
  }
</style>
