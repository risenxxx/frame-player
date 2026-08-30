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
  import ScrollFade from '$lib/components/ScrollFade.svelte';
  import { blockContextMenu } from '$lib/dom';
  import { t } from '$lib/i18n.svelte';

  interface Props {
    /// Shown in the head. Also the accessible name unless `label` overrides it —
    /// the torrent picker displays the torrent's name but announces itself as
    /// the picker.
    title: string;
    label?: string;
    /// Sheet geometry. `settings` is the default width the tab row sets.
    variant?: 'settings' | 'link' | 'subs' | 'diag' | 'catalog';
    /// Marks a sheet whose content scrolls. Purely documentation — the
    /// scrollbar skin in app.css is universal (see the note there).
    scrollable?: boolean;
    /// Rendered under the title, inside the block that stays pinned to the top
    /// of the sheet — the settings tab row is what this exists for. It is a
    /// snippet of the *caller's* rather than a prop on this shell for the
    /// reason `children` is: the tab row's markup, its state and its styles all
    /// belong to the dialog that owns the sections, and only its position on
    /// screen belongs here. Its parameter scrolls the sheet back to the top,
    /// which a tab row needs and cannot reach on its own.
    header?: Snippet<[() => void]>;
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
    header,
    onclose,
    children,
  }: Props = $props();

  let sheet = $state<HTMLDivElement | undefined>();
  /// Whether anything has scrolled up behind the pinned block. It is only ever
  /// the divider's business: content sliding under an edge that draws nothing
  /// reads as content being cut off, and a divider standing over a sheet that
  /// has not moved is a line drawn about nothing.
  let stuck = $state(false);

  $effect(() => {
    const box = sheet;
    if (!box) return;
    /// The one event this needs. A tab switch that leaves the new section
    /// shorter than the scroll position is a scroll too — the browser clamps
    /// `scrollTop` and reports it here — so nothing else has to watch for
    /// the content changing under it.
    const onScroll = () => (stuck = box.scrollTop > 0);
    onScroll();
    box.addEventListener('scroll', onScroll, { passive: true });
    return () => box.removeEventListener('scroll', onScroll);
  });

  /// Handed to the header snippet. Switching section is arriving somewhere new,
  /// and arriving halfway down it is the one way a pinned tab row can still
  /// leave you unsure where you are.
  function resetScroll() {
    sheet?.scrollTo({ top: 0 });
  }

  function requestClose() {
    if (closeDisabled) return;
    onclose();
  }
</script>

<!-- Clicking the backdrop closes the dialog, and its keyboard equivalent is
     Escape, which `closeTopmost` has handled all along — not a keydown on a
     `role="presentation"` scrim, which would put a tab stop on the dimming. -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
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
    class:catalog-dialog={variant === 'catalog'}
    class:scrollable
    role="dialog"
    aria-label={label ?? title}
    tabindex="-1"
    bind:this={sheet}
    onclick={(e) => e.stopPropagation()}
  >
    <!-- The head and whatever the dialog pins under it are one block, not two
         sticky boxes stacked by measuring the first one's height: the second
         would need a `top` in pixels that is right only for today's title font
         and close button. -->
    <div class="settings-top" class:with-header={!!header} class:stuck>
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
      {@render header?.(resetScroll)}
    </div>
    {@render children()}
    <!-- The sheet is its own scroll container, so this belongs here rather
         than in each of the seven bodies — and it stays off entirely for the
         ones that fit. It is last on purpose: the contract is "final child of
         the box that scrolls". -->
    <ScrollFade />
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
       (48px of content) and the bottom matches it, so centering stays centered.
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
       was centered in, so at full height it reached 39px from the top of the
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
       sheet scrolls or not.

       The top figure is 0 because it has moved into `.settings-top`, which is
       pinned there: a scroll container's padding scrolls away with the content,
       so 16px left here would be 16px the head loses the moment the sheet
       moves, and the title would end up against the border. Owned by the
       pinned block, it is 16px that stays. */
    padding: 0 8px 14px 18px;
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

  /* The widest sheet in the player, and the width is arithmetic rather than
     taste: the poster grid is five columns of 150px with a 16px gap, i.e. 814px
     of content, plus the sheet's own 38px of padding, gutter and border. Five
     because a poster is 2:3 and four of them leave the row looking like a
     shelf with a gap in it, while six needs a window wider than the 1280 this
     player is routinely used at. `100%` still wins in a narrow window — the
     grid is `auto-fill`, so it drops to four, three, two columns by itself. */
  .catalog-dialog {
    width: min(852px, 100%);
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

  /* ---- The pinned block ----
     The head — and, where a dialog passes one, whatever it puts under it —
     stays at the top of the sheet while the content scrolls behind. Two things
     it buys, and the second is why the settings dialog passes its tab row in
     here: the × is reachable without scrolling a long sheet back to the top,
     and the row that says which section you are reading does not leave with it.

     It spans the content box and deliberately does **not** bleed into the
     sheet's side padding: nothing is ever painted there — every child of the
     sheet is inside the content box — so a bleed would buy no coverage while
     costing the sheet's rounded corners, which the 18px on each side keeps
     this rectangle clear of. It is also the width the ScrollFade's own band
     has painted since it shipped, in the same fill, so the two edges of a
     scrolling sheet are the same shape. */
  .settings-top {
    position: sticky;
    top: 0;
    /* Over the rows it covers, and over the ScrollFade's 2 — a positioned box
       already paints above in-flow content, but a row that positions itself
       would otherwise win on source order alone. */
    z-index: 3;
    padding-top: 16px;
    /* Opaque, where the sheet's own fill is that colour at 0.97. The 3% is
       nothing under a static gradient — the ScrollFade paints exactly that
       value for the same reason — and everything under a line of text sliding
       behind the title: 3% of #e8e8ec on this ground is ~6/255 of ghosting,
       moving, which is far more visible than the ≤4/255 this costs at the
       block's own edges over the brightest video a sheet can sit on. */
    background: #101016;
    /* Both of these belong to `::after` below; declared here so the custom
       property and the transition have one home. */
    --top-cover: 12px;
  }

  .settings-top.with-header {
    --top-cover: 18px;
  }

  /* The gap under the block, covered.

     Nothing about the spacing inside a dialog changed for this, and that is
     what this pseudo-element is for. The head's `margin-bottom` (and the tab
     row's, where there is one) escapes the block — a last child's bottom margin
     collapses through a parent with no bottom padding or border — so it still
     collapses with the top margin of whatever follows, exactly as it did when
     the head was a plain child of the sheet. Give the block that space as
     padding instead and the collapse stops: `.info-section`'s 16px would go
     from a 16px gap to a 28px one, `.keys-group`'s 28 to 46, and every dialog
     would need its first child adjusted.

     What escapes is uncovered, though, and content scrolls through it. Hence a
     cover of exactly the margin that escaped: since collapsing takes the
     larger of the two, the real gap is never *smaller* than this, so the cover
     can never reach content standing at rest. In the diagnosis dialog the
     block is a flex item — an independent formatting context, so nothing
     collapses out of it — and the 12px it covers is the column's `row-gap`
     instead, which comes to the same number. */
  .settings-top::after {
    content: '';
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    height: var(--top-cover);
    background: inherit;
    /* This state only ever changes because the viewer scrolled, which is what
       earns it a transition — see the note in ScrollFade for the mirror image
       of that rule. */
    transition: box-shadow 120ms ease;
  }

  /* The edge content disappears under, drawn only once something has. A
     hairline plus a soft shadow, at the bottom of the covered band rather than
     at the block's own border edge, because that is where content actually
     stops being visible. */
  .settings-top.stuck::after {
    box-shadow:
      0 1px 0 rgba(255, 255, 255, 0.08),
      0 7px 12px -8px rgba(0, 0, 0, 0.75);
  }

  /* With a tab row there is already a line at the bottom of it, and a second
     one 18px below reads as a box rather than as an edge. The shadow alone. */
  .settings-top.with-header.stuck::after {
    box-shadow: 0 7px 12px -8px rgba(0, 0, 0, 0.75);
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
