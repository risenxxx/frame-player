---
paths:
  - "src-tauri/src/torrent*.rs"
  - "src/lib/torrent.svelte.ts"
---

# torrents-core — the short form

The short form of [`docs/rules/torrents-core.md`](../../docs/rules/torrents-core.md): the modules this area is made of,
its rules one paragraph each, and the commands that exercise it. Claude Code loads this file
by itself when a file it covers is opened with Read/Edit/Write — not when one is read through
Bash, which is why `CLAUDE.md` says to read it before changing the area.

Each line is the rule; the chapter it links to is the whole of it.

## Modules

| Path | What lives there |
|---|---|
| `src-tauri/src/torrent.rs` | Torrent streaming: the lazy librqbit session, the loopback HTTP server mpv opens (`/t/<infohash>/<index>/<name>`, Range + HEAD), and `torrent_add`/`torrent_status`/`torrent_release`/`torrent_clear_cache`. Frontend half in `src/lib/torrent.svelte.ts` |

## Critical gotchas

(Numbered as in `CLAUDE.md`, which keeps the headline of each.)

16. **A file descriptor is not a renewable resource on macOS, and running out of them does not fail like running out of something.** launchd gives a GUI application a soft `RLIMIT_NOFILE` of **256**, and exhausting it poisons `AudioComponentInstanceNew` and `VTIsHardwareDecodeSupported` **for the life of the process** — which reads as a video that never plays, with nothing in any log to connect it to a limit. `raise_fd_limit()` is the first statement of `run()`, and `torrent_storage.rs` opens a torrent's files lazily under a process-wide budget. → [torrents-core](../../docs/rules/torrents-core.md)

## Rules

- **A torrent is a network source that steers itself, and both halves of that are the design.** A magnet becomes a list of files, each served from our own loopback HTTP server at `/t/<infohash>/<index>/<name>`; mpv opens that URL … → [torrents-core](../../docs/rules/torrents-core.md)
- **A torrent arrives three ways, and a `.torrent` file is not a file the player opens.** A magnet is pasted, a `.torrent` URL is pasted, and a `.torrent` **file** is dropped or picked — and only the first two fit a box that takes … → [torrents-core](../../docs/rules/torrents-core.md)
- **A freshly added torrent is not ready to be touched, and the order of the three steps is load-bearing**: `add_torrent` returns while the torrent is still `Initializing` (checking and allocating files on disk, which scales with … → [torrents-core](../../docs/rules/torrents-core.md)
- **Seeding is off by default, and the switch has to be able to stop it *now*.** In Germany and several other jurisdictions the exposure from uploading is categorically worse than from downloading, so a player must not opt anyone … → [torrents-core](../../docs/rules/torrents-core.md)
- **A finished episode leaves no trace, so the torrent store keeps its own record of them.** `frameplayer.positions` *deletes* an entry past `FINISHED_FRACTION` — which is what takes a film out of "continue watching" — so a watched … → [torrents-core](../../docs/rules/torrents-core.md)
- **Once the viewer can choose where torrents are written, "the torrent directory" is three questions, and answering them with one path is how a video player deletes somebody's films.** `Dirs` keeps them apart. → [torrents-core](../../docs/rules/torrents-core.md)
- **The torrent's folder carries its info hash, and disk is the source of truth for storage.** `ManagedTorrentOptions.output_folder` is `pub(crate)`, so a folder named after the torrent alone could never be *read back* and the … → [torrents-core](../../docs/rules/torrents-core.md)
- **A deleted torrent used to come back, one per restart, as an empty copy of itself.** `torrent_forget` removed the directory and our record, but took the torrent out of librqbit's own store only when it was in `inner.torrents` — … → [torrents-core](../../docs/rules/torrents-core.md)
- **Deleting a torrent must not be able to fail quietly, and three separate things made it able to.** Reported as "«удалить всё» пишет «освобождено 0 МБ», раздача остаётся, у неё по-прежнему 14 ГБ", and the diagnosis cost a day … → [torrents-core](../../docs/rules/torrents-core.md)
