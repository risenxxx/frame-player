/**
 * The connection to a room: the socket, the clock offset, and the one function
 * everything else in the player calls to say that the timeline moved.
 *
 * **This module imports nothing from the player, and that is load-bearing.**
 * `playback.svelte.ts` and `seek.svelte.ts` publish into it, and both of those
 * already sit high in the import graph — `playback` reaches `cast`, `seek`
 * reaches `thumbs`. If the bus reached back for `player` or `cast` to *apply*
 * what arrives, that would be a cycle, and a cycle here costs no error and no
 * warning: the bundler resolves it and leaves a module-evaluation order nobody
 * chose (`npm run check-imports` is what says so out loud). So the bus is cut in
 * two along that line — publishing goes *down* into this leaf, and applying goes
 * *up*, in `apply.svelte.ts`, which is above everything and is reached only
 * through the callbacks registered here.
 *
 * The one thing worth knowing about the shape of the wire: what travels is the
 * **timeline after a change, never the change**. A replayed action ("seek back
 * five") is not idempotent, does not survive a dropped message, and cannot
 * answer the only question a late-joining or lagging viewer has, which is where
 * they should be *now*. A snapshot answers all three.
 */

import {
  CODE_LENGTH,
  PROTOCOL_VERSION,
  emptyTimeline,
  isErrorCode,
  normalizeCode,
  positionAt,
  shouldApply,
  type ClientMsg,
  type ContentRef,
  type ErrorCode,
  type Member,
  type RoomRules,
  type ServerMsg,
  type SharedTracks,
  type Timeline,
  type TrackKind,
} from './protocol';
import {
  estimateOffset,
  offsetUncertainty,
  pushSample,
  relayClock,
  sampleOf,
  type Sample,
} from './clock';

/**
 * The relay this build points at by default.
 *
 * A setting overrides it, and an **empty setting means this**, which is why
 * `setRelayUrl('')` removes the key rather than storing a blank: clearing the
 * field in the settings sheet has to restore the default rather than turn the
 * feature off. `server/` is the whole of what has to be deployed to run your own.
 *
 * Not a secret and not an identity: the relay learns a room code, a display
 * name and what the room is watching (unless it is hidden), and it holds none of
 * it for longer than the evening. What it never sees is the film.
 */
export const DEFAULT_RELAY = 'relay.frameplayer.app';

const RELAY_KEY = 'frameplayer.relay';
const NAME_KEY = 'frameplayer.syncName';

/// Reconnect backoff. Short enough that a Wi-Fi blip is invisible, long enough
/// that a relay that is down is not hammered by every player that ever joined.
const RETRY_MS = [500, 1000, 2000, 4000, 8000, 15000] as const;

/// Pings are frequent while the offset settles and rare afterwards — it is an
/// estimate of a clock, not a heartbeat (the relay's own ping is that).
const PING_FAST_MS = 500;
const PING_FAST_COUNT = 8;
/**
 * Ten seconds, not thirty.
 *
 * The offset is no longer only used to place the playhead — it now *sets the
 * correction band* (`deadbandFor`), so a stale uncertainty makes the room
 * needlessly loose. A ping is a few dozen bytes and this is six a minute; the
 * window of eight samples then refreshes about once a minute, which is fast
 * enough to follow a laptop moving between networks and slow enough to be
 * invisible.
 */
const PING_SLOW_MS = 10_000;

/**
 * How long after our own publish the timeline is treated as ours rather than the
 * room's.
 *
 * Between sending a seek and hearing it back, the authoritative timeline still
 * describes where the film *was* — so drift correction, which is the thing
 * watching for exactly that difference, would haul playback back to the old
 * position and then the echo would send it forward again. The window is one
 * round trip plus room to spare.
 */
const PUBLISH_SETTLE_MS = 1500;

/** A refusal worth showing, or a socket that will not open. */
export type SyncError = ErrorCode | 'unreachable' | 'no_relay';

export type Phase = 'off' | 'connecting' | 'joined';

export interface RoomEvent {
  kind: 'joined' | 'left' | 'host';
  name: string;
  /// `performance.now()` when it happened — the chip fades it out on age.
  at: number;
}

class Wire {
  phase = $state<Phase>('off');
  /// The room code, once the relay has answered. Never what was typed.
  room = $state('');
  me = $state('');
  host = $state('');
  hostOnly = $state(false);
  /// The room's own rules about tracks. Defaults mirror the relay's, so a
  /// panel opened before the handshake lands does not show the wrong answer.
  shareAudio = $state(true);
  shareSubs = $state(false);
  members = $state<Member[]>([]);
  /// Ids of members the room is waiting for. Ours included, when it is us.
  waiting = $state<string[]>([]);
  timeline = $state<Timeline>(emptyTimeline());
  /// The last refusal. Cleared by the next successful action.
  error = $state<SyncError | null>(null);
  /// How far our clock estimate could be out, in milliseconds. Shown to the
  /// viewer *and* used: it sets how tightly the room is held (`deadbandFor`).
  uncertainty = $state(Infinity);
  /**
   * The last thing that happened to the room's membership, for the indicator.
   *
   * Somebody arriving or leaving is the one change a viewer cannot infer from
   * the film: playback simply carries on, or stops, with nothing to say who did
   * it. The chip announces it for a few seconds and then goes back to the
   * roster — a notification rather than a state, which is why it carries the
   * time it happened rather than a flag somebody has to clear.
   */
  event = $state<RoomEvent | null>(null);

  /// In a room at all — what every publisher checks before doing anything.
  get on(): boolean {
    return this.phase === 'joined';
  }

  /// Somebody is holding the room up.
  get waitingFor(): Member[] {
    return this.members.filter((m) => this.waiting.includes(m.id));
  }

  /// This viewer may move the timeline.
  get mayDrive(): boolean {
    return this.on && (!this.hostOnly || this.me === this.host);
  }

  get isHost(): boolean {
    return this.on && this.me === this.host;
  }

  /// Whether this kind of track choice is the room's, in both directions —
  /// published when made here, and taken when made elsewhere. One answer, so
  /// two members cannot disagree about what the room does.
  shares(kind: TrackKind): boolean {
    return kind === 'audio' ? this.shareAudio : this.shareSubs;
  }
}

export const wire = new Wire();

// ---- settings ---------------------------------------------------------------
//
// The two that are genuinely this machine's: where to connect, and what to call
// yourself. Everything else about a shared session is the *room's* and arrives
// from the relay — see `RoomRules`.

export function relayUrl(): string {
  try {
    return localStorage.getItem(RELAY_KEY) ?? DEFAULT_RELAY;
  } catch {
    return DEFAULT_RELAY;
  }
}

export function setRelayUrl(url: string) {
  try {
    const clean = url.trim().replace(/\/+$/, '');
    if (clean) localStorage.setItem(RELAY_KEY, clean);
    else localStorage.removeItem(RELAY_KEY);
  } catch {
    // not critical: the address simply will not survive a restart
  }
}

export function displayName(): string {
  try {
    return localStorage.getItem(NAME_KEY) ?? '';
  } catch {
    return '';
  }
}

export function setDisplayName(name: string) {
  try {
    const clean = name.trim().slice(0, 32);
    if (clean) localStorage.setItem(NAME_KEY, clean);
    else localStorage.removeItem(NAME_KEY);
  } catch {
    // not critical
  }
}

/**
 * The websocket address for an `https://relay.example` style setting.
 *
 * Plain `ws://` is refused unless it is loopback, and that is not fussiness: the
 * relay carries what everyone in the room is watching, so a cleartext connection
 * hands it to every network between here and there. Loopback is exempt because
 * that is the development case and it never leaves the machine.
 */
export function socketUrl(setting: string): string | null {
  // Trimmed only. Stripping trailing slashes *before* the scheme is detected
  // turns `http://` into `http:`, which no longer looks like it has a scheme —
  // so it was prefixed again and parsed as a host literally named `http`. The
  // path is tidied below, after there is a URL to tidy.
  const raw = setting.trim();
  if (!raw) return null;
  let url: URL;
  try {
    url = new URL(/^[a-z]+:\/\//i.test(raw) ? raw : `https://${raw}`);
  } catch {
    return null;
  }
  if (!url.hostname) return null;
  const loopback = url.hostname === '127.0.0.1' || url.hostname === 'localhost' || url.hostname === '::1';
  if (url.protocol === 'http:' || url.protocol === 'ws:') {
    if (!loopback) return null;
    url.protocol = 'ws:';
  } else if (url.protocol === 'https:' || url.protocol === 'wss:') {
    url.protocol = 'wss:';
  } else {
    return null;
  }
  return `${url.origin}${url.pathname.replace(/\/$/, '')}/ws`;
}

// ---- subscribers ------------------------------------------------------------
//
// `apply.svelte.ts` registers here. Callbacks rather than the module being
// imported, because that is the whole reason this file is a leaf.

type TimelineHandler = (timeline: Timeline, fromSelf: boolean) => void;
type RoomHandler = () => void;

let onTimelineCb: TimelineHandler = () => {};
let onRoomCb: RoomHandler = () => {};

export function initWire(handlers: { timeline?: TimelineHandler; room?: RoomHandler }) {
  if (handlers.timeline) onTimelineCb = handlers.timeline;
  if (handlers.room) onRoomCb = handlers.room;
}

// ---- the connection ---------------------------------------------------------

let socket: WebSocket | null = null;
/// The room we mean to be in. Kept across a drop, which is what makes a
/// reconnect land in the same room rather than opening a new one.
let wantRoom: string | null = null;
let wantJoin = false;
let attempt = 0;
let retryTimer: ReturnType<typeof setTimeout> | undefined;
let pingTimer: ReturnType<typeof setTimeout> | undefined;
let pingsSent = 0;

let samples: Sample[] = [];
/// Relay clock minus ours, milliseconds. Seeded from the handshake so there is a
/// usable value before the first round trip completes.
let offset = 0;
let publishedUntil = 0;

/// What we last told the relay about ourselves, so a reconnect can restate it
/// without waiting for the next thing to change.
///
/// **False until something says otherwise**, and that default is load-bearing.
/// The relay counts a joining member as not-ready — the newcomer has a file to
/// open, and the others should wait for them — so a client whose first message
/// says "ready" undoes that freeze before it has opened anything. Measured
/// against a live relay: with `true` here the room froze at 4.70 s and thawed in
/// the same second, which is the freeze existing in the logs and nowhere else.
/// Agreeing with the relay's own assumption costs nothing, and the readiness
/// effect in `apply.svelte.ts` flips it the moment there is something to play.
let lastReady = false;
let lastReason = '';
let lastPublished: Timeline | null = null;

/// The relay's clock, as well as we can tell. Whole milliseconds — see
/// `relayClock`, which is where the reason lives and where it is tested.
export function serverNow(): number {
  return relayClock(Date.now(), offset);
}

/// Where the room says playback should be, right now.
export function targetPosition(): number {
  return positionAt(wire.timeline, serverNow());
}

/**
 * Our own last change is still in flight, so the authoritative timeline is
 * behind what this viewer already did. Drift correction stands down while this
 * is true — otherwise it would drag playback back to the old position and the
 * echo would send it forward again, which reads as the film twitching.
 */
export function publishSettling(): boolean {
  return performance.now() < publishedUntil;
}

/**
 * Join a room, or create one when `code` is null.
 *
 * Safe to call while already connected: it leaves first, because "join this
 * other room" is a thing viewers do from a link while sitting in a room.
 */
export function joinRoom(code: string | null) {
  const wanted = code === null ? '' : normalizeCode(code);
  if (code !== null && wanted.length !== CODE_LENGTH) {
    wire.error = 'bad_message';
    return;
  }
  leaveRoom({ quiet: true });
  const url = socketUrl(relayUrl());
  if (!url) {
    wire.error = 'no_relay';
    return;
  }
  wantRoom = wanted;
  wantJoin = true;
  attempt = 0;
  wire.error = null;
  wire.phase = 'connecting';
  open(url);
}

export function leaveRoom(opts: { quiet?: boolean } = {}) {
  wantJoin = false;
  wantRoom = null;
  clearTimeout(retryTimer);
  clearTimeout(pingTimer);
  const sock = socket;
  socket = null;
  if (sock && sock.readyState === WebSocket.OPEN) {
    try {
      sock.send(JSON.stringify({ t: 'bye' } satisfies ClientMsg));
    } catch {
      // going away regardless
    }
  }
  sock?.close();
  wire.phase = 'off';
  wire.room = '';
  wire.me = '';
  wire.host = '';
  wire.hostOnly = false;
  wire.shareAudio = true;
  wire.shareSubs = false;
  wire.members = [];
  wire.waiting = [];
  wire.timeline = emptyTimeline();
  wire.uncertainty = Infinity;
  wire.event = null;
  samples = [];
  offset = 0;
  publishedUntil = 0;
  lastPublished = null;
  if (!opts.quiet) wire.error = null;
  onRoomCb();
}

function open(url: string) {
  let sock: WebSocket;
  try {
    sock = new WebSocket(url);
  } catch {
    fail('unreachable');
    return;
  }
  socket = sock;

  sock.onopen = () => {
    if (socket !== sock) return;
    send({
      t: 'hello',
      ver: PROTOCOL_VERSION,
      room: wantRoom ?? '',
      name: displayName(),
    });
  };

  sock.onmessage = (ev) => {
    if (socket !== sock) return;
    let msg: ServerMsg;
    try {
      msg = JSON.parse(String(ev.data)) as ServerMsg;
    } catch {
      return;
    }
    handle(msg);
  };

  sock.onclose = () => {
    if (socket !== sock) return;
    socket = null;
    clearTimeout(pingTimer);
    // A refusal has already set the error and cleared `wantJoin`; anything else
    // is the network, and the network comes back.
    if (!wantJoin) {
      wire.phase = 'off';
      return;
    }
    wire.phase = 'connecting';
    retry(url);
  };

  sock.onerror = () => {
    // `onclose` always follows, and it is where the retry lives — doing it here
    // too would double every backoff step.
  };
}

function retry(url: string) {
  clearTimeout(retryTimer);
  const wait = RETRY_MS[Math.min(attempt, RETRY_MS.length - 1)];
  attempt += 1;
  // Only after the ladder has been walked once: a player that says
  // "unreachable" on the first blip is wrong more often than it is right.
  if (attempt > RETRY_MS.length) wire.error = 'unreachable';
  retryTimer = setTimeout(() => {
    if (wantJoin) open(url);
  }, wait);
}

function fail(code: SyncError) {
  wire.error = code;
  wantJoin = false;
  wantRoom = null;
  wire.phase = 'off';
}

function send(msg: ClientMsg) {
  const sock = socket;
  if (!sock || sock.readyState !== WebSocket.OPEN) return;
  try {
    sock.send(JSON.stringify(msg));
  } catch {
    // the close handler will deal with it
  }
}

function handle(msg: ServerMsg) {
  switch (msg.t) {
    case 'welcome': {
      attempt = 0;
      wire.phase = 'joined';
      wire.room = msg.room;
      wire.me = msg.me;
      wire.host = msg.host;
      applyRules(msg);
      wire.members = msg.members;
      wire.waiting = msg.waiting;
      wire.error = null;
      // A usable offset immediately, replaced by the estimator's within a
      // second or two. Without it the first projected position after joining is
      // out by the whole clock difference between the two machines.
      offset = msg.now - Date.now();
      samples = [];
      wire.uncertainty = Infinity;
      pingsSent = 0;
      schedulePing();
      // Authoritative: a handshake describes the room as it is now, and a room
      // that has been created afresh counts revisions from zero again.
      applyTimeline(msg.timeline, true);
      // Restate what the relay cannot know after a reconnect: whether we are
      // ready, and what we were playing. Without the second, a host who dropped
      // and came back would find the room still pointing at the old file.
      send({ t: 'ready', ready: lastReady, reason: lastReason });
      if (lastPublished && msg.timeline.rev === 0) publish(lastPublished);
      onRoomCb();
      break;
    }
    case 'timeline':
      applyTimeline({
        content: msg.content,
        tracks: msg.tracks,
        paused: msg.paused,
        position: msg.position,
        speed: msg.speed,
        at: msg.at,
        rev: msg.rev,
        by: msg.by,
      });
      break;
    case 'members':
      noteMembership(msg.members, msg.host);
      wire.members = msg.members;
      wire.host = msg.host;
      applyRules(msg);
      wire.waiting = msg.waiting;
      onRoomCb();
      break;
    case 'pong': {
      const sample = sampleOf(msg.c, msg.s, Date.now());
      if (!sample) break;
      samples = pushSample(samples, sample);
      offset = estimateOffset(samples);
      wire.uncertainty = offsetUncertainty(samples);
      break;
    }
    case 'error': {
      const code: SyncError = isErrorCode(msg.code) ? msg.code : 'bad_message';
      // Always logged, with whatever the relay said. This is the one class of
      // failure a viewer cannot diagnose and a developer cannot reproduce from
      // a description — and it cost a real session: a fractional `at` made every
      // publish undecodable, and all the player could say was that the server
      // had not understood something.
      console.warn(`[sync] relay refused: ${msg.code}`, msg.message);
      // **A refusal while in a room is about one message, never about the
      // session.** It used to end it: any error at all called `fail`, so a
      // single malformed publish threw the viewer back to the join dialog
      // mid-film, with an error under the code field about a code that had
      // been perfectly fine. Before we are in, an error *is* the answer to the
      // attempt; after, whether the session survives is the socket's to decide.
      if (wire.on) {
        wire.error = code;
        break;
      }
      fail(code);
      break;
    }
  }
}

/**
 * Take a timeline from the relay. See `shouldApply` for what is being defended
 * against and why the comparison is the shape it is.
 */
/**
 * Notice who arrived and who left.
 *
 * Diffed here rather than announced by the relay, because the relay would have
 * to say it *to* somebody: the member list is one broadcast to everybody, and a
 * per-recipient "Anna joined" message would be a second delivery path for
 * information already in the first.
 *
 * Only ever one event, the newest. Three people arriving at once is a moment,
 * not three notifications — and the roster underneath already says who is here.
 */
function noteMembership(next: Member[], host: string) {
  const before = wire.members;
  // The first list after joining is not a stream of arrivals: everybody in it
  // was already here.
  if (before.length === 0) return;
  const had = new Set(before.map((m) => m.id));
  const has = new Set(next.map((m) => m.id));

  const arrived = next.find((m) => !had.has(m.id) && m.id !== wire.me);
  if (arrived) {
    wire.event = { kind: 'joined', name: arrived.name, at: performance.now() };
    return;
  }
  const gone = before.find((m) => !has.has(m.id));
  if (gone) {
    wire.event = { kind: 'left', name: gone.name, at: performance.now() };
    return;
  }
  // Nobody came or went, so a changed host is a handover — which matters
  // enough to say when only the host may drive.
  if (host !== wire.host && host && wire.host) {
    const who = next.find((m) => m.id === host);
    if (who) wire.event = { kind: 'host', name: who.name, at: performance.now() };
  }
}

function applyTimeline(next: Timeline, authoritative = false) {
  if (!shouldApply(wire.timeline, next, authoritative)) return;
  const fromSelf = next.by === wire.me;
  // Our own change coming back is what ends the settling window: from here the
  // authoritative timeline and this viewer's player agree.
  if (fromSelf) publishedUntil = 0;
  wire.timeline = next;
  onTimelineCb(next, fromSelf);
}

// ---- saying things ----------------------------------------------------------

/** What a publisher supplies. `at`, `rev` and `by` are not the caller's. */
export type TimelinePatch = Pick<Timeline, 'content' | 'tracks' | 'paused' | 'position' | 'speed'>;

/**
 * Say that the timeline moved.
 *
 * Called from `playback`'s verbs and from the two points in `seek` where a
 * gesture *ends* — never from a preview. That is the same release-only rule the
 * cast seekbar keeps, for a related reason: a drag is a stream of positions, and
 * every one of them would be a seek on somebody else's machine.
 */
export function publish(patch: TimelinePatch) {
  if (!wire.on) return;
  if (!wire.mayDrive) {
    wire.error = 'not_allowed';
    return;
  }
  const next: Timeline = {
    ...patch,
    at: serverNow(),
    // Unchanged: the relay assigns the new one. Keeping ours here is what lets
    // the echo (a strictly higher revision) still be applied.
    rev: wire.timeline.rev,
    by: wire.me,
  };
  // Optimistically ours, so the projection is right for this viewer at once and
  // the room's own arithmetic does not fight the gesture that just happened.
  wire.timeline = next;
  lastPublished = next;
  publishedUntil = performance.now() + PUBLISH_SETTLE_MS;
  send({ t: 'timeline', timeline: next });
}

/**
 * Convenience for the common case: keep the content and the tracks, change the
 * rest.
 *
 * Carrying them through matters because the timeline is a *snapshot*: a publish
 * that left `tracks` out would be publishing "the room has no audio preference",
 * so every pause would quietly undo somebody's track choice.
 */
export function publishState(paused: boolean, position: number, speed: number) {
  publish({ content: wire.timeline.content, tracks: wire.timeline.tracks, paused, position, speed });
}

/** Announce what this viewer is playing, and start it from `position`. */
export function publishContent(content: ContentRef | null, position: number, paused: boolean) {
  publish({
    content,
    // A new film is a new set of tracks: keeping the previous one would have the
    // matcher hunting for the last episode's dub in this one. The opener's own
    // choice is published as soon as it is made.
    tracks: null,
    paused,
    position,
    speed: wire.timeline.speed || 1,
  });
}

/**
 * Say which track the room is watching or listening to, for one kind.
 *
 * **Merged with what the room already holds, never replacing it.** The timeline
 * is a snapshot, so publishing `{audio}` alone would say "the room has no
 * subtitle preference" — and a viewer who shares audio but not subtitles would
 * silently undo the subtitle choice of everybody who does share them, every time
 * they changed the dub.
 *
 * Position and pause come from where the room already is: choosing a track is
 * not a claim about either, and reading this machine's own `timePos` here would
 * publish it as though it were a seek.
 */
export function publishTrack(kind: TrackKind, wish: SharedTracks['audio']) {
  if (!wire.on) return;
  const tl = wire.timeline;
  const tracks: SharedTracks = {
    audio: tl.tracks?.audio ?? null,
    sub: tl.tracks?.sub ?? null,
    [kind]: wish,
  };
  publish({
    content: tl.content,
    tracks,
    paused: tl.paused,
    position: positionAt(tl, serverNow()),
    speed: tl.speed || 1,
  });
}

/**
 * Whether this viewer can be played to right now.
 *
 * Reported rather than inferred, because the relay freezes the room on it: a
 * member who is buffering or still opening a file holds everyone, which is the
 * behaviour that makes a room usable over a torrent at all.
 */
export function reportReady(ready: boolean, reason = '') {
  if (lastReady === ready && lastReason === reason) return;
  lastReady = ready;
  lastReason = reason;
  if (wire.on) send({ t: 'ready', ready, reason });
}

/**
 * Change the room's rules — who may drive, and which track choices are shared.
 *
 * One function for all three because they are one kind of thing (what this room
 * does, as opposed to where it is) and because they answer to the same person.
 * The relay refuses anybody but the host; this refuses first so a guest's switch
 * does not flicker on and back.
 *
 * Nothing is applied optimistically: the rules arrive in the `members`
 * broadcast, so every member — including the one who flipped the switch — learns
 * them from the same message, and there is no window in which the host believes
 * something the room does not.
 */
export function setRoomRules(rules: RoomRules) {
  if (!wire.isHost) return;
  send({ t: 'mode', ...rules });
}

function applyRules(rules: { hostOnly: boolean; shareAudio: boolean; shareSubs: boolean }) {
  wire.hostOnly = rules.hostOnly;
  wire.shareAudio = rules.shareAudio;
  wire.shareSubs = rules.shareSubs;
}

// ---- the clock loop ---------------------------------------------------------

function schedulePing() {
  clearTimeout(pingTimer);
  if (!socket) return;
  const wait = pingsSent < PING_FAST_COUNT ? PING_FAST_MS : PING_SLOW_MS;
  pingTimer = setTimeout(() => {
    if (!socket) return;
    pingsSent += 1;
    send({ t: 'ping', c: Date.now() });
    schedulePing();
  }, wait);
}
