<script lang="ts">
  /// The part of an OSC panel that scrolls, and the statement that it does.
  ///
  /// Every panel over the control bar — the queue, the chapters, the two track
  /// menus, the cast picker and the overflow panel — wraps its rows in this.
  /// The panel itself (`.menu`, in app.css) keeps the shape, the fill and the
  /// border; what is in here is the list, its scrollbar, and the two edges that
  /// say the list goes on past them.
  ///
  /// **A mask, where the dialogs paint.** `ScrollFade` lays the sheet's own
  /// fill back over the text, which is right for a sheet: it stands on a dimmed
  /// backdrop, and the 3% of picture the paint takes away there is at most
  /// 4/255. A panel is 0.94 over the picture itself, undimmed, so the same
  /// paint removes 6% of up to 255 — measured over white, the band came out at
  /// 16 against the panel's 30, a dark rectangle with its own left and right
  /// edges, stopping short of the scrollbar. That is the darker band `MenuBack`
  /// records twice, and no tint fixes it: anything painted over a see-through
  /// surface adds to its alpha. A mask is the one thing that takes the rows
  /// away and leaves the surface under them as it was — and a mask covers
  /// everything the masked element paints, its own background included, which
  /// is why the list had to become a box of its own inside the panel rather
  /// than the panel.
  ///
  /// **The scrollbar is counted in the panel's width from the first layout.**
  /// A panel is as wide as its longest row. WebKit leaves an `overflow: auto`
  /// bar out of that width on the layout that discovers the bar is needed, and
  /// counts it on every layout after — so a panel opened 10px too narrow, its
  /// longest names cut to an ellipsis by the bar that had just taken their
  /// room, and grew to fit them at the first thing that dirtied its layout.
  /// That was the pointer arriving: the row's hover styles are enough. Measured
  /// in a WKWebView harness against the built stylesheet — 318px and four names
  /// cut on a fresh layout, 328px and none after adding the hover declarations
  /// to one row. Blink counts the bar from the start (327px in every state), so
  /// this was only ever wrong on macOS. `overflow-y: scroll` is counted by both
  /// on the first layout (measured: 328px fresh), so the list is given that for
  /// as long as it has something to scroll, and `auto` back when it fits —
  /// where a permanent `scroll` would hold 10px on one side of a panel whose
  /// rows are inset 6px on both.
  import type { Snippet } from 'svelte';

  interface Props {
    children: Snippet;
  }

  let { children }: Props = $props();

  let el = $state<HTMLDivElement | undefined>();
  /// Something is past the bottom edge. False whenever the list fits, which is
  /// what keeps both edges off entirely.
  let more = $state(false);
  /// Something is past the top one. The chapter list and the queue open
  /// scrolled to the entry being played, so this is as often true on arrival
  /// as `more` is.
  let above = $state(false);
  /// Suppresses the transition for one update — see `jump` below.
  let instant = $state(false);

  /// A pixel or two of slack, for the reason `ScrollFade` carries the same:
  /// `scrollHeight` and the sum of the fractional row heights do not always
  /// agree to the last unit, and an edge that never goes away on a list
  /// already scrolled to its end is worse than one that leaves a hair early.
  const EDGE = 2;

  /// How much of each end fades: a row and a third.
  const FADE = 42;

  $effect(() => {
    const box = el;
    const frame = box?.parentElement;
    const panel = frame?.parentElement;
    if (!box || !frame || !panel) return;

    let raf = 0;
    let clear = 0;
    /// **Only a scroll earns the transition** — the rule `ScrollFade` records,
    /// for the reason it records: an edge that grows in answers the viewer's
    /// own gesture, and one that creeps in a beat after the panel opened or
    /// its content changed reads as a flicker. Set before the measure rather
    /// than inside it, so the flag and the two states reach the DOM in one
    /// flush.
    let jump = true;
    const measure = () => {
      raf = 0;
      /// Decided on the local, never on `instant`: reading back a `$state`
      /// this effect writes would subscribe the effect to itself (the
      /// ScrollFade lesson, 95 class writes in 1.5s on an idle box).
      const now = jump;
      jump = false;
      /// First, because it decides the panel's width and therefore everything
      /// read below. No slack here: the question is whether a bar exists, and
      /// one pixel of content too many is enough to raise it.
      box.style.overflowY = box.scrollHeight > box.clientHeight ? 'scroll' : '';
      /// The bar's width is kept out of the mask and out of the chevrons'
      /// centering, so both need to know it — read off the box rather than
      /// copied from the scrollbar skin in app.css.
      frame.style.setProperty('--menu-bar', `${box.offsetWidth - box.clientWidth}px`);
      /// **The edges are for a panel at its full height.** Once the window is
      /// what limits the panel — a short window, the mini player — the list is
      /// visibly cut off by the window itself, which says "scroll" without any
      /// help, and two 42px edges would be most of what is left of it. Both
      /// numbers are the panel's own (`--menu-cap` and the `max-height` built
      /// from it, in app.css), read here rather than repeated. An answer that
      /// cannot be read is a yes: a panel with no cap has all the room it
      /// wants.
      const style = getComputedStyle(panel);
      const cap = Number.parseFloat(style.getPropertyValue('--menu-cap'));
      const room = Number.parseFloat(style.maxHeight);
      const edges = !(room < cap);
      /// Written straight to the element rather than through a class: the
      /// panel that owns this list scrolls to its entry in the same tick, and
      /// `scroll-padding` has to be in force by then whatever order the two
      /// effects' writes are flushed in.
      frame.style.setProperty('--menu-fade', edges ? `${FADE}px` : '0px');
      instant = now;
      more = edges && box.scrollHeight - box.scrollTop - box.clientHeight > EDGE;
      above = edges && box.scrollTop > EDGE;
      /// Given back on the next frame, by which time the value is committed —
      /// restoring a transition never animates anything retroactively.
      if (now) {
        cancelAnimationFrame(clear);
        clear = requestAnimationFrame(() => (instant = false));
      }
    };
    const schedule = (animated: boolean) => {
      if (!animated) jump = true;
      if (!raf) raf = requestAnimationFrame(measure);
    };
    /// A scroll is the viewer's own only once the panel has been on screen.
    /// The chapter list and the queue scroll themselves to the entry being
    /// played as they open, and that arrives here as a scroll like any other.
    /// Two frames rather than one, because which side of the first frame's
    /// callbacks a scroll event lands on is the engine's business.
    let settled = false;
    let settle = requestAnimationFrame(() => {
      settle = requestAnimationFrame(() => (settled = true));
    });
    const onScroll = () => schedule(settled);

    measure();
    box.addEventListener('scroll', onScroll, { passive: true });
    const ro = new ResizeObserver(() => schedule(false));
    ro.observe(box);
    /// (The window crossing the height the edges need is a resize of the box
    /// too — below it the box is as tall as the window lets it be.)
    /// The box resizing is not enough on its own: the queue's names arrive
    /// after the panel has opened and an entry can be removed from it, and
    /// under a `max-height` neither changes the size of the box. Attributes
    /// are not observed — the rows' own classes and styles change on every
    /// hover and every drag.
    const mo = new MutationObserver(() => schedule(false));
    mo.observe(box, { childList: true, subtree: true, characterData: true });

    return () => {
      if (raf) cancelAnimationFrame(raf);
      if (clear) cancelAnimationFrame(clear);
      cancelAnimationFrame(settle);
      box.removeEventListener('scroll', onScroll);
      ro.disconnect();
      mo.disconnect();
    };
  });
</script>

<div class="menu-frame">
  <div class="menu-body scrollable" class:above class:more class:instant bind:this={el}>
    {@render children()}
  </div>
  <!-- Outside the list, because everything inside it is under the mask — at
       the two edges most of all. Decorative for the reason `ScrollFade` gives:
       a screen reader already has "there is more" from the scroll container
       itself. -->
  <svg class="menu-edge up" class:on={above} class:instant viewBox="0 0 16 16" aria-hidden="true">
    <path
      d="M3.5 10 8 5.5 12.5 10"
      fill="none"
      stroke="currentColor"
      stroke-width="1.8"
      stroke-linecap="round"
      stroke-linejoin="round"
    />
  </svg>
  <svg class="menu-edge down" class:on={more} class:instant viewBox="0 0 16 16" aria-hidden="true">
    <path
      d="M3.5 6 8 10.5 12.5 6"
      fill="none"
      stroke="currentColor"
      stroke-width="1.8"
      stroke-linecap="round"
      stroke-linejoin="round"
    />
  </svg>
</div>

<style>
  /* Registered, because an unregistered custom property has no type and a
     transition between two of its values is a flip at the halfway point. Where
     `@property` is not known the edges still work; they arrive at once. */
  @property --menu-fade-top {
    syntax: '<length>';
    inherits: false;
    initial-value: 0px;
  }

  @property --menu-fade-bottom {
    syntax: '<length>';
    inherits: false;
    initial-value: 0px;
  }

  /* The panel's one flex item: it takes what is left of the panel's
     `max-height`, and the list inside it scrolls. Positioned, so the chevrons
     have the list's own box to stand against. */
  .menu-frame {
    position: relative;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .menu-body {
    /* Without this the list is as tall as its rows and the panel's cap is
       never passed down to it: a flex item does not shrink below its content
       unless told to. */
    min-height: 0;
    overflow-y: auto;
    /* Same reason as .ctxmenu: sideways is never a direction this scrolls, and
       leaving it `visible` next to a scrolling axis computes it to `auto`. */
    overflow-x: hidden;
    /* The panel's own inset, moved in here with the rows it is around. A scroll
       container's padding scrolls with its content, which is what it always
       did — the first row still starts 6px under the panel's edge and the last
       one still ends 6px over it. */
    padding: 6px;
    /* The list scrolls itself to the entry being played (`nearest`), and
       without this the entry lands on the very edge — under the fade, which is
       the one row that must not be. `--menu-fade` is 0 where there are no
       edges, and the entry goes back to the edge it always went to. */
    scroll-padding-block: var(--menu-fade, 0px);

    --menu-fade-top: 0px;
    --menu-fade-bottom: 0px;
    /* Two layers, and the union of them is what shows. The first keeps the
       scrollbar whole: a thumb near either end would otherwise fade with the
       rows beside it, and a bar that dims as it moves reads as a bar going
       away. The second is the list, on `ScrollFade`'s own curve turned inside
       out — 0.97 of paint there is 0.03 of row here: gone for the first
       quarter of the band, so that nothing is left readable under the chevron,
       and whole by `--menu-fade`.
       With both lengths at 0 every stop of an end lands on the edge itself and
       the list is whole — the state of a panel that fits. */
    -webkit-mask-image:
      linear-gradient(#000, #000),
      linear-gradient(
        to bottom,
        transparent 0,
        transparent calc(var(--menu-fade-top) * 0.24),
        rgba(0, 0, 0, 0.22) calc(var(--menu-fade-top) * 0.52),
        #000 var(--menu-fade-top),
        #000 calc(100% - var(--menu-fade-bottom)),
        rgba(0, 0, 0, 0.22) calc(100% - var(--menu-fade-bottom) * 0.52),
        transparent calc(100% - var(--menu-fade-bottom) * 0.24),
        transparent 100%
      );
    mask-image:
      linear-gradient(#000, #000),
      linear-gradient(
        to bottom,
        transparent 0,
        transparent calc(var(--menu-fade-top) * 0.24),
        rgba(0, 0, 0, 0.22) calc(var(--menu-fade-top) * 0.52),
        #000 var(--menu-fade-top),
        #000 calc(100% - var(--menu-fade-bottom)),
        rgba(0, 0, 0, 0.22) calc(100% - var(--menu-fade-bottom) * 0.52),
        transparent calc(100% - var(--menu-fade-bottom) * 0.24),
        transparent 100%
      );
    -webkit-mask-size:
      var(--menu-bar, 0px) 100%,
      100% 100%;
    mask-size:
      var(--menu-bar, 0px) 100%,
      100% 100%;
    -webkit-mask-position:
      right top,
      left top;
    mask-position:
      right top,
      left top;
    -webkit-mask-repeat: no-repeat;
    mask-repeat: no-repeat;
    transition:
      --menu-fade-top 140ms ease,
      --menu-fade-bottom 140ms ease;
  }

  .menu-body.above {
    --menu-fade-top: var(--menu-fade, 0px);
  }

  .menu-body.more {
    --menu-fade-bottom: var(--menu-fade, 0px);
  }

  /* Written to win rather than left to source order — two classes against
     one. */
  .menu-body.instant {
    transition: none;
  }

  /* `ScrollFade`'s chevron: the same glyph, the same 8px from the edge it
     points past. Centered on the rows rather than on the panel, which differ by
     half a scrollbar. Opacity, because this one appears and disappears — the
     three-strength rule about small glyphs is for controls that stay. */
  .menu-edge {
    position: absolute;
    left: calc((100% - var(--menu-bar, 0px)) / 2);
    width: 15px;
    height: 15px;
    transform: translateX(-50%);
    color: rgba(232, 232, 236, 0.66);
    /* A caption on the scroll position, never a target: a click here is a
       click on the row underneath. */
    pointer-events: none;
    opacity: 0;
    transition: opacity 140ms ease;
  }

  .menu-edge.up {
    top: 8px;
  }

  .menu-edge.down {
    bottom: 8px;
  }

  .menu-edge.on {
    opacity: 1;
  }

  .menu-edge.instant {
    transition: none;
  }
</style>
