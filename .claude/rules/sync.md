---
paths:
  - "src/lib/sync/**"
  - "services/**"
  - "shared/**"
  - "go.work"
  - "src/lib/components/RoomDialog.svelte"
---

# sync — the short form

The short form of [`docs/rules/sync.md`](../../docs/rules/sync.md): the modules this area is made of,
its rules one paragraph each, and the commands that exercise it. Claude Code loads this file
by itself when a file it covers is opened with Read/Edit/Write — not when one is read through
Bash, which is why `CLAUDE.md` says to read it before changing the area.

Each line is the rule; the chapter it links to is the whole of it.

## Modules

| Path | What lives there |
|---|---|
| `src/lib/sync/` | Watching together, in a handful of pieces with a direction that is load-bearing. `protocol.ts` is the wire and a **leaf** (its field lists are compiler-proved complete and compared against `shared/sync-protocol.txt`); `clock.ts` is the offset estimate; `wire.svelte.ts` is the socket, the room and `publish()` — also a leaf, which is what lets `playback` and `seek` publish without a cycle; `content.ts` says what is playing and is the **only** place a `ContentRef` is built (hence the privacy gate); `drift.ts` is the three correction bands; `quality.ts` is the dot beside each member — how well they keep up, judged by themselves from the clock loop's pings and the reconciler's disruptions, pure and tested; `ready.ts` is whether this player is holding the room up, pure and tested because both ways of being wrong are silent — a member stuck "loading" freezes the film, one who claims to be ready while nothing is on their screen is left behind; `apply.svelte.ts` sits **above everything** and is what applies an arriving timeline, opens the room's content and runs the reconciler. Publishing goes down into the leaf, applying goes up — see `docs/watch-together.md` |
| `services/` | The Go services, one directory each, deployed apart and developed together behind the root `go.work`. `relay/` is watching together; `tmdb/` is the metadata proxy the catalog reads (README there for the capacity numbers and why the key lives in it) |
| `services/relay/` | The relay: rooms, a shared timeline, nothing else. Go, one dependency, no database, nothing written to disk. Content travels as opaque JSON and is never parsed there, so a new kind of source never means a redeploy. `cmd/probe` is a headless peer, which is how this is tested without a second machine |

## Rules

- **Watching together is one shared timeline and nothing else, and the shape of the wire is the whole design.** What travels is a **snapshot of the state after a change** — `{content, paused, position, speed, at, rev, by}` — never … → [sync](../../docs/rules/sync.md)
