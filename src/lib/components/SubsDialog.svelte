<script lang="ts">
  /// Subtitle search and download against OpenSubtitles.
  ///
  /// Markup only — the state, the API calls and the reasoning behind them are in
  /// `$lib/subs.svelte.ts`, because two other places reach into them: the track
  /// menu removes a downloaded subtitle, and the page opens this panel.
  import Dialog from '$lib/components/Dialog.svelte';
  import { t } from '$lib/i18n.svelte';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import {
    OPENSUBTITLES_SIGNUP,
    downloadSub,
    openSubsAuth,
    runSubsSearch,
    subs,
    subsSignIn,
    subsSignOut,
  } from '$lib/subs.svelte';
</script>

<Dialog title={t('subs.title')} variant="subs" onclose={() => (subs.open = false)}>
  <div class="subs-query">
    <input
      class="link-input"
      type="text"
      spellcheck="false"
      autocapitalize="off"
      autocorrect="off"
      placeholder={t('subs.placeholder')}
      bind:this={subs.inputEl}
      bind:value={subs.query}
      onkeydown={(e) => {
        if (e.key === 'Enter') { e.preventDefault(); void runSubsSearch(); }
        if (e.key === 'Escape') { e.preventDefault(); subs.open = false; }
        e.stopPropagation();
      }}
    />
    <button class="primary" disabled={subs.busy} onclick={() => void runSubsSearch()}>
      {subs.busy ? t('subs.searching') : t('subs.search')}
    </button>
  </div>

  <!-- Said out loud: without it, a list of episodes coming back for a
       two-word query reads as the search guessing. -->
  {#if subs.episode}
    <div class="setting-hint">
      {t('subs.episode_from_name', {
        season: subs.episode.season,
        episode: subs.episode.episode,
      })}
    </div>
  {/if}

  <!-- A pill picks one of several named values, which is exactly what a
       language filter is. -->
  <div class="segmented">
    {#each subs.languages as option (option.value)}
      <button
        class="segopt"
        class:sel={subs.lang === option.value}
        onclick={() => { subs.lang = option.value; void runSubsSearch(); }}
      >
        {option.label}
      </button>
    {/each}
  </div>

  <!-- Which of the two searches produced this list is the single most
       load-bearing sentence in the panel: an exact match drops in
       already in sync, a title match may be a different release. The
       "hash matched nothing" case is stated on its own, because that is
       a fact about the file rather than about the search. -->
  {#if subs.hashBlocked}
    <div class="setting-hint">{t('subs.hash_blocked')}</div>
  {:else if subs.hits?.length && subs.matchKind === 'hash'}
    <div class="setting-hint">{t('subs.by_hash')}</div>
  {:else if subs.hits?.length}
    <div class="setting-hint">
      {t(subs.hashTried ? 'subs.no_exact_then_title' : 'subs.by_title')}
    </div>
  {:else if subs.hits && subs.hashTried && !subs.busy}
    <div class="setting-hint">{t('subs.no_exact')}</div>
  {/if}

  {#if subs.error}
    <div class="link-error">{subs.error}</div>
  {/if}

  {#if subs.hits}
    {#if subs.hits.length}
      <div class="subs-list scrollable">
        {#each subs.hits as hit (hit.file_id)}
          <div class="subs-row">
            <div class="subs-main">
              <div class="subs-name" data-tip={hit.release || hit.file_name}>
                {hit.release || hit.file_name}
              </div>
              <div class="subs-meta">
                <span class="subs-lang">{hit.language.toUpperCase()}</span>
                <!-- An episode's own title is usually "Episode 3", so
                     the series and the number are what identify it. -->
                {#if hit.parent && hit.season !== null && hit.episode !== null}
                  <span class="subs-movie">
                    {hit.parent} · S{hit.season}E{hit.episode}
                  </span>
                {:else if hit.movie}
                  <span class="subs-movie">{hit.movie}{hit.year ? ` (${hit.year})` : ''}</span>
                {/if}
                {#if hit.fps !== null}
                  <span
                    class="subs-fps"
                    class:warn={subs.fpsOff(hit.fps)}
                    data-tip={subs.fpsOff(hit.fps) ? t('subs.fps_off') : undefined}
                  >
                    {t('info.fps', { value: hit.fps })}
                  </span>
                {/if}
                <span>{t('subs.downloads', { count: hit.downloads })}</span>
              </div>
              <div class="subs-badges">
                {#if hit.hash_match}<span class="subs-badge match">{t('subs.badge_hash')}</span>{/if}
                {#if hit.from_trusted}<span class="subs-badge">{t('subs.badge_trusted')}</span>{/if}
                {#if hit.hearing_impaired}<span class="subs-badge">{t('subs.badge_hi')}</span>{/if}
                {#if hit.ai_translated}<span class="subs-badge warn">{t('subs.badge_ai')}</span>{/if}
              </div>
            </div>
            <button
              class="btn-outline subs-sm subs-get"
              disabled={subs.busyId !== null}
              onclick={() => void downloadSub(hit)}
            >
              {subs.busyId === hit.file_id ? t('subs.downloading') : t('subs.download')}
            </button>
          </div>
        {/each}
      </div>
    {:else if !subs.busy}
      <div class="setting-hint">{t('subs.empty')}</div>
    {/if}
  {/if}

  {#if subs.quota && subs.quota.remaining !== null}
    <div class="setting-hint">
      {t('subs.quota', { count: subs.quota.remaining })}
      {#if subs.quota.reset}· {t('subs.quota_reset', { time: subs.quota.reset })}{/if}
    </div>
  {/if}

  <!-- The account, folded away until asked for: downloading works
       without one, and signing in only raises the daily ceiling. -->
  <div class="subs-account">
    {#if subs.account?.signed_in}
      <div class="subs-account-line">
        <span>{t('subs.signed_in', { name: subs.account.username ?? '' })}</span>
        {#if subs.account.remaining_downloads !== null && subs.account.allowed_downloads !== null}
          <span class="subs-account-quota">
            {t('subs.quota_account', {
              remaining: subs.account.remaining_downloads,
              allowed: subs.account.allowed_downloads,
            })}
          </span>
        {/if}
        <button
          class="btn-outline subs-sm"
          disabled={subs.authBusy}
          onclick={() => void subsSignOut()}
        >
          {t('subs.sign_out')}
        </button>
      </div>
      {#if subs.account.keychain_failed}
        <div class="setting-hint">{t('subs.keychain_failed')}</div>
      {/if}
    {:else if subs.authOpen}
      <div class="subs-auth" bind:this={subs.authEl}>
        <input
          class="link-input"
          type="text"
          autocomplete="username"
          spellcheck="false"
          autocapitalize="off"
          autocorrect="off"
          placeholder={t('subs.username')}
          bind:this={subs.authUserEl}
          bind:value={subs.user}
          onkeydown={(e) => e.stopPropagation()}
        />
        <input
          class="link-input"
          type="password"
          autocomplete="current-password"
          placeholder={t('subs.password')}
          bind:value={subs.pass}
          onkeydown={(e) => {
            if (e.key === 'Enter') { e.preventDefault(); void subsSignIn(); }
            e.stopPropagation();
          }}
        />
        <!-- A boolean is a switch in this UI, never a pill. Labeled
             the way the settings rows are: the text is a sibling and
             the switch carries its own `aria-label`, because a `<label>`
             around a `<button>` labels nothing and does not forward
             clicks either. -->
        <div class="subs-remember">
          <button
            class="switch"
            class:on={subs.remember}
            role="switch"
            aria-checked={subs.remember}
            aria-label={t('subs.remember')}
            onclick={() => (subs.remember = !subs.remember)}
          ><span class="switch-knob"></span></button>
          <span>{t('subs.remember')}</span>
        </div>
        <div class="setting-hint">{t('subs.remember_hint')}</div>
        <!-- A link, not a button: this genuinely navigates — it opens
             the registration page in the browser. -->
        <div class="setting-hint">
          {t('subs.no_account')}
          <button class="settings-link" onclick={() => void openUrl(OPENSUBTITLES_SIGNUP)}>
            {t('subs.register')}
          </button>
        </div>
        {#if subs.authError}
          <div class="link-error">{subs.authError}</div>
        {/if}
        <div class="link-actions">
          <button class="btn-outline" onclick={() => { subs.authOpen = false; subs.pass = ''; }}>
            {t('subs.cancel')}
          </button>
          <button
            class="primary"
            disabled={subs.authBusy || !subs.user.trim() || !subs.pass}
            onclick={() => void subsSignIn()}
          >
            {subs.authBusy ? t('subs.signing_in') : t('subs.sign_in')}
          </button>
        </div>
      </div>
    {:else}
      <div class="subs-account-line">
        <span class="subs-account-hint">{t('subs.sign_in_hint')}</span>
        <button class="btn-outline subs-sm" onclick={() => void openSubsAuth()}>
          {t('subs.sign_in')}
        </button>
      </div>
    {/if}
  </div>
</Dialog>

<style>
  /* The row owns the vertical spacing, not the field inside it: `.link-input`
     carries `margin: 4px 0 8px` for the link dialog, where it is the only
     thing in its row, and an asymmetric margin on a centered flex item tilts
     the pair — the field sat 2px high and the button looked dropped. */
  .subs-query {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 4px 0 8px;
  }

  .subs-query .link-input {
    flex: 1;
    min-width: 0;
    margin: 0;
  }

  .subs-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-top: 10px;
    /* Bounded so the dialog cannot outgrow a small window; the rows scroll. */
    max-height: min(46vh, 340px);
    overflow-y: auto;
    overflow-x: hidden;
  }

  .subs-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 8px 8px 10px;
    border-radius: 8px;
  }

  .subs-row:hover {
    background: rgba(255, 255, 255, 0.06);
  }

  /* min-width: 0 on the column AND on the label below it, or the flex row
     grows to the longest release name instead of ellipsising it. */
  .subs-main {
    flex: 1;
    min-width: 0;
  }

  .subs-name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #e8e8ec;
    font-size: 13px;
  }

  .subs-meta {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 3px;
    color: #9a9aa5;
    font-size: 11.5px;
    white-space: nowrap;
  }

  .subs-lang {
    color: #b9b9c3;
    font-weight: 600;
    letter-spacing: 0.04em;
  }

  .subs-movie {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* The one answer available to "will this fit": OpenSubtitles carries no
     duration for a subtitle, and a frame-rate mismatch is the failure that
     drifts further out the longer you watch. */
  .subs-fps {
    flex: none;
  }

  .subs-fps.warn {
    color: #f0a0a0;
  }

  .subs-badges {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 5px;
  }

  .subs-badge {
    padding: 1px 7px;
    border: 1px solid rgba(255, 255, 255, 0.14);
    border-radius: 999px;
    color: #9a9aa5;
    font-size: 10.5px;
  }

  /* Indigo means one thing in this UI — selected / on / primary — and "this is
     the one that matches your file" is exactly that. */
  .subs-badge.match {
    border-color: transparent;
    background: #6366f1;
    color: #fff;
  }

  .subs-badge.warn {
    border-color: rgba(240, 160, 160, 0.4);
    color: #f0a0a0;
  }

  /* A control inside a row, not a dialog action: `.btn-outline`'s own 9/20
     padding and 15px type are sized for the footer of a dialog, where there is
     one of them. In a list there is one per row, and at that size they dominate
     what they are attached to. */
  button.subs-sm {
    padding: 5px 13px;
    border-radius: 7px;
    font-size: 12.5px;
  }

  .subs-get {
    flex: none;
  }

  .subs-account {
    margin-top: 12px;
    padding-top: 10px;
    border-top: 1px solid rgba(255, 255, 255, 0.08);
  }

  .subs-account-line {
    display: flex;
    align-items: center;
    gap: 10px;
    color: #b9b9c3;
    font-size: 12.5px;
  }

  /* The hint gives up its width first: the button must not be pushed off. */
  .subs-account-hint {
    flex: 1;
    min-width: 0;
  }

  .subs-account-quota {
    flex: 1;
    min-width: 0;
    color: #9a9aa5;
    text-align: right;
  }

  .subs-auth {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .subs-auth .link-input {
    margin: 0;
  }

  .subs-remember {
    display: flex;
    align-items: center;
    gap: 10px;
    color: #d6d6de;
    font-size: 13px;
  }
</style>
