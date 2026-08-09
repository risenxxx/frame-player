<script lang="ts">
  /// The fade at the bottom of a box that has more below the fold.
  ///
  /// Rendered as the **last child of the scroll container itself** — that is
  /// the whole contract, and it is what lets a list opt in with one line and no
  /// wrapper. `parentElement` is the container.
  ///
  /// Everything about the shape is forced by how a scroll container works, and
  /// each of the three was measured in a WKWebView harness against the real
  /// sheet geometry rather than reasoned about:
  ///
  /// **`sticky`, not `absolute`.** An absolutely positioned child of a scroll
  /// container resolves against the *content* box, whose bottom is the end of
  /// the content — so `bottom: 0` puts the fade below the last row and it
  /// scrolls up out of view with everything else, which is precisely backwards.
  /// Sticky is the only way to stay on the viewport's edge without wrapping the
  /// container in a positioned parent, and wrapping was the alternative this
  /// avoids in five places.
  ///
  /// **The paint hangs below the sticky box.** A sticky box is clamped to its
  /// containing block, so a negative offset on *it* buys nothing: measured on
  /// the settings sheet (`padding: 16px 8px 14px 18px`), `bottom: -14px` lands
  /// in exactly the same place as `bottom: 0` — 15px above the sheet's outer
  /// edge, i.e. the content box. Content scrolls *through* that 14px band, so a
  /// fade stopping there would leave a sharp strip of half-cut text under a
  /// faded one, which looks broken rather than deliberate. An absolutely
  /// positioned child is not clamped — it is only clipped, and a scroll
  /// container clips at its **padding** box — so the paint reaches the border
  /// with a negative `bottom` of the container's own padding. Measured flush
  /// (1px from the outer edge, the border) and not clipped.
  ///
  /// **The overhang and the gap are read from the container, not passed in.**
  /// A prop per call site is a number that drifts the day the sheet's padding
  /// changes; there are two facts to know and the container already states
  /// both. The gap matters because a zero-height child of a flex column still
  /// takes a `row-gap` before it — measured, 12px in the diagnosis dialog — and
  /// a fade that adds space to the box it describes is a poor trade.

  let el = $state<HTMLDivElement | undefined>();
  /// False whenever the box fits, which is what keeps the fade off entirely.
  let more = $state(false);

  /// A pixel or two of slack, for the reason the recents rail carries the same:
  /// `scrollHeight` and the sum of the fractional row heights do not always
  /// agree to the last unit, and a fade that never goes away on a box already
  /// scrolled to its end is worse than one that leaves a hair early.
  const EDGE = 2;

  $effect(() => {
    const box = el?.parentElement;
    if (!el || !box) return;

    const cs = getComputedStyle(box);
    el.style.setProperty('--fade-overhang', cs.paddingBottom || '0px');
    const gap = /flex|grid/.test(cs.display) ? Number.parseFloat(cs.rowGap) : 0;
    el.style.marginTop = Number.isFinite(gap) && gap ? `${-gap}px` : '';

    let raf = 0;
    const measure = () => {
      raf = 0;
      more = box.scrollHeight - box.scrollTop - box.clientHeight > EDGE;
    };
    const schedule = () => {
      if (!raf) raf = requestAnimationFrame(measure);
    };

    measure();
    box.addEventListener('scroll', schedule, { passive: true });
    const ro = new ResizeObserver(schedule);
    ro.observe(box);
    /// The container resizing is not enough on its own, and the case that
    /// proves it is the subtitle panel: results arrive into a box with a
    /// `max-height`, so the content grows while the container's own size never
    /// changes and no `ResizeObserver` fires. Attributes are deliberately not
    /// observed — this element's own `on` class lives inside `box`, and
    /// watching attributes would make every toggle schedule another measure.
    const mo = new MutationObserver(schedule);
    mo.observe(box, { childList: true, subtree: true, characterData: true });

    return () => {
      if (raf) cancelAnimationFrame(raf);
      box.removeEventListener('scroll', schedule);
      ro.disconnect();
      mo.disconnect();
    };
  });
</script>

<!-- Decorative, and deliberately so: "there is more below" is something a
     screen reader already knows from the scroll container itself, so
     announcing it here would be the same fact twice — and as a live region it
     would be that fact again on every scroll. -->
<div class="scrollfade" class:on={more} bind:this={el} aria-hidden="true">
  <span class="scrollfade-paint"></span>
  <svg class="scrollfade-arrow" viewBox="0 0 16 16">
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
  .scrollfade {
    position: sticky;
    bottom: 0;
    /* No height of its own: both children paint upward out of a zero-height
       box, so the fade costs the content not one pixel and a box that does not
       scroll is laid out exactly as it was before. */
    height: 0;
    /* Above the rows it covers. A positioned element already paints over
       in-flow content, but a row that positions itself would otherwise win on
       source order alone — and this is also what makes the two children below
       a stacking context of their own, so the arrow's own layering cannot
       reach anything outside. */
    z-index: 2;
    /* This is a caption on the scroll position, never a target: a click here
       is a click on the row underneath, exactly as it would be without the
       fade. The same rule the bars' gradient tails carry. */
    pointer-events: none;
    color: rgba(232, 232, 236, 0.66);
    opacity: 0;
    transition: opacity 140ms ease;
  }

  .scrollfade.on {
    opacity: 1;
  }

  /* Opacity rather than color for the three-strength reason the torrent list's
     cross avoided it: that one is on screen the whole time, where taking a
     1.4px stroke off full opacity makes WebKit re-rasterise it and the glyph
     visibly twitches. This element appears and disappears, which is what
     opacity is for — the fade in is the only state change there is. */

  .scrollfade-paint {
    position: absolute;
    right: 0;
    /* Down into the container's own bottom padding — see the note in the
       script for why this cannot be an offset on the sticky box. */
    bottom: calc(-1 * var(--fade-overhang, 0px));
    left: 0;
    /* Tall enough that the text under it goes from readable to gone across
       more than one line, which is what stops it reading as a bar. */
    height: 58px;
    /* The sheet's own fill, so the fade has no edges of its own — it is the
       surface coming back over the text rather than a shadow laid on it. Its
       last stop is where the text has to be unreadable, so the opaque end is
       reached before the very bottom rather than at it. */
    background: linear-gradient(
      to top,
      rgba(16, 16, 22, 0.97) 0%,
      rgba(16, 16, 22, 0.97) 24%,
      rgba(16, 16, 22, 0.78) 52%,
      rgba(16, 16, 22, 0) 100%
    );
  }

  /* 8px above whatever the container's visible bottom edge is — the overhang
     term is what makes that one number right for a sheet with 14px of padding
     and for a list with none. No optical nudge, unlike the rail's chevrons: a
     downward one is symmetric about the axis it is centered on. */
  .scrollfade-arrow {
    position: absolute;
    bottom: calc(8px - var(--fade-overhang, 0px));
    left: 50%;
    display: block;
    width: 15px;
    height: 15px;
    transform: translateX(-50%);
  }
</style>
