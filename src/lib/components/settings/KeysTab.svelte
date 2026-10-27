<script lang="ts">
  /// Settings → Keys: the hotkey editor.
  import { IS_MAC } from '$lib/platform';
  import { t } from '$lib/i18n.svelte';
  import {
    ACTIONS,
    GROUP_ORDER,
    actionLabel,
    assign,
    chordLabel,
    chordOf,
    chordsOf,
    hasCustomBindings,
    isCustom,
    isMenuAccelerator,
    isModifierCode,
    reservedReason,
    resetAction,
    resetAll,
    unassign,
    type ActionId,
  } from '$lib/keys.svelte';

  interface Props {
    /// One line under one row: what a rebind took away, or why a chord was
    /// refused. Cleared by the next attempt, so it reads as a reply rather than
    /// as a status the panel keeps. The dialog's rather than this tab's, so
    /// the reply is still there after a visit to another tab.
    keyNote: { id: ActionId; text: string } | null;
  }

  let { keyNote = $bindable() }: Props = $props();

  // ---- Key bindings editor -------------------------------------------------

  /// The action whose next keystroke is being captured, or null.
  let recordingAction = $state<ActionId | null>(null);

  const RESERVED_NOTE = {
    digits: 'keys.taken_digits',
    contextual: 'keys.taken_contextual',
    menu: 'keys.taken_menu',
  } as const;

  /// Capture phase, on the window: the player's own handler is a *bubble*
  /// listener on the same window, and stopping propagation on the way down
  /// means the keystroke never reaches it. Without that, recording a binding
  /// for "полный экран" would go fullscreen while recording it. `preventDefault`
  /// is the other half — Space would otherwise re-click the button that started
  /// the recording, and Tab would move focus out of the row.
  function recordChord(e: KeyboardEvent) {
    e.preventDefault();
    e.stopPropagation();
    // A modifier on its own is not a chord: the viewer is still reaching for
    // the key, so keep waiting rather than binding ⇧ to anything.
    if (isModifierCode(e.code)) return;
    const id = recordingAction;
    if (!id) return;
    const chord = chordOf(e);
    // Bare Escape cancels. It is reserved anyway, so without this the panel
    // would answer an attempt to back out by explaining why Escape is taken.
    if (chord === 'Escape') {
      stopRecording();
      return;
    }
    const reason = reservedReason(chord);
    if (reason) {
      keyNote = { id, text: t(RESERVED_NOTE[reason]) };
      stopRecording();
      return;
    }
    const stolen = assign(id, chord);
    keyNote = stolen ? { id, text: t('keys.stolen', { action: actionLabel(stolen) }) } : null;
    stopRecording();
  }

  function startRecording(id: ActionId) {
    keyNote = null;
    recordingAction = id;
    window.addEventListener('keydown', recordChord, true);
  }

  function stopRecording() {
    recordingAction = null;
    window.removeEventListener('keydown', recordChord, true);
  }

  $effect(() => {
    // Recording must not outlive the panel that started it: a capture listener
    // left on the window would swallow the next key pressed at the video, which
    // reads as the player having frozen rather than as a dialog left open.
    // This tab is unmounted when another one is shown, so that is the moment.
    return stopRecording;
  });
</script>

{#each GROUP_ORDER as group (group)}
  <div class="set-group">{t(`kgroup.${group}`)}</div>
  <div class="set-panel">
  {#each ACTIONS.filter((a) => a.group === group) as def (def.id)}
    {@const chords = chordsOf(def.id)}
    <div class="setting keyrow">
      <div class="keyrow-name">{actionLabel(def.id)}</div>
      <div class="keyrow-chords">
        {#each chords as chord (chord)}
          <!-- A chord the macOS menu bar owns is shown but not
               removable: taking it off here would change nothing,
               since the system delivers it from the menu either way. -->
          <span class="kbadge" class:fixed={isMenuAccelerator(chord)}>
            {chordLabel(chord)}
            {#if !isMenuAccelerator(chord)}
              <button
                class="kbadge-x"
                data-tip={t('keys.remove')}
                aria-label={t('keys.remove')}
                onclick={() => unassign(def.id, chord)}
              >
                <svg viewBox="0 0 10 10"><path stroke="currentColor" d="M0 0l10 10M10 0L0 10"/></svg>
              </button>
            {/if}
          </span>
        {/each}
        {#if IS_MAC && def.menuMac && !chords.includes(def.menuMac)}
          <span class="kbadge fixed" data-tip={t('keys.menu_note')}>
            {chordLabel(def.menuMac)}
          </span>
        {/if}
        {#if chords.length === 0 && !(IS_MAC && def.menuMac)}
          <span class="kbadge empty">{t('keys.none')}</span>
        {/if}
        {#if recordingAction === def.id}
          <span class="kbadge recording">{t('keys.press')}</span>
        {:else}
          <button
            class="kadd"
            data-tip={t('keys.add')}
            aria-label={t('keys.add')}
            onclick={() => startRecording(def.id)}
          >+</button>
        {/if}
        {#if isCustom(def.id)}
          <button class="settings-link kreset" onclick={() => { keyNote = null; resetAction(def.id); }}>
            {t('keys.restore')}
          </button>
        {/if}
      </div>
    </div>
    {#if recordingAction === def.id}
      <div class="setting-hint keynote">{t('keys.press_hint')}</div>
    {:else if keyNote?.id === def.id}
      <div class="setting-hint keynote">{keyNote.text}</div>
    {:else if def.note}
      <div class="setting-hint keynote">{t(def.note)}</div>
    {/if}
  {/each}
  </div>
{/each}
<div class="settings-foot">{t('keys.footer')}</div>
<button class="btn-danger" disabled={!hasCustomBindings()} onclick={() => { keyNote = null; resetAll(); }}>
  {t('keys.reset_all')}
</button>

<style>
  /* ---- Key bindings editor ----
     A row is a flex pair rather than the label-above-control shape the rest of
     the dialog uses: thirty-six actions at two lines each is a page of
     scrolling, and a chord is short enough to sit beside its own name. */

  /* `min-width: 0` on the item AND on the row above it, or a long action name
     refuses to shrink below its content and the dialog grows a horizontal
     scrollbar instead of ellipsising — the same trap the queue rows hit. */
  .keyrow-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #b9b9c3;
    font-size: 12.5px;
  }

  .keyrow-chords {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    flex-wrap: wrap;
    gap: 6px;
  }

  .kbadge {
    display: inline-flex;
    align-items: center;
    /* Measured against the shipped stylesheet: the glyph sits 9px from the
       chord text and 9px from the badge's padding box (10px from the border
       itself, which is the extra pixel of the border and reads as balanced).
       With the button flush against the text (gap: 0) its own 5px of internal
       padding was all that separated the two, against 8px on the outside — so
       the glyph read as pushed into the text it sat next to. */
    gap: 4px;
    padding: 3px 4px 3px 8px;
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 6px;
    background: rgba(255, 255, 255, 0.06);
    color: #e6e6ec;
    font-size: 12px;
    line-height: 1.3;
    white-space: nowrap;
  }

  /* Nothing to take off these two, so no × and no room held for one. */
  .kbadge.fixed,
  .kbadge.empty {
    padding-right: 8px;
    color: #77777f;
  }

  .kbadge.empty {
    border-style: dashed;
    background: none;
  }

  /* Indigo means one thing here as everywhere else: this is the live one. */
  .kbadge.recording {
    border-color: #6366f1;
    background: rgba(99, 102, 241, 0.18);
    color: #fff;
  }

  /* An 18px square, not a 7px glyph: the icon is the *mark*, the button is the
     target. As a bare <svg> the hit area was the strokes themselves, which on a
     badge this size is a few pixels of diagonal line — missable even when aimed
     at deliberately. The square is padding, so the glyph keeps its size. */
  .kbadge-x {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 18px;
    height: 18px;
    padding: 0;
    border: none;
    border-radius: 4px;
    background: none;
    color: #77777f;
    cursor: pointer;
    transition: background 0.12s ease, color 0.12s ease;
  }

  .kbadge-x svg {
    width: 8px;
    height: 8px;
    fill: none;
    stroke-width: 1.6;
  }

  /* Red on hover for the same reason .btn-danger is red: this one removes. */
  .kbadge-x:hover {
    background: rgba(248, 113, 113, 0.16);
    color: #f87171;
  }

  .kadd {
    width: 22px;
    height: 22px;
    padding: 0;
    border: 1px dashed rgba(255, 255, 255, 0.18);
    border-radius: 6px;
    background: none;
    color: #9a9aa5;
    font-size: 14px;
    line-height: 1;
    cursor: pointer;
  }

  .kadd:hover {
    border-color: #818cf8;
    border-style: solid;
    color: #fff;
  }

  .kreset {
    font-size: 11.5px;
  }

  /* Sits under the row it answers, not as a hint about the next one. */
  .keynote {
    margin: -2px 0 10px;
  }

  .setting.keyrow {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 28px;
    margin-bottom: 6px;
  }
</style>
