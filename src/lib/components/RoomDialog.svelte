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
  import { CODE_LENGTH, formatCode, normalizeCode, type RoomRules } from '$lib/sync/protocol';
  import {
    displayName,
    joinRoom,
    leaveRoom,
    relayUrl,
    setDisplayName,
    setRoomRules,
    wire,
  } from '$lib/sync/wire.svelte';
  import { fmtSize } from '$lib/units';

  interface Props {
    onclose: () => void;
  }

  let { onclose }: Props = $props();

  let name = $state(displayName());
  /// A code an invitation link brought, if there was one. Read and cleared
  /// before the field exists: the invitation has been delivered the moment it
  /// is in hand, and leaving it set would re-raise this dialog every time the
  /// viewer closed it.
  const invited = invite.code;
  invite.code = '';

  let code = $state(invited);
  /// Which button last copied, not merely *that* something did — one shared
  /// flag put "Скопировано" on the code button when the link was taken.
  let copied = $state<'code' | 'link' | null>(null);

  const canJoin = $derived(normalizeCode(code).length === CODE_LENGTH);
  /// How the viewer got here, not what is currently typed — so a code entered
  /// by hand does not reorder the form under the cursor at its sixth character.
  const fromInvite = invited !== '';

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

  const waitingNames = $derived(wire.waitingFor.map((m) => m.name || t('sync.anon')));

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

  async function copy(what: 'code' | 'link', text: string) {
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      copied = what;
      // Cleared only if it is still this one: pressing the other button in the
      // meantime moves the label there, and clearing then would take the newer
      // confirmation away early.
      setTimeout(() => {
        if (copied === what) copied = null;
      }, 1600);
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
        <button class="btn-outline" onclick={() => copy('code', wire.room)}>
          {copied === 'code' ? t('sync.copied') : t('sync.copy_code')}
        </button>
        {#if link}
          <button class="btn-outline" onclick={() => copy('link', link)}>
            {copied === 'link' ? t('sync.copied') : t('sync.copy_link')}
          </button>
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
      <!-- A failure has to stand, not fade: a viewer whose window is empty
           while the room plays on needs to know it was tried and why it did
           not work, and an OSD is gone before they open this panel. -->
      {#if sync.failed}<div class="link-error">{t('sync.open_failed')}</div>{/if}
    </div>

    <!-- A heading and hairline rows rather than another bordered box: the
         complaint was that the names ran into the prose above and below, which
         is a *grouping* problem, and a list that looks like a list solves it
         without nesting a third surface inside the sheet. The glyph carries it
         at a glance — a column of person marks is legible as "these are people"
         before a word of it is read. -->
    <div class="room-people">
      <div class="setting-label">{t('sync.members', { count: wire.members.length })}</div>
      <ul class="room-list">
        {#each wire.members as member (member.id)}
          <li class="room-person">
            <svg class="room-person-ico" viewBox="0 0 16 16" aria-hidden="true">
              <path
                d="M8 8.2a2.6 2.6 0 1 0 0-5.2 2.6 2.6 0 0 0 0 5.2Zm-4.6 4.9c0-2.5 2.1-3.9 4.6-3.9s4.6 1.4 4.6 3.9"
                fill="none"
                stroke="currentColor"
                stroke-width="1.4"
                stroke-linecap="round"
              />
            </svg>
            <span class="room-name">{member.name || t('sync.anon')}</span>
            {#if member.id === wire.host}<span class="room-badge">{t('sync.host_badge')}</span>{/if}
            {#if member.id === wire.me}<span class="room-badge">{t('sync.you')}</span>{/if}
            {#if !member.ready}<span class="room-badge loading">{t('sync.loading_badge')}</span>{/if}
          </li>
        {/each}
      </ul>
    </div>

    <!-- The room's own rules, all three of them, and all three the host's.
         They sit together because they are one kind of thing — what this room
         does, as opposed to where it is in the film — and a panel where one
         switch answers to a different person than the two beside it is a panel
         nobody can predict. A guest sees them read-only, which is honest: they
         describe the room the guest is in. -->
    <div class="room-rules">
      {@render rule('hostOnly', t('sync.host_only_label'), t('sync.host_only_hint'), wire.hostOnly)}
      {@render rule('shareAudio', t('sync.share_audio'), t('sync.share_audio_hint'), wire.shareAudio)}
      {@render rule('shareSubs', t('sync.share_subs'), t('sync.share_subs_hint'), wire.shareSubs)}
      <!-- Said once for the group rather than on each disabled switch: three
           identical explanations would read as three separate refusals. -->
      {#if !wire.isHost}<p class="setting-hint">{t('sync.rules_guest')}</p>{/if}
    </div>

    <!-- Only once there is somebody to be out of step with. Alone in a room the
         figure is real but answers a question nobody asked, and printing it
         beside a film that is playing perfectly reads as a warning. -->
    {#if wire.members.length > 1 && Number.isFinite(wire.uncertainty)}
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
    <!-- ---- getting in ----

         A sign-in form's shape, because it is a sign-in form's problem: one
         action with no prerequisite, one that needs something you were sent,
         and a rule separating them. The order flips when a code is already in
         hand — arriving from an invitation link, the button you want is the one
         that uses it, and making the viewer find it below a divider would be
         the form arguing with the reason it was opened. -->
    <p class="setting-hint room-lead">{t('sync.lead')}</p>

    <!-- Who you are.
         It has to read as belonging to neither branch below — bare above the
         primary button it was simply the first field of "create a room", and
         somebody joining by code skipped it and arrived nameless. What does
         that here is the plain rule under it, saying "that was about you, this
         is about what you want to do", and nothing else: it is `.room-field`,
         exactly like the code field, because two text inputs in one dialog
         wearing different shapes reads as a mistake rather than as a
         distinction. The placeholder carries what a hint line used to. -->
    <label class="room-field">
      <span class="setting-label">{t('sync.name_label')}</span>
      <input
        class="link-input room-input"
        bind:value={name}
        placeholder={t('sync.name_ph')}
        maxlength="32"
      />
    </label>
    <div class="room-rule"></div>

    {#if fromInvite}
      {@render joinBlock(true)}
      <div class="room-or">{t('sync.or')}</div>
      {@render createButton(false)}
    {:else}
      {@render createButton(true)}
      <div class="room-or">{t('sync.or')}</div>
      {@render joinBlock(false)}
    {/if}

    {#if wire.error}
      <p class="link-error">{t(`sync.err_${wire.error}` as 'sync.err_no_room')}</p>
    {/if}
  {/if}
</Dialog>

{#snippet rule(key: keyof RoomRules, label: string, hint: string, on: boolean)}
  <div class="setting room-rule-row">
    <div class="row-toggle">
      <div class="row-text">
        <div class="setting-label">{label}</div>
        <div class="setting-hint">{hint}</div>
      </div>
      <button
        class="switch"
        class:on
        role="switch"
        aria-checked={on}
        aria-label={label}
        disabled={!wire.isHost}
        onclick={() => setRoomRules({ [key]: !on })}
      >
        <span class="switch-knob"></span>
      </button>
    </div>
  </div>
{/snippet}

{#snippet createButton(primary: boolean)}
  <button
    class="room-wide"
    class:primary
    class:btn-outline={!primary}
    disabled={wire.phase === 'connecting'}
    onclick={create}
  >
    {wire.phase === 'connecting' ? t('sync.connecting') : t('sync.create')}
  </button>
{/snippet}

{#snippet joinBlock(primary: boolean)}
  <label class="room-field">
    <span class="setting-label">{t('sync.join_label')}</span>
    <div class="room-join">
      <!-- svelte-ignore a11y_autofocus -->
      <input
        class="link-input room-input room-code-input"
        bind:value={code}
        placeholder={t('sync.code_ph')}
        spellcheck="false"
        autocapitalize="characters"
        autofocus
        onkeydown={(e) => {
          if (e.key === 'Enter') join();
        }}
      />
      <button class="room-go" class:primary class:btn-outline={!primary} disabled={!canJoin} onclick={join}>
        {t('sync.go')}
      </button>
    </div>
  </label>
{/snippet}


<style>
  /* The fields below carry their own 12px of top margin, so the lead only has
     to make up the difference — otherwise the first field sits noticeably
     further from the text than the others do from each other. */
  .room-lead {
    margin: 0 0 2px;
  }

  /* Plain, wordless, and the only other rule in the dialog carries «или» — so
     the two are not competing to mean the same thing. This one separates who
     you are from what you want to do; that one separates the two things you
     could want. */
  .room-rule {
    height: 1px;
    margin: 16px 0 4px;
    background: rgba(255, 255, 255, 0.1);
  }

  .room-field {
    display: block;
    margin-top: 12px;
  }

  /* The shared `.link-input` carries its own vertical margin, which is right in
     a stack of fields and wrong inside a row — and an explicit height is what
     makes the button beside it match. Both boxes are border boxes (the reset in
     app.css), so this is the outer height of each and does not depend on either
     one's font: `.link-input` is 13px and `.btn-outline` is 15px, which is
     exactly why the button stood ~2px taller than the field it sat next to. */
  .room-input {
    margin: 0;
    height: 38px;
  }

  .room-wide {
    display: block;
    width: 100%;
    text-align: center;
    margin-top: 14px;
  }

  /* The rule with a word in it, which is what says these are alternatives
     rather than steps. `::before`/`::after` rather than a bordered box, so the
     line meets the text at its own height whatever the label says. */
  .room-or {
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 16px 0 4px;
    color: #7a7a88;
    font-size: 12px;
  }

  .room-or::before,
  .room-or::after {
    content: '';
    flex: 1;
    height: 1px;
    background: rgba(255, 255, 255, 0.12);
  }

  .room-go {
    flex: none;
    height: 38px;
    /* `.primary` and `.btn-outline` both carry generous horizontal padding for
       a standalone button; beside a field the label is one word and the button
       should not out-measure the input it belongs to. */
    padding: 0 18px;
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

  /* Smaller than a standalone `.btn-outline`, which is sized for being the
     thing you came to press. These sit beside the code and are how you *take*
     it away — secondary to the six characters they are next to, which are set
     at 26px. Specificity: a scoped `.room-copy .btn-outline` is (0,2,0) against
     the shared `button.btn-outline` at (0,1,1), and classes are compared before
     elements — so this wins wherever the two meet. */
  .room-copy .btn-outline {
    padding: 0 12px;
    height: 30px;
    font-size: 12.5px;
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

  /* Grouped rather than spaced apart: three switches in a row read as one
     subject, which is what they are. The last one drops its margin so the
     group ends where the buttons below begin. */
  .room-rules {
    margin-bottom: 14px;
  }

  .room-rule-row:last-child {
    margin-bottom: 0;
  }

  .room-people {
    margin-bottom: 14px;
  }

  .room-list {
    list-style: none;
    margin: 4px 0 0;
    padding: 0;
  }

  /* Separated by a hairline rather than by a gap: with names of wildly
     different lengths a gap alone leaves a ragged column that reads as prose,
     which is the thing being fixed. The last row drops its rule so the list
     ends where the switches below begin. */
  .room-person {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 0;
    border-bottom: 1px solid rgba(255, 255, 255, 0.06);
    font-size: 13px;
    color: #d6d6de;
  }

  .room-person:last-child {
    border-bottom: none;
    padding-bottom: 0;
  }

  .room-person-ico {
    flex: none;
    width: 13px;
    height: 13px;
    color: #6f6f7a;
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
