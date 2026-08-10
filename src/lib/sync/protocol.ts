/**
 * The watch-together wire, as this end sees it.
 *
 * Mirrored by `server/internal/wire/protocol.go`, and the two are kept honest by
 * `shared/sync-protocol.txt`: a list of field names that both test suites read.
 * That file exists because JSON decoding does not complain about a field it did
 * not find — it leaves a zero — so a rename on one side costs no error anywhere.
 * What it costs is a room where somebody presses pause and nothing happens, with
 * both machines looking healthy, which is the worst shape a bug can have.
 *
 * The lists at the bottom are what makes that check possible from here: TypeScript
 * interfaces are erased at runtime, so a test cannot enumerate their fields. They
 * are written out once and the compiler is made to prove they are complete —
 * add a field to `Timeline` without adding it there and `npm run check` fails on
 * the line that says so.
 *
 * **A leaf module.** It imports nothing, which is what lets `playback` and `seek`
 * reach the bus without a cycle (see the header of `wire.svelte.ts`).
 */

/** Bumped only for a change a peer cannot ignore. */
export const PROTOCOL_VERSION = 1;

// ---- what is playing --------------------------------------------------------

/**
 * What the room is watching, as opposed to how any one member reaches it.
 *
 * The relay never parses this — it travels as opaque JSON and is only bounded in
 * size — so a new kind of source is a change here and never a redeploy of the
 * server. The four kinds are the four honest answers to "can the others open
 * what I am watching":
 *
 * - `torrent` — yes, exactly. An info hash plus an index identifies a file the
 *   way nothing else here can, and the others can fetch it themselves. This is
 *   the case the feature is best at.
 * - `url` — yes, if the site lets them. yt-dlp resolves it on each machine.
 * - `file` — no. Nothing can be sent, so what travels is enough to *recognise* a
 *   copy: the release hash (size plus 64 KiB from each end, the OpenSubtitles
 *   scheme already implemented in `opensubtitles.rs`), the duration and the size.
 *   A viewer opens their own copy and the player says whether it is the same
 *   release, a different rip, or something else entirely.
 * - `hidden` — the file is under a privacy root, so the room is told only that
 *   something is playing. The timeline still works; the name does not travel.
 */
export type ContentRef =
  | {
      kind: 'torrent';
      /** How to reopen it: the info hash is the identity, `dn` only a label. */
      magnet: string;
      infoHash: string;
      /** Which file inside the torrent. */
      index: number;
      /** The path inside the torrent, for matching and for display. */
      file: string;
      title: string;
      duration: number;
    }
  | { kind: 'url'; url: string; title: string; duration: number }
  | {
      kind: 'file';
      title: string;
      duration: number;
      size: number;
      /** The OpenSubtitles release hash, or '' when it could not be read. */
      hash: string;
    }
  | { kind: 'hidden' };

// ---- the timeline -----------------------------------------------------------

/**
 * What the room agrees on, as a snapshot of the state *after* a change — never a
 * delta.
 *
 * That is the whole reason this shape works: it is idempotent, it survives a
 * dropped message, and it is directly readable by drift correction, which has to
 * answer "where should I be *now*" rather than "what happened". A replayed action
 * ("seek back five seconds") answers none of those.
 *
 * `at`, `rev` and `by` are the relay's to fill in. A client sends them as zeros
 * and they are overwritten, which is what keeps revisions monotonic however many
 * people press space at once.
 */
export interface Timeline {
  content: ContentRef | null;
  /**
   * How the room is playing it — currently the audio track, and nothing else.
   *
   * On the timeline rather than in a message of its own, and that is worth
   * saying: a track choice is not "where in the film" and does not obviously
   * belong here. What earns it the place is that the timeline is already a
   * *snapshot* with last-writer-wins semantics and already arrives with the
   * handshake — so a viewer joining mid-film gets the room's audio choice for
   * free, and a dropped message costs nothing. A separate message would need
   * its own delivery, its own re-statement on join, and its own reasoning about
   * ordering against the timeline it accompanies.
   */
  tracks: SharedTracks | null;
  paused: boolean;
  /** Seconds into the file at `at`. */
  position: number;
  speed: number;
  /** Relay clock, milliseconds. */
  at: number;
  /** Monotonic within a room; only a strictly higher one may be applied. */
  rev: number;
  /** Member who caused it, or '' for the relay itself (the readiness freeze). */
  by: string;
}

/**
 * What the room agrees about *how* to play the film.
 *
 * **Both kinds travel; whether either is shared is a rule of the room**, set by
 * the host beside "only the host controls playback" — not a preference each
 * viewer keeps. That is the difference between a room whose members agree about
 * what it does and a room where one person's audio choice reaches everybody
 * while another's does not.
 *
 * The defaults are the asymmetry, and they are the argument for having the
 * switches at all. A room is watching one film and listening to one soundtrack,
 * so audio is shared — hearing different audio is a strange way to watch
 * together. Subtitles default off, because one viewer needs them and another
 * does not, one reads a second language and another is a native speaker;
 * sharing that choice would turn them *off* for somebody who cannot follow the
 * film without them. Subtitle size, position and delay are presentation and stay
 * personal unconditionally — that is the roadmap's rule, and these two are the
 * only things on the other side of it.
 *
 * **A description, never an id.** Track ids are positions inside one file: the
 * Russian dub that is #2 in one rip is routinely #3 in another, so an id shared
 * between two copies selects the wrong thing silently. What travels is the
 * descriptor the player already stores for its own per-folder track memory, and
 * the receiving end resolves it with the same scoring (`matchTrack`) — which is
 * what makes this work at all when two people have different releases.
 *
 * `null` on either side means "the room has no opinion", which is different from
 * `'no'` — the explicit "play none of them".
 */
export interface SharedTracks {
  audio: TrackDescriptor | 'no' | null;
  sub: TrackDescriptor | 'no' | null;
}

/** One kind of track, where both are handled the same way. */
export type TrackKind = 'audio' | 'sub';

/** Mirrors `TrackDesc` in player.svelte.ts — see `SharedTracks`. */
export interface TrackDescriptor {
  lang: string | null;
  title: string | null;
  codec: string | null;
  forced: boolean;
  index: number;
}

export interface Member {
  id: string;
  name: string;
  /** False while this member is buffering or still opening the file. */
  ready: boolean;
}

/**
 * Where the timeline says playback is at `nowMs` on the *relay's* clock.
 *
 * The one piece of arithmetic both ends run, which is why `at` is a relay
 * timestamp: every client knows its own offset from that clock, and nothing has
 * to agree about wall time.
 */
export function positionAt(t: Timeline, nowMs: number): number {
  if (t.paused || !t.content) return t.position;
  return Math.max(0, t.position + ((nowMs - t.at) / 1000) * t.speed);
}

/**
 * Whether an arriving timeline may replace the one this client holds.
 *
 * The rule that stops a shared session from coming apart on a bad connection,
 * and it is one line because almost all of the work is done by the wire itself:
 * a WebSocket is a TCP stream, so within one connection messages *cannot*
 * arrive out of order — they arrive in order or the connection breaks. What is
 * left to defend against is everything around that:
 *
 *   - a **reconnect**, where the previous socket's traffic must not be mixed
 *     into the new session (the `socket !== sock` guards do that);
 *   - a **duplicate or replay**, which a monotonic revision makes inert;
 *   - and plain **delay**, which costs nothing at all — the timeline is a
 *     projection from `at`, so a snapshot that arrives two seconds late still
 *     computes the correct position for right now. That is the deepest reason
 *     the wire carries state rather than actions: a late *action* is wrong, a
 *     late *snapshot* is merely old.
 *
 * **`>=`, not `>`, and that is load-bearing rather than an off-by-one.** A
 * refusal is answered by the relay re-sending the room's current timeline at
 * the *same* revision it already had — and that message is precisely the
 * correction that pulls a guest back from the position they optimistically
 * moved to. Dropping it as "not newer" would leave them somewhere the room is
 * not, permanently.
 *
 * `authoritative` is the handshake: a `welcome` describes the room as it is now
 * and is never a stale reading of it. It also covers the one case a revision
 * cannot — a room that has ceased to exist and been created afresh starts
 * counting again, and a client still holding the old count would otherwise
 * reject every timeline it was ever sent.
 */
export function shouldApply(held: Timeline, next: Timeline, authoritative: boolean): boolean {
  return authoritative || next.rev >= held.rev;
}

/** An empty room's timeline: nothing playing, nothing stamped. */
export function emptyTimeline(): Timeline {
  return { content: null, tracks: null, paused: true, position: 0, speed: 1, at: 0, rev: 0, by: '' };
}

// ---- messages ---------------------------------------------------------------

export type ClientMsg =
  | { t: 'hello'; ver: number; room: string; name: string }
  | { t: 'timeline'; timeline: Timeline }
  | { t: 'ready'; ready: boolean; reason: string }
  | ({ t: 'mode' } & RoomRules)
  | { t: 'ping'; c: number }
  | { t: 'bye' };

/**
 * The room's own rules, as opposed to where it is in the film.
 *
 * All three belong to the **host**, and that is one sentence rather than three:
 * the host owns the room's rules. A panel where one switch answers to a
 * different person than the two beside it is a panel nobody can predict.
 *
 * On a `mode` message each is optional — only what is being changed is sent —
 * and on the way back they are always present, because the receiver is being
 * told the whole state rather than a delta.
 */
export interface RoomRules {
  hostOnly?: boolean;
  /** A track choice by anybody applies to everybody, per kind. */
  shareAudio?: boolean;
  shareSubs?: boolean;
}

export interface Welcome {
  t: 'welcome';
  ver: number;
  room: string;
  me: string;
  host: string;
  hostOnly: boolean;
  shareAudio: boolean;
  shareSubs: boolean;
  members: Member[];
  timeline: Timeline;
  waiting: string[];
  /** The relay's clock, so there is a usable offset before the first ping. */
  now: number;
}

/** A Timeline flattened next to `t` rather than nested under a key. */
export type TimelineMsg = { t: 'timeline' } & Timeline;

export interface MembersMsg {
  t: 'members';
  members: Member[];
  host: string;
  hostOnly: boolean;
  shareAudio: boolean;
  shareSubs: boolean;
  /** Ids, not names: the frontend renders them itself, and a rename cannot
   * desync the two lists. */
  waiting: string[];
}

export interface PongMsg {
  t: 'pong';
  /** Our own reading, echoed untouched — the round trip is measured against it. */
  c: number;
  s: number;
}

export interface ErrorMsg {
  t: 'error';
  code: string;
  message: string;
}

export type ServerMsg = Welcome | TimelineMsg | MembersMsg | PongMsg | ErrorMsg;

/**
 * Every refusal the relay can send, as a union rather than a string.
 *
 * `t('sync.err_…')` is keyed off these, so a code the relay grows without a
 * sentence here is a compile error rather than a blank line in the dialog.
 */
export type ErrorCode =
  | 'no_room'
  | 'room_full'
  | 'busy'
  | 'rate_limited'
  | 'bad_message'
  | 'bad_version'
  | 'not_allowed';

const ERROR_CODES: ReadonlySet<string> = new Set<ErrorCode>([
  'no_room',
  'room_full',
  'busy',
  'rate_limited',
  'bad_message',
  'bad_version',
  'not_allowed',
]);

export function isErrorCode(code: string): code is ErrorCode {
  return ERROR_CODES.has(code);
}

// ---- the shared field-name contract -----------------------------------------
//
// `Missing<T, F>` is what makes these lists provably complete: it resolves to the
// keys of `T` that `F` does not name, and each assertion below is typed as
// `true` only while that is `never`. Add a field to an interface without adding
// it to its list and the error names the field you forgot.

type Missing<T, F extends readonly string[]> = Exclude<keyof T & string, F[number]>;
type Complete<T, F extends readonly string[]> =
  Missing<T, F> extends never ? true : ['missing from the field list:', Missing<T, F>];

const TIMELINE_FIELDS = ['content', 'tracks', 'paused', 'position', 'speed', 'at', 'rev', 'by'] as const;
const MEMBER_FIELDS = ['id', 'name', 'ready'] as const;
const HELLO_FIELDS = ['t', 'ver', 'room', 'name'] as const;
const CLIENT_TIMELINE_FIELDS = ['t', 'timeline'] as const;
const READY_FIELDS = ['t', 'ready', 'reason'] as const;
const MODE_FIELDS = ['t', 'hostOnly', 'shareAudio', 'shareSubs'] as const;
const PING_FIELDS = ['t', 'c'] as const;
const BYE_FIELDS = ['t'] as const;
const WELCOME_FIELDS = [
  't',
  'ver',
  'room',
  'me',
  'host',
  'hostOnly',
  'shareAudio',
  'shareSubs',
  'members',
  'timeline',
  'waiting',
  'now',
] as const;
const SERVER_TIMELINE_FIELDS = [
  't',
  'content',
  'tracks',
  'paused',
  'position',
  'speed',
  'at',
  'rev',
  'by',
] as const;
const MEMBERS_FIELDS = ['t', 'members', 'host', 'hostOnly', 'shareAudio', 'shareSubs', 'waiting'] as const;
const PONG_FIELDS = ['t', 'c', 's'] as const;
const ERROR_FIELDS = ['t', 'code', 'message'] as const;

/** One variant of the client union, picked by its `t`. */
type ClientOf<K extends ClientMsg['t']> = Extract<ClientMsg, { t: K }>;

const _timeline: Complete<Timeline, typeof TIMELINE_FIELDS> = true;
const _member: Complete<Member, typeof MEMBER_FIELDS> = true;
const _hello: Complete<ClientOf<'hello'>, typeof HELLO_FIELDS> = true;
const _clientTimeline: Complete<ClientOf<'timeline'>, typeof CLIENT_TIMELINE_FIELDS> = true;
const _ready: Complete<ClientOf<'ready'>, typeof READY_FIELDS> = true;
const _mode: Complete<ClientOf<'mode'>, typeof MODE_FIELDS> = true;
const _ping: Complete<ClientOf<'ping'>, typeof PING_FIELDS> = true;
const _bye: Complete<ClientOf<'bye'>, typeof BYE_FIELDS> = true;
const _welcome: Complete<Welcome, typeof WELCOME_FIELDS> = true;
const _serverTimeline: Complete<TimelineMsg, typeof SERVER_TIMELINE_FIELDS> = true;
const _members: Complete<MembersMsg, typeof MEMBERS_FIELDS> = true;
const _pong: Complete<PongMsg, typeof PONG_FIELDS> = true;
const _error: Complete<ErrorMsg, typeof ERROR_FIELDS> = true;

/**
 * What the test compares against `shared/sync-protocol.txt`.
 *
 * The assertions above prove each list matches its interface; this is what
 * proves it matches the other language. Both halves are needed — one without the
 * other is a contract with one signature.
 */
export const PROTOCOL_FIELDS: Readonly<Record<string, readonly string[]>> = {
  timeline: TIMELINE_FIELDS,
  member: MEMBER_FIELDS,
  'client:hello': HELLO_FIELDS,
  'client:timeline': CLIENT_TIMELINE_FIELDS,
  'client:ready': READY_FIELDS,
  'client:mode': MODE_FIELDS,
  'client:ping': PING_FIELDS,
  'client:bye': BYE_FIELDS,
  'server:welcome': WELCOME_FIELDS,
  'server:timeline': SERVER_TIMELINE_FIELDS,
  'server:members': MEMBERS_FIELDS,
  'server:pong': PONG_FIELDS,
  'server:error': ERROR_FIELDS,
};

// The completeness assertions are declarations whose only purpose is to be
// type-checked; naming them here keeps them from reading as dead code to a
// linter, and keeps the failure on the assertion rather than on an unused local.
export const FIELD_CHECKS = [
  _timeline,
  _member,
  _hello,
  _clientTimeline,
  _ready,
  _mode,
  _ping,
  _bye,
  _welcome,
  _serverTimeline,
  _members,
  _pong,
  _error,
] as const;

// ---- room codes -------------------------------------------------------------

/**
 * Crockford's base32, and the reason for it: a person reads a code out loud and
 * another person types it in. The four glyphs that make that go wrong — I, L, O
 * and U — are out of the alphabet, and the ones people type anyway are folded
 * onto their look-alikes rather than refused, because refusing the code the
 * viewer is looking at is the worse half of the same problem.
 *
 * Kept in step with `NormalizeCode` in `server/internal/wire/code.go`; the
 * relay normalises again on arrival, so a disagreement here costs a refusal
 * rather than a wrong room.
 */
export const CODE_LENGTH = 6;
const CODE_ALPHABET = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';

export function normalizeCode(input: string): string {
  let out = '';
  for (const raw of input.trim().toUpperCase()) {
    if (raw === ' ' || raw === '-' || raw === '_' || raw === '.') continue;
    const ch = raw === 'I' || raw === 'L' ? '1' : raw === 'O' ? '0' : raw;
    // `U` is neither in the alphabet nor a look-alike for anything in it, so it
    // can only be a typo — and guessing would open a room that is not theirs.
    if (!CODE_ALPHABET.includes(ch)) return '';
    out += ch;
  }
  return out.length === CODE_LENGTH ? out : '';
}

/** A code as it is shown: `ABC-123`, which is how people read one aloud. */
export function formatCode(code: string): string {
  return code.length === CODE_LENGTH ? `${code.slice(0, 3)}-${code.slice(3)}` : code;
}
