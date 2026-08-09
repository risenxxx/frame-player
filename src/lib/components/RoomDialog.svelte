<script lang="ts">
  /// Watching together: getting into a room, and everything about the one you
  /// are in.
  ///
  /// One dialog with two faces rather than two components, because from the
  /// viewer's side it is one thing — "watch together" — and which face they get
  /// is simply whether they are already in a room. Splitting it would mean two
  /// menu entries for one idea, and a second place to decide which to show.
  ///
  /// The room *server* is deliberately NOT here. It was, on the argument that
  /// this is where you find out you need one — but practically nobody runs their
  /// own relay, so a field on the way into every room asked a question with one
  /// answer and put it in front of the two controls that matter. It lives in
  /// settings, under «Основные»; this dialog only points there, and only when
  /// the address turns out to be wrong.
  import Dialog from '$lib/components/Dialog.svelte';
  import { formatTime } from '$lib/format';
  import { t } from '$lib/i18n.svelte';
  import { showOsd } from '$lib/osd.svelte';
  import { sync } from '$lib/sync/apply.svelte';
  import { invite } from '$lib/sync/link.svelte';
  import { CODE_LENGTH, formatCode, normalizeCode } from '$lib/sync/protocol';
  import {
    displayName,
    joinRoom,
    leaveRoom,
    relayUrl,
    setDisplayName,
    setHostOnly,
    wire,
  } from '$lib/sync/wire.svelte';
  import { fmtSize } from '$lib/units';

  interface Props {
    onclose: () => void;
  }

  let { onclose }: Props = $props();

  let name = $state(displayName());
  /// A code an invitation link brought, if there was one. Taken here and
  /// cleared at once: the invitation has been delivered the moment the field
  /// holds it, and leaving it set would re-raise this dialog every time the
  /// viewer closed it.
  let code = $state(takeInvite());

  function takeInvite(): string {
    const pending = invite.code;
    invite.code = '';
    return pending;
  }
  let copied = $state(false);

  const canJoin = $derived(normalizeCode(code).length === CODE_LENGTH);

  /// A link a friend can actually be sent. The relay serves a page at `/j/<code>`
  /// which offers `frameplayer://join/<code>` — a bare custom-scheme link is
  /// left as plain text by most chat applications, so it would not be clickable
  /// where people actually paste it.
  const link = $derived.by(() => {
    const base = relayUrl().trim().replace(/\/+$/, '');
    if (!base || !wire.room) return '';
    const withScheme = /^[a-z]+:\/\//i.test(base) ? base : `https://${base}`;
    return `${withScheme.replace(/^ws/, 'http')}/j/${wire.room}`;
  });

  const waitingNames = $derived(wire.waitingFor.map((m) => m.name || t('sync.you')));

  function persist() {
    setDisplayName(name);
  }

  function create() {
    persist();
    joinRoom(null);
  }

  function join() {
    if (!canJoin) return;
    persist();
    joinRoom(code);
  }

  async function copy(text: string) {
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      copied = true;
      setTimeout(() => (copied = false), 1600);
    } catch {
      // Clipboard permission is the webview's to refuse; the code is on screen
      // in full either way, which is what it is there for.
      showOsd(text);
    }
  }

  /// What the room is watching, as a sentence. Deliberately not the raw title
  /// for a hidden file: that is the whole point of hiding it.
  const watching = $derived.by(() => {
    const content = wire.timeline.content;
    if (!content) return null;
    if (content.kind === 'hidden') return { title: t('sync.hidden'), note: t('sync.hidden_note') };
    if (content.kind === 'file') {
      const meta = t('sync.local_meta', {
        duration: formatTime(content.duration),
        size: fmtSize(content.size),
      });
      return { title: content.title, note: `${t('sync.local_note')} ${meta}` };
    }
    return { title: content.title, note: '' };
  });

  const verdict = $derived.by(() => {
    if (!wire.timeline.content || sync.match === 'unknown') return '';
    return t(`sync.match_${sync.match}` as 'sync.match_exact');
  });
</script>

<Dialog title={t('sync.title')} variant="link" {onclose}>
  {#if wire.on}
    <!-- ---- in a room ---- -->
    <div class="room-code">
      <span class="room-code-value">{formatCode(wire.room)}</span>
      <div class="room-copy">
        <button class="btn-outline" onclick={() => copy(wire.room)}>
          {copied ? t('sync.copied') : t('sync.copy_code')}
        </button>
        {#if link}
          <button class="btn-outline" onclick={() => copy(link)}>{t('sync.copy_link')}</button>
        {/if}
      </div>
    </div>

    {#if waitingNames.length}
      <p class="room-waiting">
        {sync.holdingUp
          ? t('sync.waiting_you')
          : waitingNames.length === 1
            ? t('sync.waiting_one', { name: waitingNames[0] })
            : t('sync.waiting_many', { count: waitingNames.length })}
        <span class="room-sub">{t('sync.waiting_hint')}</span>
      </p>
    {/if}

    <div class="room-now">
      {#if watching}
        <div class="room-now-title">{t('sync.watching')}: {watching.title}</div>
        {#if watching.note}<div class="room-sub">{watching.note}</div>{/if}
        {#if verdict}<div class="room-sub">{verdict}</div>{/if}
      {:else}
        <div class="room-now-title">{t('sync.nothing')}</div>
        <div class="room-sub">{t('sync.nothing_hint')}</div>
      {/if}
      {#if sync.opening}<div class="room-sub">{t('sync.opening')}</div>{/if}
    </div>

    <ul class="room-people">
      {#each wire.members as member (member.id)}
        <li class="room-person">
          <span class="room-name">{member.name || t('sync.you')}</span>
          {#if member.id === wire.host}<span class="room-badge">{t('sync.host_badge')}</span>{/if}
          {#if member.id === wire.me}<span class="room-badge">{t('sync.you')}</span>{/if}
          {#if !member.ready}<span class="room-badge loading">{t('sync.loading_badge')}</span>{/if}
        </li>
      {/each}
    </ul>

    <div class="setting">
      <span class="setting-label">{t('sync.host_only_label')}</span>
      <button
        class="switch"
        class:on={wire.hostOnly}
        role="switch"
        aria-checked={wire.hostOnly}
        aria-label={t('sync.host_only_label')}
        disabled={!wire.isHost}
        onclick={() => setHostOnly(!wire.hostOnly)}
      >
        <span class="switch-knob"></span>
      </button>
    </div>
    <p class="setting-hint">{t('sync.host_only_hint')}</p>

    {#if Number.isFinite(wire.uncertainty)}
      <p class="setting-hint">{t('sync.clock', { ms: Math.round(wire.uncertainty) })}</p>
    {/if}

    {#if wire.error}
      <p class="link-error">{t(`sync.err_${wire.error}` as 'sync.err_no_room')}</p>
    {/if}

    <div class="link-actions">
      <button
        class="btn-danger"
        onclick={() => {
          leaveRoom();
          onclose();
        }}>{t('sync.leave')}</button
      >
    </div>
  {:else}
    <!-- ---- getting in ---- -->
    <p class="setting-hint">{t('sync.lead')}</p>

    <label class="room-field">
      <span class="setting-label">{t('sync.name_label')}</span>
      <input class="link-input" bind:value={name} placeholder={t('sync.name_ph')} maxlength="32" />
    </label>

    <label class="room-field">
      <span class="setting-label">{t('sync.join_label')}</span>
      <div class="room-join">
        <!-- svelte-ignore a11y_autofocus -->
        <input
          class="link-input room-code-input"
          bind:value={code}
          placeholder={t('sync.code_ph')}
          spellcheck="false"
          autocapitalize="characters"
          autofocus
          onkeydown={(e) => {
            if (e.key === 'Enter') join();
          }}
        />
        <button class="btn-outline" disabled={!canJoin} onclick={join}>
          {t('sync.go')}
        </button>
      </div>
    </label>

    {#if wire.error}
      <p class="link-error">{t(`sync.err_${wire.error}` as 'sync.err_no_room')}</p>
    {/if}

    <div class="link-actions">
      <button class="primary" disabled={wire.phase === 'connecting'} onclick={create}>
        {wire.phase === 'connecting' ? t('sync.connecting') : t('sync.create')}
      </button>
    </div>
  {/if}
</Dialog>

<style>
  .room-field {
    display: block;
    margin-top: 12px;
  }

  .room-field .setting-label {
    display: block;
    margin-bottom: 5px;
  }

  .room-join {
    display: flex;
    gap: 8px;
  }

  /* The code is read aloud and typed back, so it is set in a monospaced face
     with letter spacing — the two things that stop `0`/`O` and `1`/`l` being
     guessed at. The alphabet has already removed the worst of those (see
     `normalizeCode`); this removes the rest. */
  .room-code-input {
    flex: 1;
    min-width: 0;
    font-family: ui-monospace, 'SF Mono', Menlo, Consolas, monospace;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  .room-code {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
    margin-bottom: 14px;
  }

  .room-code-value {
    font-family: ui-monospace, 'SF Mono', Menlo, Consolas, monospace;
    font-size: 26px;
    font-weight: 600;
    letter-spacing: 0.14em;
    color: #e8e8ec;
  }

  .room-copy {
    display: flex;
    gap: 8px;
  }

  .room-waiting {
    margin: 0 0 12px;
    padding: 9px 11px;
    border: 1px solid rgba(129, 140, 248, 0.45);
    border-radius: 9px;
    color: #e8e8ec;
    font-size: 13px;
  }

  .room-now {
    margin-bottom: 12px;
  }

  .room-now-title {
    color: #e8e8ec;
    font-size: 13px;
    /* A title can be anything, including a path-shaped name from a torrent, and
       the dialog is positioned by its own width — so it wraps rather than
       stretching the sheet. */
    overflow-wrap: anywhere;
  }

  .room-sub {
    display: block;
    margin-top: 3px;
    color: #9a9aa6;
    font-size: 12px;
    overflow-wrap: anywhere;
  }

  .room-people {
    list-style: none;
    margin: 0 0 14px;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  .room-person {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: 13px;
    color: #d6d6de;
  }

  .room-name {
    /* Ellipsis needs this on the item as well as on the row — a flex item does
       not shrink below its content unless told to. */
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .room-badge {
    flex: none;
    padding: 1px 6px;
    border-radius: 5px;
    background: rgba(255, 255, 255, 0.08);
    color: #9a9aa6;
    font-size: 11px;
  }

  /* The one badge worth the accent: it is the reason the room is standing still. */
  .room-badge.loading {
    background: rgba(99, 102, 241, 0.22);
    color: #c7cbff;
  }
</style>
