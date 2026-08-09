# Watching together

Two or more players holding one timeline: the same film, the same position, the
same pauses. No media passes between them and none passes through the relay —
every player fetches the film itself, from its own disk, its own website or its
own swarm. What is shared is the **timeline**, and the roadmap's rule for this
feature was written before a line of it existed:

> **Sync the timeline, not the presentation.** Position, pause, speed and which
> file is playing are shared. Tracks, subtitle appearance, volume and window
> state stay personal.

That rule survives intact, and everything below is a consequence of it or of the
other prediction the roadmap made: that the protocol would be the easy half.

## Why the wire carries state, not actions

The obvious design is to send the gesture — "pause", "seek back five" — and
replay it on the other end. It is wrong for three reasons that only show up once
a network is involved:

- **It is not idempotent.** A message delivered twice seeks twice.
- **It does not survive a loss.** One dropped "pause" and the two players
  disagree for ever, with nothing to notice it.
- **It cannot answer the question that actually matters**, which is not "what
  happened" but "where should I be *right now*". A viewer whose file stalled for
  eight seconds, or who joined mid-film, needs an answer no sequence of past
  actions provides.

So what travels is a snapshot of the state *after* a change:

```
{ content, paused, position, speed, at, rev, by }
```

`at` is a **relay** timestamp and `rev` is assigned by the relay, and both of
those matter. A client applies only a strictly higher `rev`, which makes
reordering and duplication harmless and makes two people pressing space at once
produce one winner rather than two peers each believing something different. And
because `at` is stated in the relay's clock, every player only has to know its
own offset from that one clock — nothing has to agree about wall time.

Where playback should be is then arithmetic anybody can do:

```
target = position + (relayNow - at) / 1000 × speed        (paused ⇒ position)
```

One consequence worth naming: **a message that arrives at a bad moment can
simply be dropped**. While a seek gesture owns the position, while this player's
own change is still in flight, while it is stalled on the network — the
reconciler returns and does nothing, because a snapshot recomputed a second
later is as good as one applied now. There is no state machine to fall out of.

## Correcting drift: speed, not seeking

This is the part the roadmap warned about, and the warning was right.

An exact seek costs decoding from the preceding keyframe. Measured elsewhere in
this project (see *Seek flags are a performance contract* in `CLAUDE.md`): on a
4K60 VP9 file with a 5 s GOP an `absolute+exact` seek is **1.9 s**, against
0.05 s for a keyframe seek. So a player that seeks to correct 300 ms of drift
stops for nearly two seconds to fix a third of one, and comes out further from
the room than it started — with a visible stall where there had been a
difference nobody could see.

Ten seconds at 103 % of speed fixes the same 300 ms with nothing to see and
nothing to hear: mpv resamples with `scaletempo2` by default, so pitch is
preserved and a few per cent is inaudible.

Hence three bands (`src/lib/sync/drift.ts`), and the thresholds are the whole of
that module:

| difference | what happens |
|---|---|
| ≤ 0.15 s | nothing. Below perception across two rooms, and correcting it would mean never running at the speed the film was authored at |
| ≤ 2 s | bend the speed, proportionally, aiming to erase it over ~10 s; never more than ±10 % |
| > 2 s | one seek, and accept its cost |
| paused, > 0.25 s | seek. There is no speed to bend, and a still frame is exactly where a difference shows |

The correction is applied **around the room's speed**, not around 1×, so a room
watching at 1.5× that drifts does not silently lose the speed everybody chose.
And it is written straight to mpv rather than through the playback verb, because
it is this machine catching up — publishing it would have every peer chasing
every other peer.

## Readiness: one person buffering stops the room

The relay decides this, and it decides it by **stamping a real timeline** rather
than by setting a flag beside one: when anybody is not ready it publishes a
paused timeline at the position playback had reached, and lifts that pause when
everybody is ready again. Freezing as a timeline change is what keeps every
client's projection working unmodified — a viewer who joins mid-wait sees the
same state as everyone else, and drift correction keeps working through it.

Three rules make it usable rather than annoying:

- **A pause a person asked for is never lifted this way.** The relay records
  whether the current pause is its own; a human pausing during a wait clears
  that, so the film stays paused when the buffering ends.
- **Resuming while somebody is still loading means "resume when they are
  done"** — the relay accepts the change, notes that the pause is now its own
  again, and lifts it at the right moment.
- **A member who never reports is counted as ready after 45 seconds.** Without
  it, one player that died mid-buffer holds an evening with nothing on screen
  able to explain it.

Joining a room also counts as not-ready, which is correct: the newcomer has a
file to open, and the others should wait for the person who just arrived.

## Identifying what is playing

A relay carries no media, so the only thing a room can share is enough to
*find* the film. How well that works depends entirely on where it came from, and
the four cases are four honest answers rather than four formats.

**A torrent is the good case**, and it is why the roadmap paired this feature
with torrents in the first place. An info hash plus a file index names a file
exactly; every player fetches it from the swarm itself. Because the metadata is
cached beside the data (`torrents.md`), a guest joining mid-season is a
`torrent_add` measured in milliseconds and a `loadfile` — and switching episode
is the same info hash with another index, so a season plays through without
anybody re-pasting anything. There is no equivalent for local files, which is
the asymmetry that decides how good this feature feels.

**A URL** works if each machine can resolve it; yt-dlp runs on both ends
independently.

**A local file cannot be sent.** What travels is enough to recognise a copy: the
release hash (the OpenSubtitles scheme — size plus 64 KiB from each end —
already implemented in `opensubtitles.rs` and reused rather than rewritten), the
duration and the size. The viewer opens their own copy and the player says which
of three things it is:

| verdict | meaning |
|---|---|
| the same release | hashes match. A shared timeline means exactly what it says |
| a different rip | the durations agree within 2 s but the hashes do not. Approximately right, and saying so is worth more than refusing |
| a different file | the durations disagree. The timeline still runs, but it points somewhere else in this copy |

Nothing is refused on this verdict. A viewer who deliberately opens the
director's cut is not making a mistake the player should correct — they are
making one the player should mention.

**A file under a privacy root** publishes `hidden`: the timeline works, the name
does not travel. See below.

The comparison is *identity*, not equality: a title that arrived late, a
duration that firmed up after the file opened, or a magnet rebuilt from the info
hash must not read as a different film, because every one of those would restart
playback for everybody in the room.

## Privacy: the seventh enforcement point

`CLAUDE.md` states the rule that generated this section: *every new path by
which something about a file leaves this machine is another enforcement point*.
A room is such a path, and the most direct of the seven — the other six speak to
a service or to a device on the same network, and this one speaks to other
people, by name, in a room they were invited to by a code that can be forwarded.

So a file under a privacy root publishes `{ kind: 'hidden' }` and nothing else:
no name, no size, no hash, not even to the relay. The timeline still
synchronises, so a room can watch something one member keeps private, and the
panel says so out loud rather than leaving it to be discovered.

The check lives in `contentOf()`, which is deliberately the **only** place a
content reference is ever built — a privacy check with two entry points is a
privacy check with one of them missing. The release hash carries a second gate
in Rust (`release_hash` refuses a private path exactly as `subs_search` does),
which is the same belt-and-braces the rest of this rule already has.

## The bus

The request this feature was built under was that actions be picked up by a
shared session **by design**, not by somebody remembering to wire each one. The
codebase already had the mechanism, built for casting:

- Every gesture — a button, a menu row, a hotkey, a seekbar drag — goes through
  a **verb** in `playback.svelte.ts`. That is what the module is for. Publishing
  from the verbs means a control added next year is shared without anybody
  thinking about it.
- `SYNC_BEHAVIOR` is a `Record<ActionId, 'shared' | 'personal' | 'solo'>`, so a
  new action **does not compile** until somebody has said which kind it is.
  Exactly the device `CAST_BEHAVIOR` already is, and for exactly the same
  failure: every casting bug that module exists for was an omission, and an
  omission the compiler refuses never reaches a viewer.

Only one action is `solo` — refused while a room is on — and it is the A–B loop,
which holds playback inside a segment that drift correction would fight once a
second for as long as the loop lasts. Frame stepping is `personal` rather than
`solo`, because a frame is an order of magnitude below the deadband and the room
is paused anyway. Repeat mode is `personal` for a subtler reason: a viewer whose
file loops while the room moves on ends up on different content, and that heals
itself, because the room's content is opened.

Publishing is **release-only** for gestures, which is the same rule the cast
seekbar already keeps: a drag is a stream of positions and every one of them
would be a seek on somebody else's machine. The two places a gesture ends —
`onSeekUp` and `endScrub` — are the only two in `seek.svelte.ts` that publish.

### Why the module is in three pieces

`npm run check-imports` forbids cycles in `src/`, and a cycle here would be the
silent kind: the bundler resolves it and leaves a module-evaluation order nobody
chose. `playback` and `seek` already sit high in the graph (one reaches `cast`,
the other `thumbs`), so:

```
sync/wire.svelte.ts     a leaf — imports nothing from the player
      ▲          ▲
      │ publish  │ publish
playback       seek
      ▲
sync/apply.svelte.ts    above everything: player, cast, seek, open, torrent, playlist
      ▲
+page.svelte            initSync()
```

Publishing goes *down* into the leaf; applying goes *up*, and the two halves
meet only through the callbacks `initSync` registers.

A related distinction that is easy to get wrong: **an arriving timeline is not a
gesture.** It goes to mpv directly rather than through the verbs, because a verb
raises an OSD, pauses a drag, and — the one that would actually break —
publishes. Routing arrivals back through the verbs would have every peer
re-broadcasting every peer. The player already draws this line elsewhere:
`applyProperty` sweeping a stale mirror against the `property` hook acting on a
real event.

## The relay

Go, `server/`, one dependency (`github.com/coder/websocket`), a static binary of
a few megabytes on `scratch`. No database, nothing written to disk, and a room
ceases to exist a few minutes after the last person leaves — deliberately, not
for want of finishing: the least this can know is the most it should.

Builds point at `relay.frameplayer.app`, and the address is a **field in the
settings sheet** rather than a build-time constant — so running your own is a
setting rather than a fork, and `server/` is the whole of what has to be
deployed. It sits under «Основные» rather than in the room dialog, where it
started: practically nobody runs their own relay, so a field on the way into
every room was a question with one answer standing in front of the two controls
that matter. The room dialog points at settings when the address turns out to be
wrong, which is the only moment it is worth reading. What the relay learns is a
room code, a display name and what the room is watching (unless it is hidden);
what it never sees is the film.

**Content is opaque to it.** What is playing travels as raw JSON and is never
parsed, only bounded in size, so a new kind of source is a change to the player
and never a redeploy of the relay. The one thing the relay does decide is
readiness, because that has to be decided somewhere everybody can see.

Rooms hold a plain mutex rather than running as an actor goroutine, and that is
a considered choice rather than the lazy one. The actor's advantages — a
broadcast that cannot deadlock, total ordering — both hold here anyway, because
every send into a member's queue is non-blocking and the lock makes ordering
total. What the actor adds on top is a lifecycle (start, stop, the race between
the hub handing over a client and the room having exited) that has to be got
right for nothing. The rule that keeps it true: **never do I/O under the lock**.
A member's outbox is a buffered channel, and a full one costs that member their
connection, never the room its progress.

**Room codes are Crockford's base32.** A code is read aloud and typed back, so
the four glyphs that make that go wrong (I, L, O, U) are out of the alphabet —
and, crucially, the ones people type anyway are *folded onto their look-alikes*
rather than refused. An alphabet that merely excluded them would reject the code
the viewer is looking at, which is the worse half of the same problem. 32^6 ≈
1.07e9, which is not a cryptographic secret and is not meant to be one: the code
is a handle, its lifetime is an evening, and joins are rate-limited per address
so the space cannot be walked.

### The contract between two languages

`shared/sync-protocol.txt` lists the field names of every message, and both test
suites read it. It exists because JSON decoding does not complain about a field
it did not find — it leaves a zero — so a rename on one side costs no error
anywhere. What it costs is a room where somebody presses pause and nothing
happens, with both machines looking healthy.

Go marshals a fully populated value of each message and compares the keys it
actually produced. TypeScript compares a list of field names the compiler
already forces to match each interface (`Complete<T, F>` in `protocol.ts`
resolves to `true` only while nothing is missing, and names the missing field
otherwise). A rename on either side turns that side red until the file is
updated, and updating it turns the other side red until it agrees.

One trap found while building this, and it is the kind that makes a contract
quietly stop being one: **`go test` caches**, and the fixture lives outside the
Go module, so a renamed field was answered with `(cached)` while the two
languages disagreed. `t.Setenv` marks the test uncacheable, which is why there
is an otherwise pointless-looking environment variable at the top of it.

## Testing it without a second computer

The player is single-instance, so a second `npm run tauri dev` signals the first
rather than starting one — which would make "does this still sync" cost a second
machine, and in practice mean it never gets checked. `server/cmd/probe` is the
other end of a room: it joins, follows the timeline, and prints once a second
where it thinks playback is.

```bash
go run ./server &
go run ./server/cmd/probe -play -drive           # creates a room, prints the code
go run ./server/cmd/probe -room ABC123 -drive     # joins the player's room
go run ./server/cmd/probe -room ABC123 -skew 300ms
go run ./server/cmd/probe -room ABC123 -hold 20s
```

`-skew` makes the probe lie about its own clock, which is the only way to see
drift correction working as something other than a coincidence; `-hold` keeps
the probe not-ready so the freeze and the waiting overlay get exercised. A
measured run of the pair: probe A opens a film, probe B joins three seconds in,
the relay freezes the room at **3.00 s** — the projected position, not the last
human one — and resumes there when B reports ready; B's measured clock offset
comes out at exactly **−250 ms** against a `-skew 250ms`, cancelling it.

## What this deliberately does not do

- **No media through the relay.** Not a limitation to lift later; it is the
  design.
- **No sharing of a local file.** Making a torrent out of one's own copy is
  technically within reach (librqbit can create one, and the seeding, port and
  UPnP machinery exists) but it is a different feature with a legal dimension of
  its own, and it is not this one.
- **No chat.** The bus would carry it; a room is not a messenger.
- **No discovery of who else is watching.** A room exists because somebody sent
  a code.
