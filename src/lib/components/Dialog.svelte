<script lang="ts">
  /// The shell every dialog in the player shares: backdrop, sheet, head, close.
  ///
  /// Seven dialogs (settings, link, yt-dlp update, torrent file picker,
  /// subtitles, media info, cast diagnosis) were each carrying their own copy
  /// of this markup, which is how they drifted — one guarded its backdrop click
  /// against a busy state and the rest did not, one wrote the head inline and
  /// the rest across five lines. The shell is the same in all of them, so it is
  /// written once.
  ///
  /// **The body stays with its caller.** `children` is a snippet rendered by
  /// whoever passes it, so its content keeps *that* component's style scope —
  /// which is why moving this shell out needed no change to a single rule
  /// belonging to a dialog's contents. Only the shell's own CSS moved here.
  ///
  /// The variant is a named prop rather than a class string because Svelte
  /// prunes CSS selectors it cannot statically see used in the template: a
  /// `class={sheetClass}` passed in from outside would leave `.link-dialog`
  /// looking unused and it would be dropped from the bundle.
  import type { Snippet } from 'svelte';
  import { blockContextMenu } from '$lib/dom';
  import { t } from '$lib/i18n.svelte';

  interface Props {
    /// Shown in the head. Also the accessible name unless `label` overrides it —
    /// the torrent picker displays the torrent's name but announces itself as
    /// the picker.
    title: string;
    label?: string;
    /// Sheet geometry. `settings` is the default width the tab row sets.
    variant?: 'settings' | 'link' | 'subs' | 'diag';
    /// Marks a sheet whose content scrolls. Purely documentation — the
    /// scrollbar skin in app.css is universal (see the note there).
    scrollable?: boolean;
    /// While set, neither the backdrop nor the × closes the dialog. The yt-dlp
    /// update dialog uses it to hold itself open across a download.
    closeDisabled?: boolean;
    onclose: () => void;
    children: Snippet;
  }

  let {
    title,
    label,
    variant = 'settings',
    scrollable = false,
    closeDisabled = false,
    onclose,
    children,
  }: Props = $props();

  function requestClose() {
    if (closeDisabled) return;
    onclose();
  }
</script>

<div
  class="settings-backdrop"
  role="presentation"
  onclick={requestClose}
  ondblclick={(e) => e.stopPropagation()}
  oncontextmenu={blockContextMenu}
>
  <div
    class="settings"
    class:link-dialog={variant === 'link'}
    class:subs-dialog={variant === 'subs'}
    class:diag-dialog={variant === 'diag'}
    class:scrollable
    role="dialog"
    aria-label={label ?? title}
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
  >
    <div class="settings-head">
      <span>{title}</span>
      <button
        class="settings-close"
        data-tip={t('set.close')}
        aria-label={t('bar.close')}
        disabled={closeDisabled}
        onclick={requestClose}
      >
        <svg viewBox="0 0 10 10"><path stroke="currentColor" d="M0 0l10 10M10 0L0 10"/></svg>
      </button>
    </div>
    {@render children()}
  </div>
</div>

<style>
  .settings-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.45);
    display: grid;
    place-items: center;
    /* The dialog's inset from the window, kept here rather than as a magic
       number inside the dialog's own max-height: the padding bounds the grid
       area, so `max-height: 100%` on the child follows it automatically. It
       needs to be generous — the settings list grew tall enough to reach the
       window edges at an ordinary player height, and a panel flush against the
       frame reads as a menu, not a dialog. The top figure clears the title bar
       (48px of content) and the bottom matches it, so centring stays centred.
       No `safe` needed unlike the start screen: this child scrolls internally,
       so it never overflows the grid area in the first place. */
    padding: 56px 24px;
    z-index: 70;
  }

  /* The tab row is what sets this number, and the Russian labels are the
     binding case. Measured against the built stylesheet rather than a
     hand-written copy of it (see the queue-row note): the seven of them render
     to **510px** at the real 13px font and 13px gap, English to 408.

     598 is that 560 plus the sheet's own 36px of horizontal room and 2px of
     border, because this is an outer width now — the number changed, the sheet
     did not. Content is 560 whether or not the sheet scrolls (see the gutter
     note below), so the tab row always has exactly 50px of slack.

     The old 428 was already 13px short of *six* tabs, so "Клавиши" was being
     clipped by a row whose overflow scroll is invisible by design
     (`scrollbar-width: none`). The seventh tab did not create that; it made it
     impossible to miss. */
  .settings {
    width: min(598px, 100%);
    /* Against the backdrop's padded grid area, and now honestly: as a
       content-box height this let the sheet stand 32px taller than the area it
       was centred in, so at full height it reached 39px from the top of the
       window and did not in fact clear the 48px title bar the backdrop's 56px
       of padding was chosen to clear. */
    max-height: 100%;
    /* `scroll`, not `auto`, and that is the whole trick — see the padding note
       below. The bar is only ever visible when there is something to scroll:
       the track is transparent and WebKit draws no thumb on a sheet that fits,
       so what this buys is 10px of reserved width and nothing on screen. */
    overflow-y: scroll;
    background: rgba(16, 16, 22, 0.97);
    border: 1px solid rgba(255, 255, 255, 0.09);
    border-radius: 14px;
    /* The right side is 8px of padding plus the scrollbar's own 10px, which is
       the same 18px as the left — asymmetric in the declaration so that it is
       symmetric on screen. A scrollbar takes its width out of the *content* box
       and leaves `padding-right` inside it, so without a permanent reservation
       everything right-aligned in this sheet moves by the bar's 10px the moment
       the content outgrows the window: measured, the × sits 19px from the
       sheet's edge on a short tab and 29px on a tall one, and the tab row loses
       10px of its 50px of slack with it.

       That reservation used to be `scrollbar-gutter: stable`, and on macOS it
       did nothing at all — which inverted the bug rather than fixing it: with
       the padding already cut to 8px, a sheet that did NOT scroll had 9px on
       the right against 19px on the left. Measured in a WKWebView harness
       against this built stylesheet, `overflow-y: auto` + `scrollbar-gutter:
       stable`, short content: `offsetWidth - clientWidth` = 0, gaps 19/9;
       scrolling: 19/19. Chromium honoured the gutter in both, so this was only
       ever wrong on macOS. `CSS.supports('scrollbar-gutter','stable')` returns
       **true** there and the computed style reads back `stable`, so neither
       feature detection nor the computed value would have caught it.

       Forcing the scrollbar on is what actually reserves the space, and it
       measures identically on both engines: 10px reserved, 19/19 whether the
       sheet scrolls or not. */
    padding: 16px 8px 14px 18px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.5);
  }

  /* Narrower than the settings sheet: it holds one field. Outer, like the
     sheet's own width — 520 of content plus the same 38 of padding, gutter and
     border. */
  .link-dialog {
    width: min(558px, 100%);
  }

  /* Wider than the link dialog: a row here carries a release name, which is
     the one field that must not be truncated to uselessness — it is how you
     tell one rip from another. */
  .subs-dialog {
    width: min(680px, 100%);
  }

  /* `.settings` is a block box, so the `gap` this used to declare did nothing
     and the report ran into the device line above it and the footer below.
     Making the sheet a column is the fix; the two larger gaps are where the
     subject changes — from "which device" to "what it answered", and from the
     answers to what you can do with them. */
  .diag-dialog {
    display: flex;
    flex-direction: column;
    max-width: min(560px, calc(100vw - 48px));
    gap: 12px;
  }

  .settings-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 12px;
    color: #e8e8ec;
    font-size: 15px;
    font-weight: 500;
  }

  .settings-close {
    width: 30px;
    height: 30px;
    display: grid;
    place-items: center;
    background: transparent;
    border: none;
    border-radius: 7px;
    color: #d6d6de;
    cursor: pointer;
  }

  .settings-close svg {
    width: 9px;
    height: 9px;
  }

  .settings-close:hover {
    background: rgba(255, 255, 255, 0.1);
  }
</style>
