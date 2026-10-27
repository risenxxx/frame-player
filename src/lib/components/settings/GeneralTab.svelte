<script lang="ts">
  /// Settings → General: language, the controls and the pointer, the window,
  /// watching together, the watch history, a portable copy's handler,
  /// reporting a problem, and About.
  import { invoke } from '@tauri-apps/api/core';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { portable, setHandler } from '$lib/portable.svelte';

  import { IS_MAC } from '$lib/platform';
  import { REPO_URL, SITE_URL } from '$lib/links';
  import { locale, setLocale, t, type Locale } from '$lib/i18n.svelte';
  import {
    addExcludedFolder,
    clearHistory,
    history,
    removeExcludedFolder,
    toggleHistory,
  } from '$lib/history.svelte';
  import { debugLog, setDebugLog } from '$lib/report.svelte';
  import {
    AUTO_HIDE_CHOICES,
    HIDE_DELAY_CHOICES,
    CURSOR_HIDE_CHOICES,
    VIDEO_CLICK_CHOICES,
    PINCH_CHOICES,
    setAutoHide,
    setCursorHide,
    setHideDelay,
    setVideoClick,
    setPinch,
    syncMenuChecks,
    toggleWindowPref,
    windowPrefs,
    windowFit,
    setWindowFit,
    WINDOW_FIT_CHOICES,
    type WindowSwitch,
  } from '$lib/window-prefs.svelte';
  import { DEFAULT_RELAY, relayUrl, setRelayUrl } from '$lib/sync/wire.svelte';

  interface Props {
    /// Raises the third-party notices, which are a layer above this sheet. A
    /// callback rather than reaching for `overlays`: no component in this
    /// project opens a dialog by itself — the page owns the surface stack.
    onLicenses: () => void;
    /// Raises the issue report, a layer above this sheet like the notices.
    onReport: () => void;
    /// Read by the dialog when it opens, so About is complete whichever tab
    /// the sheet was opened on.
    appVersion: string | null;
    /// The WebView2 / WebKit build drawing this sheet.
    webviewVersion: string | null;
  }

  let { onLicenses, onReport, appVersion, webviewVersion }: Props = $props();

  /// The About links are links — they go somewhere — but a click on an anchor
  /// inside the web view would navigate the web view itself, so it is handed to
  /// the system browser instead. The href stays for what it says on hover.
  function openLink(e: MouseEvent, url: string) {
    e.preventDefault();
    void openUrl(url).catch(() => {});
  }

  /// The language also lives in the native macOS menu, which is built in Rust
  /// and cannot reach the dictionary — push the choice over and let it rebuild.
  function changeLocale(next: Locale) {
    setLocale(next);
    if (IS_MAC) {
      void invoke('set_menu_locale', { locale: next })
        .then(() => syncMenuChecks())
        .catch(() => {});
    }
  }

  // ---- Watching together ----
  // Written on `change` rather than on every keystroke: a half-typed host is a
  // host, and storing it means the room dialog would try to open it if the
  // viewer left the sheet without blurring the field.
  let relayVal = $state(relayUrl());

  function saveRelay(next: string) {
    setRelayUrl(next);
    // Read back rather than kept: `setRelayUrl` trims and drops a trailing
    // slash, and an empty entry falls back to the default — so the field has to
    // show what will actually be used, not what was typed.
    relayVal = relayUrl();
  }
</script>

{#snippet windowSwitch(key: WindowSwitch, label: string)}
  <div class="setting">
    <div class="row-toggle">
      <div class="row-text">
        <div class="setting-label">{label}</div>
      </div>
      <button
        class="switch"
        class:on={windowPrefs[key]}
        role="switch"
        aria-checked={windowPrefs[key]}
        aria-label={label}
        onclick={() => toggleWindowPref(key)}
      >
        <span class="switch-knob"></span>
      </button>
    </div>
  </div>
{/snippet}

<div class="set-group">{t('sec.interface')}</div>
<div class="set-panel">
  <div class="setting">
    <div class="setting-label">{t('set.language')}</div>
    <div class="segmented">
      <button class="segopt" class:sel={locale() === 'ru'} onclick={() => changeLocale('ru')}>
        Русский
      </button>
      <button class="segopt" class:sel={locale() === 'en'} onclick={() => changeLocale('en')}>
        English
      </button>
      <button class="segopt" class:sel={locale() === 'zh'} onclick={() => changeLocale('zh')}>
        中文
      </button>
    </div>
  </div>
</div>

<div class="set-group">{t('sec.chrome')}</div>
<div class="set-panel">

  <!-- A pill, not a switch: three named places the controls may fade in,
       and the middle one is the answer for anybody who comes from VLC or
       MPC. The cursor sits right under it because it is the same question
       asked of the pointer, answered separately so the bar can stay while
       the pointer goes. -->
  <div class="setting">
    <div class="setting-label">{t('set.autohide')}</div>
    <div class="segmented">
      {#each AUTO_HIDE_CHOICES as v (v)}
        <button class="segopt" class:sel={windowPrefs.autoHide === v} onclick={() => setAutoHide(v)}>
          {t(`set.autohide_${v}`)}
        </button>
      {/each}
    </div>
    <div class="setting-hint">{t('set.autohide_hint')}</div>
  </div>

  <!-- One delay for the bars and the cursor, because it is one timer: the
       pointer resting. Kept on screen under `never` rather than removed — the
       cursor still reads it there. -->
  <div class="setting">
    <div class="setting-label">{t('set.hide_delay')}</div>
    <div class="segmented">
      {#each HIDE_DELAY_CHOICES as v (v)}
        <button class="segopt" class:sel={windowPrefs.hideDelay === v} onclick={() => setHideDelay(v)}>
          {v === 0 ? t('set.hide_delay_now') : t('set.seek_sec', { s: (v / 1000).toLocaleString(locale()) })}
        </button>
      {/each}
    </div>
    <div class="setting-hint">{t('set.hide_delay_hint')}</div>
  </div>

  <div class="setting">
    <div class="setting-label">{t('set.cursor_hide')}</div>
    <div class="segmented">
      {#each CURSOR_HIDE_CHOICES as v (v)}
        <button class="segopt" class:sel={windowPrefs.cursorHide === v} onclick={() => setCursorHide(v)}>
          {t(`set.cursor_hide_${v}`)}
        </button>
      {/each}
    </div>
    <div class="setting-hint">{t('set.cursor_hide_hint')}</div>
  </div>
</div>

<div class="set-group">{t('sec.pointer')}</div>
<div class="set-panel">

  <!-- Three named things a click on the picture can mean, so a pill. The
       default tells a click from a double click without waiting, and the
       two others are for whoever would rather not pay for that: the
       blink of a pause under a double click, or a pause where a double
       click was meant. -->
  <div class="setting">
    <div class="setting-label">{t('set.video_click')}</div>
    <div class="segmented">
      {#each VIDEO_CLICK_CHOICES as v (v)}
        <button class="segopt" class:sel={windowPrefs.videoClick === v} onclick={() => setVideoClick(v)}>
          {t(`set.video_click_${v}`)}
        </button>
      {/each}
    </div>
    <div class="setting-hint">{t('set.video_click_hint')}</div>
  </div>

  <!-- macOS only: nowhere else does anything native see a pinch as a pinch
       rather than as Ctrl+wheel. Two named things the gesture can be, so a
       pill; the zoom keeps ⌥+pinch and Ctrl+wheel either way. -->
  {#if IS_MAC}
    <div class="setting">
      <div class="setting-label">{t('set.pinch')}</div>
      <div class="segmented">
        {#each PINCH_CHOICES as v (v)}
          <button class="segopt" class:sel={windowPrefs.pinch === v} onclick={() => setPinch(v)}>
            {t(`set.pinch_${v}`)}
          </button>
        {/each}
      </div>
      <div class="setting-hint">{t('set.pinch_hint')}</div>
    </div>
  {/if}
</div>

  <!-- The window's own switches, which used to live only in the context
       menu and the macOS menu bar — somebody looking for "remember the
       window's position" looks here first. Same state as both menus, so
       nothing can disagree. -->
<div class="set-group">{t('sec.window')}</div>
<div class="set-panel">
  {@render windowSwitch('remember', t('ctx.win_remember'))}
  <!-- What a new picture does to the window: three named outcomes, so a
       pill rather than the two switches it is stored as (`WindowFit`). -->
  <div class="setting">
    <div class="setting-label">{t('ctx.win_fit')}</div>
    <div class="segmented">
      {#each WINDOW_FIT_CHOICES as v (v)}
        <button class="segopt" class:sel={windowFit() === v} onclick={() => setWindowFit(v)}>
          {t(`ctx.win_fit_${v}`)}
        </button>
      {/each}
    </div>
    <div class="setting-hint">{t('set.win_fit_hint')}</div>
  </div>
  {@render windowSwitch('alwaysOnTop', t('ctx.win_ontop'))}
  {@render windowSwitch('snapMini', t('ctx.win_snap'))}
</div>

<div class="set-group">{t('sec.together')}</div>
<div class="set-panel">

  <!-- Watching together needs a server both ends agree on, and this is where
       it lives rather than in the room dialog: practically nobody runs their
       own, so a field on the way into every room was asking a question with
       one answer. The room dialog points here when the address turns out to
       be wrong, which is the only moment it matters.

       Placed with the language rather than inside the history block below,
       even though "which server learns what I watch" is a fair privacy
       question: history, excluded folders and clearing them are one story
       read top to bottom, and a text field in the middle of it breaks the
       run.

       Empty means the default — `setRelayUrl` removes the key rather than
       storing a blank — so the placeholder is the address itself and
       clearing the field restores it instead of turning the feature off. -->
  <div class="setting">
    <div class="setting-label">{t('sync.relay_label')}</div>
    <div class="setting-hint">{t('sync.relay_hint')}</div>
    <input
      class="link-input"
      value={relayVal}
      placeholder={DEFAULT_RELAY}
      spellcheck="false"
      autocapitalize="off"
      aria-label={t('sync.relay_label')}
      onchange={(e) => saveRelay(e.currentTarget.value)}
    />
  </div>
</div>


<div class="set-group">{t('sec.history')}</div>
<div class="set-panel">
  <div class="setting">
    <div class="row-toggle">
      <div class="row-text">
        <div class="setting-label">{t('set.history')}</div>
        <div class="setting-hint">{t('set.history_hint')}</div>
      </div>
      <button
        class="switch"
        class:on={history.prefs.enabled}
        role="switch"
        aria-checked={history.prefs.enabled}
        aria-label={t('set.history')}
        onclick={toggleHistory}
      >
        <span class="switch-knob"></span>
      </button>
    </div>
  </div>

  <!-- Dimmed together with disabled history: when nothing is written,
       the exclusions affect nothing, and that should be visible. -->
  <div class="setting" class:muted={!history.prefs.enabled}>
    <div class="setting-label">{t('set.excluded')}</div>
    <div class="setting-hint">{t('set.excluded_hint')}</div>
    <div class="folders">
      {#each history.prefs.excluded as dir (dir)}
        <div class="folder-row">
          <svg class="folder-ico" viewBox="0 0 16 16" aria-hidden="true">
            <path
              d="M1.75 3.5h3.9l1.2 1.6h7.4v7.4H1.75z"
              fill="none"
              stroke="currentColor"
              stroke-width="1.3"
              stroke-linejoin="round"
            />
          </svg>
          <!-- bdi isolates the text direction: the container is rtl (so
               the ellipsis lands on the left, since the meaningful part
               of a path is its tail), and without isolation the slashes,
               being neutral characters, move about and "/Users/…" is
               drawn as "Users/…/". -->
          <span class="folder-path" title={dir}><bdi>{dir}</bdi></span>
          <button
            class="folder-remove"
            data-tip={t('set.excluded_remove')}
            aria-label={t('set.excluded_remove')}
            disabled={!history.prefs.enabled}
            onclick={() => removeExcludedFolder(dir)}
          >
            <!-- Same coordinates as the card's cross: round-capped ends
                 must stay inside the viewBox, or they get clipped at
                 the edge. -->
            <svg viewBox="0 0 10 10" aria-hidden="true">
              <path stroke="currentColor" stroke-width="1.4" stroke-linecap="round" d="M1.2 1.2l7.6 7.6M8.8 1.2l-7.6 7.6" />
            </svg>
          </button>
        </div>
      {:else}
        <div class="folders-empty">{t('set.excluded_empty')}</div>
      {/each}
      <button class="folder-add" disabled={!history.prefs.enabled} onclick={addExcludedFolder}>
        <svg viewBox="0 0 16 16" aria-hidden="true">
          <path stroke="currentColor" stroke-width="1.5" stroke-linecap="round" d="M8 3.5v9M3.5 8h9" />
        </svg>
        {t('set.excluded_add')}
      </button>
    </div>
  </div>

  <div class="setting">
    <div class="setting-label">{t('set.clear')}</div>
    <div class="setting-hint">{t('set.clear_hint')}</div>
    <button class="btn-danger" onclick={() => void clearHistory()}>{t('set.clear_btn')}</button>
  </div>
</div>

<!-- Only a portable copy has this, and only because Windows gives it no
     choice: a protocol handler and a file type are machine-side
     registrations, so the one thing a folder cannot carry is being found by
     the system. The switch shows what is true rather than what was asked
     for — another copy may have taken the association since. -->
{#if portable.active}
  <div class="set-group">{t('sec.portable')}</div>
  <div class="set-panel">
    <!-- The switch belongs to a copy that nobody installed. An installation
         that keeps its state beside itself — the installer's checkbox makes
         one — got its protocol and its associations from that installer, and
         the player has no business taking them over or giving them back; it
         says who owns them instead. -->
    {#if portable.installed}
      <div class="settings-foot">{t('set.handler_installer')}</div>
    {:else}
      <div class="setting">
        <div class="row-toggle">
          <div class="row-text">
            <div class="setting-label">{t('set.handler')}</div>
            <div class="setting-hint">{t('set.handler_hint')}</div>
          </div>
          <button
            class="switch"
            class:on={portable.handler}
            role="switch"
            aria-checked={portable.handler}
            aria-label={t('set.handler')}
            onclick={() => void setHandler(!portable.handler)}
          >
            <span class="switch-knob"></span>
          </button>
        </div>
      </div>
    {/if}
    <div class="settings-foot">{t('set.portable_foot', { path: portable.location })}</div>
    <div class="settings-foot">{t('set.portable_keychain')}</div>
  </div>
{/if}

  <!-- Which player this is and where it lives, then what it is made of: the
       web engine it draws with and the LGPL notice. The website and the
       repository are here because nothing else in the player named them —
       a viewer who wanted the project page had to find it through a search.
       The licenses open in a layer above this sheet rather than revealing a
       file: `open-path` is not in the capabilities, and a .md has no
       reliable handler on Windows regardless. Revealing the file is still
       offered, inside that dialog. No rules between the parts: the panel
       already says where the section starts and stops, and a line between
       four short items read as a list of separate settings. -->
<!-- Reporting a problem, beside the version it is about. The switch is
     here as well as in the report itself because the order of use is
     "turn it on, reproduce, collect": somebody following that from an
     issue's instructions meets it here first. -->
<div class="set-group">{t('sec.report')}</div>
<div class="set-panel">
  <div class="setting">
    <div class="row-toggle">
      <div class="row-text">
        <div class="setting-label">{t('report.verbose')}</div>
        <div class="setting-hint">{t('report.verbose_hint')}</div>
      </div>
      <button
        class="switch"
        class:on={debugLog.on}
        role="switch"
        aria-checked={debugLog.on}
        aria-label={t('report.verbose')}
        onclick={() => void setDebugLog(!debugLog.on)}
      >
        <span class="switch-knob"></span>
      </button>
    </div>
  </div>
  <div class="report-row">
    <button class="btn-outline" onclick={onReport}>{t('report.open')}</button>
  </div>
  <div class="settings-foot">{t('report.foot')}</div>
</div>

<div class="set-group">{t('sec.about')}</div>
<div class="set-panel about">
  <div class="about-head">
    <svg class="about-logo" viewBox="0 0 24 24" aria-hidden="true"><path fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" d="M3.2 7.6V5.9c0-1.5 1.2-2.7 2.7-2.7h1.7M16.4 3.2h1.7c1.5 0 2.7 1.2 2.7 2.7v1.7M20.8 16.4v1.7c0 1.5-1.2 2.7-2.7 2.7h-1.7M7.6 20.8H5.9c-1.5 0-2.7-1.2-2.7-2.7v-1.7"/><path fill="#e8e8ec" stroke="#e8e8ec" stroke-width="1.6" stroke-linejoin="round" d="M10 9v6l5.4-3z"/></svg>
    <div>
      <div class="about-name">Frame Player</div>
      {#if appVersion}
        <div class="about-version">{t('set.about_version', { version: appVersion })}</div>
      {/if}
    </div>
  </div>
  <div class="about-links">
    <a
      class="about-link"
      href={SITE_URL}
      data-tip={t('set.about_site_tip')}
      aria-label={t('set.about_site_tip')}
      onclick={(e) => openLink(e, SITE_URL)}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3c2.5 2.6 3.8 5.6 3.8 9s-1.3 6.4-3.8 9c-2.5-2.6-3.8-5.6-3.8-9S9.5 5.6 12 3z" /></svg>
      frameplayer.app
    </a>
    <a
      class="about-link"
      href={REPO_URL}
      data-tip={t('set.about_github_tip')}
      aria-label={t('set.about_github_tip')}
      onclick={(e) => openLink(e, REPO_URL)}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true"><path class="fill" d="M12 .3a12 12 0 0 0-3.8 23.4c.6.1.8-.3.8-.6v-2c-3.3.7-4-1.6-4-1.6-.6-1.4-1.4-1.8-1.4-1.8-1.1-.7.1-.7.1-.7 1.2.1 1.8 1.2 1.8 1.2 1.1 1.9 2.8 1.3 3.5 1 .1-.8.4-1.3.8-1.6-2.7-.3-5.5-1.3-5.5-5.9 0-1.3.5-2.4 1.2-3.2-.1-.3-.5-1.5.1-3.2 0 0 1-.3 3.3 1.2a11.5 11.5 0 0 1 6 0C17.2 4.9 18.2 5.2 18.2 5.2c.6 1.7.2 2.9.1 3.2.8.8 1.2 1.9 1.2 3.2 0 4.6-2.8 5.6-5.5 5.9.4.4.8 1.1.8 2.2v3.3c0 .3.2.7.8.6A12 12 0 0 0 12 .3z" /></svg>
      GitHub
    </a>
  </div>
  {#if webviewVersion}
    <p class="about-note">
      {t(IS_MAC ? 'set.webview_foot_mac' : 'set.webview_foot_win', { version: webviewVersion })}
    </p>
  {/if}
  <p class="about-note">
    {t('set.licenses_foot')}
    <button class="settings-link" onclick={onLicenses}>
      {t('set.licenses_open')}
    </button>
  </p>
</div>

<style>
  /* ---- Excluded folder list ---- */
  .folders {
    margin-top: 10px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .folder-row {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 7px 8px 7px 10px;
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.05);
  }

  .folder-ico {
    flex: none;
    width: 15px;
    height: 15px;
    color: #8f8f9c;
  }

  .folder-path {
    flex: 1;
    min-width: 0;
    /* Truncate the start, not the tail: for paths the meaningful part is the
       last components. The text direction is isolated by <bdi> in the markup. */
    direction: rtl;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #d6d6de;
    font-size: 12px;
  }

  .folder-remove {
    flex: none;
    width: 22px;
    height: 22px;
    display: grid;
    place-items: center;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: #77777f;
    cursor: pointer;
    transition: background 0.12s ease, color 0.12s ease;
  }

  .folder-remove svg {
    width: 9px;
    height: 9px;
    fill: none;
    display: block;
  }

  .folder-row:hover .folder-remove {
    color: #d6d6de;
  }

  .folder-remove:hover {
    background: rgba(255, 255, 255, 0.1);
    color: #fff;
  }


  .folder-add {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
    padding: 8px 10px;
    border: 1px dashed rgba(255, 255, 255, 0.16);
    border-radius: 8px;
    background: transparent;
    color: #b9b9c3;
    font-size: 12.5px;
    cursor: pointer;
    transition: border-color 0.12s ease, color 0.12s ease, background 0.12s ease;
  }

  .folder-add svg {
    width: 14px;
    height: 14px;
    fill: none;
  }

  .folder-add:hover:not(:disabled) {
    border-color: #818cf8;
    background: rgba(129, 140, 248, 0.08);
    color: #e8e8ec;
  }

  .folder-add:disabled,
  .folder-remove:disabled {
    cursor: default;
  }

  .folder-remove:disabled:hover {
    background: transparent;
    color: #77777f;
  }

  /* The About panel: a name and a version, two links, two notes. */
  .report-row {
    display: flex;
    margin-bottom: 4px;
  }

  .about-head {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .about-logo {
    width: 34px;
    height: 34px;
    flex: none;
    color: #818cf8;
  }

  .about-name {
    color: #e8e8ec;
    font-size: 15px;
    font-weight: 600;
  }

  .about-version {
    margin-top: 2px;
    color: #9a9aa5;
    font-size: 12px;
  }

  .about-links {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin: 14px 0;
  }

  .about-link {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    padding: 7px 12px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.05);
    color: #e8e8ec;
    font-size: 12.5px;
    text-decoration: none;
    transition:
      background 0.15s ease,
      border-color 0.15s ease,
      color 0.15s ease;
  }

  .about-link:hover {
    border-color: rgba(129, 140, 248, 0.4);
    background: rgba(129, 140, 248, 0.14);
    color: #fff;
  }

  .about-link svg {
    width: 15px;
    height: 15px;
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.7;
  }

  .about-link svg .fill {
    fill: currentColor;
    stroke: none;
  }

  .about-note {
    margin: 6px 0 0;
    color: #77777f;
    font-size: 11.5px;
    line-height: 1.45;
  }
</style>
