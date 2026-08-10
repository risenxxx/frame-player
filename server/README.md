# Watch-together relay

A room holds a **timeline** — what is playing, whether it is paused, where in it
and at what speed — and nothing else. The players fetch the film themselves,
from a disk, a website or a swarm, so no media passes through here and none ever
will. There is no database, nothing is written to disk, and a room ceases to
exist a few minutes after the last person leaves.

That is the whole design, and the rest of this file is consequences of it.

```bash
go run ./server                 # :8080
go test -race ./server/...
```

## Running it

```bash
CGO_ENABLED=0 go build -trimpath -ldflags "-s -w" -o frameplayer-relay ./server
```

A static binary of a few megabytes with no runtime dependencies. Put a TLS
terminator in front of it — the player will refuse a plaintext `ws://` address
that is not on loopback — and set `RELAY_TRUST_PROXY` so the join rate limit
keys on the viewer's address rather than on the proxy's.

Shipped builds point at **`relay.frameplayer.app`** by default
(`DEFAULT_RELAY` in `src/lib/sync/wire.svelte.ts`). Anything below is what it
takes to run your own instead: the address is a field in the player's settings
(«Основные») rather than a build-time constant, so self-hosting is a setting and
not a fork. Leaving it empty restores the default rather than turning the
feature off.

`Dockerfile` builds the same binary onto `scratch`;
`frameplayer-relay.service` runs it under systemd with everything locked down
that a process keeping no state can afford to lose.

| Variable | Default | |
|---|---|---|
| `RELAY_ADDR` | `:8080` | listening address |
| `RELAY_PUBLIC_URL` | — | what `/j/<code>` prints as the address to share |
| `RELAY_ORIGINS` | `*` | allowed `Origin` patterns — see below |
| `RELAY_TRUST_PROXY` | off | honour `X-Forwarded-For` |
| `RELAY_DOWNLOAD_WIN` · `_MAC` | — | direct installer for the visitor's platform, offered on the invitation page |
| `RELAY_DOWNLOAD_PAGE` | — | fallback for any other platform, or when no direct installer is set |
| `RELAY_MAX_ROOMS` | 5000 | |
| `RELAY_MAX_MEMBERS` | 16 | per room |
| `RELAY_ROOM_TTL` | 5m | how long an empty room waits for somebody to come back |
| `RELAY_PING` | 20s | liveness |
| `RELAY_SWEEP` | 15s | |

`GET /healthz` and `GET /metrics` (plain text) are there for a monitor.

The invitation page answers in Russian or English from `Accept-Language`, and
offers the installer for the platform in the `User-Agent` where one is
configured — a download does not take the tab with it, so the code stays on
screen behind it. It is a document: no script, no framework, nothing that runs.
Its one external dependency is the typeface, from Google Fonts. Embedding a copy
was the first version and was dropped: the privacy discipline that shapes the
*relay* is about the room and what is being watched in it, and applying it to a
webfont on a public invitation page bought nothing but a second copy of a font
to keep in step with the player's.

With none of the download variables set the page simply offers nothing, which is
better than a dead link.

**`RELAY_ORIGINS` defaults to `*` on purpose.** The room code is the only secret
here; there are no cookies, no credentials and no ambient authority of any kind,
so an `Origin` check defends against nothing — while a wrong one would refuse
the desktop app, whose origin is `tauri://localhost` on macOS and
`http://tauri.localhost` on Windows. Set it if you want the relay reachable only
from a page of your own.

## Capacity

A room is a few hundred bytes and a socket per viewer, and it is silent between
gestures — a paused film sends nothing for an hour. The traffic is a ping every
thirty seconds per client plus a message whenever somebody presses something.
One small VPS is not the constraint; file descriptors are.

The bounds that matter are all refusals rather than truncations, because a
message that does not fit is a bug or an attack and keeping half of it is how
the second becomes the first: 8 KiB a frame, 4 KiB of content, 16 members,
20 messages a second per connection, 10 joins per address and one more every six
seconds.

## What it does not understand

**Content is opaque.** What is playing travels as raw JSON and is never parsed
here — the sender writes a `ContentRef`, the receivers read one, and the relay
only bounds its size. So a new kind of source (a new protocol, a new identity
scheme) is a change to the player and never a redeploy of this.

The one thing the relay *does* decide is **readiness**. A member who is
buffering or still opening a file holds the room: it stamps a timeline that is
paused at the position playback had reached, and lifts that pause when everyone
is ready again. Freezing as a real timeline change rather than a flag beside one
is what keeps every client's projection working unmodified — a viewer who joins
mid-wait sees the same state as everybody else. A pause a *person* asked for is
never lifted this way, and a member who never reports is counted as ready after
45 seconds, so one stuck client cannot hold an evening.

## Protocol

`internal/wire` is the whole of it, and `../shared/sync-protocol.txt` is the
contract it shares with `src/lib/sync/protocol.ts` — a list of field names that
both test suites read, so a rename on either side turns the other red. That file
exists because JSON decoding does not complain about a field it did not find: it
leaves a zero, and what you get is a room where somebody presses pause and
nothing happens, with no error on either machine.

The timeline travels as a **snapshot of the state after a change, never a
delta**. That is what makes it idempotent, survivable across a dropped message,
and directly readable by the drift correction each player runs. `at`, `rev` and
`by` are the relay's to fill in; a client that sets them is overwritten, which is
what keeps revisions monotonic however many people press space at once.

Clock offset is a ping whose payload is the client's own reading, echoed
untouched: `rtt = now − c`, `offset = s + rtt/2 − now`, median of the fastest
half of the last eight samples.

## Testing it without a second computer

The player is single-instance, so a second `npm run tauri dev` signals the first
rather than starting one — which would make "does this still sync" cost a second
machine, and in practice mean it never gets checked. `cmd/probe` is the other end
of a room:

```bash
go run ./server &
go run ./server/cmd/probe -play -drive          # creates a room, prints the code
go run ./server/cmd/probe -room ABC123 -drive    # joins the player's room
go run ./server/cmd/probe -room ABC123 -skew 300ms   # a clock that is wrong on purpose
go run ./server/cmd/probe -room ABC123 -hold 20s     # freeze the room, on purpose
```

It prints the timeline it received and, once a second, where it thinks playback
is — so the real player can be driven by hand and watched from a terminal.
`-skew` is the only way to see drift correction working as something other than
a coincidence, and `-hold` is how the waiting overlay gets exercised.
