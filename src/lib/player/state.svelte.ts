/**
 * The mirrors themselves: `player`, the one object every importer holds, and
 * the types its fields are made of. What writes them is `mirrors.ts` (from
 * mpv's reports) and the commands that move a mirror ahead of the report.
 */

import { displayName } from '../format';
import type { OsdBox } from '../upscale-plan';
import type { DecodedPicture } from '../window-fit';

export interface Track {
  id: number;
  selected: boolean;
  label: string;
  /// Raw fields, kept alongside the label because they are what identifies
  /// "the same track" in a DIFFERENT file — see TrackDesc.
  lang: string | null;
  title: string | null;
  codec: string | null;
  forced: boolean;
  /// Added by `sub-add`/`audio-add` (or found by `sub-auto`) rather than living
  /// in the container. **This is what decides whether a track can be removed at
  /// all**: mpv's `sub-remove` works on external files only, and an embedded
  /// track could not be taken out without rewriting the video file — which is
  /// not something a player does.
  external: boolean;
  /// The external file behind it, when there is one.
  path: string | null;
  /// Channel count from the demuxer, audio tracks only. The cast prepare rung
  /// reads it to decide whether an E-AC-3 transcode needs a 5.1 fold-down
  /// (above 6 channels) — and, crucially, that a stereo source must not get
  /// one, which would be an upmix.
  channels: number | null;
}

/**
 * Repeat, as music services shape it: off → everything → this one.
 *
 * Two mpv properties, and `all` is `loop-playlist` alone — on a one-entry
 * playlist that already means "repeat this video", so the state needs no
 * special case to cover a single open file.
 *
 * The mode is OUR state, not a reading of the mirrors, and it is written to mpv
 * rather than cycled there. That looks like a violation of the rule that
 * toggles must go through `cycle`, but the rule guards against computing a
 * target from a mirror that a dropped property-change may have left stale —
 * here the target comes from a value mpv never writes to.
 */
export type LoopMode = 'off' | 'all' | 'one';

export interface Chapter {
  /// Position in `chapter-list`, which is also what the `chapter` property holds.
  index: number;
  /// Start time in seconds.
  time: number;
  /// As written in the container; null when it has no title. Kept raw rather
  /// than pre-formatted, so switching the interface language renames the
  /// nameless ones (see `chapterTitle`).
  title: string | null;
}

export type SdrColor = 'mpv' | 'system';

/**
 * One of mpv's cumulative drop counters, plus when it last moved.
 *
 * The total on its own answers nothing: a seek costs a handful of frames, so
 * every file carries a nonzero count within seconds of opening and it says
 * only that the file was seeked in. What separates that from a fault is
 * whether the number moved *just now* — which is why the increase is recorded
 * with its moment rather than merely accumulated.
 */
class DropCounter {
  count = $state(0);
  /// The last increase and when it landed, or null if nothing has been lost
  /// since this file opened.
  last = $state<{ delta: number; at: number } | null>(null);

  note(value: number | null) {
    const next = value ?? 0;
    // Going backwards is mpv starting a new file, not a correction — these
    // counters are per file. The record has to go with it, or a stall in the
    // previous episode is dated to this one and reads as having just happened.
    if (next < this.count) this.last = null;
    else if (next > this.count) this.last = { delta: next - this.count, at: Date.now() };
    this.count = next;
  }
}

class Player {
  ready = $state(false);
  initError = $state<string | null>(null);
  /// mpv refused something in the viewer's own mpv.conf, and the player is
  /// running without that file. Carries mpv's message, which names the option.
  /// Distinct from `initError`, which means there is no player at all.
  confError = $state<string | null>(null);

  /// False, matching mpv: pause is never set at init, so an idle player is
  /// unpaused. The old `true` showed a Play icon over already-running video
  /// until the first resync corrected it.
  paused = $state(false);
  timePos = $state(0);
  duration = $state(0);
  filename = $state<string | null>(null);
  filePath = $state<string | null>(null);
  mediaTitle = $state<string | null>(null);
  volume = $state(100);
  muted = $state(false);
  speed = $state(1);
  subDelay = $state(0);
  audioDelay = $state(0);
  subSpeed = $state(1);
  loopMode = $state<LoopMode>('off');
  eofReached = $state(false);
  playlistPos = $state(0);
  playlistCount = $state(0);

  /// dwidth/dheight — the video size including aspect.
  videoW = $state(0);
  videoH = $state(0);
  /// `video-out-params/rotate` — the turn the VO applies on top of that size,
  /// in degrees. The shape on screen is `pictureShape` of the three.
  voRotate = $state(0);

  /// vo-configured — the VO exists and is painting (its field is black until
  /// the first frame). The page latches its dark backdrop off on this.
  voConfigured = $state(false);

  audioTracks = $state<Track[]>([]);
  subTracks = $state<Track[]>([]);

  /// Fill of the network cache, 0..100, or null when there is no cache — which
  /// is the normal state for a local file.
  cacheBuffering = $state<number | null>(null);
  /// Playback is stopped waiting for the network, not by the viewer.
  stalled = $state(false);

  /// Frames thrown away by the video output because they arrived too late to
  /// show, and by the decoder because it could not keep up. Which of the two
  /// moves is the difference between "the picture cannot be produced fast
  /// enough" and "it was produced and could not be put on screen", and nothing
  /// else in the player can tell those apart.
  dropVo = new DropCounter();
  dropDecoder = new DropCounter();

  /// A–B loop points in seconds, or null when unset.
  loopA = $state<number | null>(null);
  loopB = $state<number | null>(null);

  get hasAbLoop(): boolean {
    return this.loopA !== null && this.loopB !== null;
  }

  /// Volume normalization (a labeled `af` filter). Ours to remember: mpv
  /// answers `af` with an escaped form that cannot be compared against.
  normalize = $state(false);

  /// How an SDR picture's tone curve is drawn on macOS: mpv's own reading of
  /// BT.709, or the one QuickTime uses. Ours to
  /// remember, like `normalize` — see `applySdrColor`.
  sdrColor = $state<SdrColor>('mpv');
  /// The current video's transfer function before any filter
  /// (`video-params/gamma`), mpv's guess included; null with no video.
  sourceTransfer = $state<string | null>(null);
  /// `video-params/{w,h,dw,dh,rotate}` — the picture the decoder produces,
  /// ahead of the VO; zeros with no frame decoded. What the window's shape is
  /// made of (`decodedShape`), together with `videoCrop`.
  decoded = $state<DecodedPicture>({ w: 0, h: 0, dw: 0, dh: 0, rotate: 0 });
  /// `osd-dimensions/{w,h,ml,mr,mt,mb}` — the VO's surface in device pixels
  /// and the margins around the picture in it (negative when it overflows).
  osd = $state<OsdBox>({ w: 0, h: 0, ml: 0, mr: 0, mt: 0, mb: 0 });

  /// Picture geometry, per file. `aspectOverride` is negative for "from the
  /// container" — mpv's default is -2, and any negative value means auto.
  videoRotate = $state(0);
  aspectOverride = $state(-2);
  panscan = $state(0);
  /// `video-crop` as mpv reports it: `WxH+X+Y`, or empty for none.
  videoCrop = $state('');

  chapters = $state<Chapter[]>([]);
  /// Index of the chapter being played, or -1 when the file has none.
  chapterIndex = $state(-1);

  /// Path of the user's mpv.conf, for the settings dialog footer.
  mpvConfPath = $state<string | null>(null);

  get hasFile(): boolean {
    return this.filename !== null;
  }

  /// A single chapter spanning the whole file is what containers write when
  /// they have nothing to say — there is nothing to navigate there.
  get hasChapters(): boolean {
    return this.chapters.length > 1;
  }

  get displayTitle(): string {
    // `media-title` is the container's own, or what yt-dlp resolved for a link.
    // Failing that the file name — tidied, because for a shared YouTube link
    // mpv's `filename` is the video id with its tracking query attached.
    if (this.mediaTitle) return this.mediaTitle;
    return this.filename ? displayName(this.filename) : 'Frame Player';
  }
}

export const player = new Player();
