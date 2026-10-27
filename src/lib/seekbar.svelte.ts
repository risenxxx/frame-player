/**
 * What the seekbar draws over the track besides the playhead: the chapter
 * notches, the A–B band, the chapter under the hover, and the hover preview's
 * shape. Derived from the player's mirrors and handed to `SeekBar.svelte` as
 * props.
 *
 * Out of the page's script. The preview's shape is the one piece of state:
 * it only ever moves to a real value (see `lastAspect`), so it has to outlive
 * the control bar, which leaves the page on the start screen.
 */

import { chapterAt, player } from './player.svelte';
import { resume } from './resume.svelte';
import { seek } from './seek.svelte';
import { thumbs } from './thumbs.svelte';

class SeekbarMarks {
  /// Chapter boundaries as percentages of the bar. The one at zero is dropped:
  /// containers write it for the first chapter almost every time, and a notch
  /// under the very end of the track reads as a rendering artifact rather than
  /// as a boundary.
  chapterMarks = $derived(
    resume.barDuration > 0
      ? player.chapters
          .filter((c) => c.time > 0 && c.time < resume.barDuration)
          .map((c) => (c.time / resume.barDuration) * 100)
      : [],
  );
  /// The looping segment as a band on the bar. Drawn while only A is set too —
  /// a mark that appears the moment you press the key is what tells you the key
  /// did anything, and the band then grows with the playhead.
  abRegion = $derived.by(() => {
    if (player.loopA === null || resume.barDuration <= 0) return null;
    const from = Math.max(0, Math.min(player.loopA, resume.barDuration));
    const to = Math.max(from, Math.min(player.loopB ?? player.timePos, resume.barDuration));
    // mpv only loops while the position is inside the segment; past B it keeps
    // the marks but stops acting on them, and re-arms when playback returns.
    // The band says which of the two it is, or it would be claiming that
    // something is looping when nothing is.
    const armed = player.loopB === null || player.timePos < player.loopB;
    return {
      left: (from / resume.barDuration) * 100,
      width: ((to - from) / resume.barDuration) * 100,
      armed,
    };
  });

  /// The preview box's shape, and it **only ever moves to a real value**.
  ///
  /// `dwidth`/`dheight` are observed with `none`, so mpv reports them as
  /// *unavailable* whenever the VO reconfigures — which on a torrent happens
  /// every time playback stalls for pieces. Read directly, the box fell back to
  /// 16:9 for those frames and snapped back, which is the blink seen while
  /// hovering. Same rule as `media-title`, where a transient null must not erase
  /// what is already known: keep the last real one.
  lastAspect = $state('16 / 9');
  thumbAspect = $derived(this.lastAspect);

  /// Whether the hover popup has a frame to show. Read from the thumbnail
  /// module rather than from the source kind: a torrent earns previews part-way
  /// through playback, when its file finishes downloading, which no test of the
  /// URL could tell you.
  hasThumbs = $derived(thumbs.available);

  hoverChapter = $derived(
    seek.hoverTime !== null && player.hasChapters ? chapterAt(seek.hoverTime) : null,
  );
}

export const seekbar = new SeekbarMarks();

/// `lastAspect`'s standing effect. Call during a component's initialization —
/// see the `$effect` rule in docs/rules/frontend.md.
export function initThumbAspect() {
  $effect(() => {
    if (player.videoW > 0 && player.videoH > 0) {
      seekbar.lastAspect = `${player.videoW} / ${player.videoH}`;
    }
  });
}
