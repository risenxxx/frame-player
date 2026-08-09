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

/** An empty room's timeline: nothing playing, nothing stamped. */
export function emptyTimeline(): Timeline {
  return { content: null, paused: true, position: 0, speed: 1, at: 0, rev: 0, by: '' };
}

// ---- messages ---------------------------------------------------------------

export type ClientMsg =
  | { t: 'hello'; ver: number; room: string; name: string }
  | { t: 'timeline'; timeline: Timeline }
  | { t: 'ready'; ready: boolean; reason: string }
  | { t: 'mode'; hostOnly: boolean }
  | { t: 'ping'; c: number }
  | { t: 'bye' };

export interface Welcome {
  t: 'welcome';
  ver: number;
  room: string;
  me: string;
  host: string;
  hostOnly: boolean;
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

const TIMELINE_FIELDS = ['content', 'paused', 'position', 'speed', 'at', 'rev', 'by'] as const;
const MEMBER_FIELDS = ['id', 'name', 'ready'] as const;
const HELLO_FIELDS = ['t', 'ver', 'room', 'name'] as const;
const CLIENT_TIMELINE_FIELDS = ['t', 'timeline'] as const;
const READY_FIELDS = ['t', 'ready', 'reason'] as const;
const MODE_FIELDS = ['t', 'hostOnly'] as const;
const PING_FIELDS = ['t', 'c'] as const;
const BYE_FIELDS = ['t'] as const;
const WELCOME_FIELDS = [
  't',
  'ver',
  'room',
  'me',
  'host',
  'hostOnly',
  'members',
  'timeline',
  'waiting',
  'now',
] as const;
const SERVER_TIMELINE_FIELDS = ['t', 'content', 'paused', 'position', 'speed', 'at', 'rev', 'by'] as const;
const MEMBERS_FIELDS = ['t', 'members', 'host', 'hostOnly', 'waiting'] as const;
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
