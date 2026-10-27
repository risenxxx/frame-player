---
paths:
  - "src/lib/open.svelte.ts"
  - "src/lib/torrent.svelte.ts"
  - "src/lib/torrent/**"
  - "src/lib/stall.svelte.ts"
  - "src/lib/feed.ts"
  - "src-tauri/src/torrent/**"
  - "src-tauri/src/feed.rs"
  - "src/lib/components/TorrentPickDialog.svelte"
  - "src/lib/components/TorrentUpdateDialog.svelte"
  - "src/lib/components/FeedPickDialog.svelte"
  - "src/lib/components/LoadingOverlay.svelte"
---

# torrents-playback — the short form

The short form of [`docs/rules/torrents-playback.md`](../../docs/rules/torrents-playback.md): the modules this area is made of,
its rules one paragraph each, and the commands that exercise it. Claude Code loads this file
by itself when a file it covers is opened with Read/Edit/Write — not when one is read through
Bash, which is why `CLAUDE.md` says to read it before changing the area.

Each line is the rule; the chapter it links to is the whole of it.

## Modules

| Path | What lives there |
|---|---|
| `src/lib/open.svelte.ts` | How a source becomes a playing file: the link box, torrents (resolve, pick, the start-screen list, replacing one with its re-upload) and the indicator that says something slow is under way. **One module rather than the two it was planned as**, because the halves depend on each other by design — a magnet is pasted into the box that takes links, so `submitLink` dispatches into the torrent flow, and the torrent flow reports its failures back through `linkBox.torrentError`, which raises that same dialog for a dropped `.torrent` that has nowhere else to speak. A boundary between them would be a cycle with a hook across it. It composes `loadFiles`, the queue and the position store rather than reimplementing any of them, and takes the one thing it cannot reach — the file-dialog dimming flag — through `initOpen` |
| `src-tauri/src/feed.rs` + `src/lib/feed.ts` + `FeedPickDialog.svelte` | Torrent RSS feeds: `feed_read` (RSS/Atom in their tracker dialects), `feed_torrent` (fetch an item's `.torrent` into the metadata cache), and the pure matching that decides which item is a re-upload of which release. See "An RSS feed is a link to a release" below |
| `src/lib/torrent/` (re-exported by `src/lib/torrent.svelte.ts`) | The parts of the torrent client that follow playback: `poll.svelte.ts` (`trackTorrentPlayback` — the status and buffer-map polls, the finished-episode mark, the one-ahead prefetch), `files.ts` (the videos in queue order, the subtitles inside a release), `poster.ts` (the poster taken while the data is on disk), `replace.ts` (`updateTorrent` — handing data and positions to a re-upload), `feeds.ts` (reading a feed and resolving an item through Rust). The state, the session, the store and the disk list are in [torrents-core](torrents-core.md) |

## Rules

- **Opening from the history must give back the queue as well as the file.** A local file gets it for free (`loadFiles` builds the folder queue through `filesOpened`), but a torrent episode did not: `resolveTorrentFile` handed back … → [torrents-playback](../../docs/rules/torrents-playback.md)
- **A torrent's seek-back buffer is mpv's default, because its bytes are already on disk.** `backBufferFor` (source.ts) gives a torrent and a local file 50 MiB and only a real network stream 512 MiB, set on every `path` change — mpv applies it live. 512 MiB on a 4K torrent was ~580 MB within five minutes. → [torrents-playback](../../docs/rules/torrents-playback.md)
- **A torrent stalling mid-playback used to be completely silent.** `opening` is raised in `beforeLoad` and cleared on `file-loaded`, so the loading overlay — the one thing that reports peers and rate — is gone by the time playback … → [torrents-playback](../../docs/rules/torrents-playback.md)
- **A torrent cannot gain an episode, so replacing one is a first-class action.** The info hash covers the file list and every piece hash — verified by building two torrents from the same folder and adding a file, which changes it … → [torrents-playback](../../docs/rules/torrents-playback.md)
- **An RSS feed is a link to a *release*, and that is what makes discovery possible without becoming a background client.** Many trackers publish a feed per release whose next item is the re-upload with the next episode, so a feed … → [torrents-playback](../../docs/rules/torrents-playback.md)
- **The buffer map needed no fork of librqbit, and finding that out was the whole job.** `with_chunk_tracker` is `pub(crate)`, which reads as "patch required" — but `Api::api_dump_haves` is public and hands out the same bitmap, as … → [torrents-playback](../../docs/rules/torrents-playback.md)
- **Previews before a file is complete are per-position, not per-file.** `thumb_get(exact)` opens its own decode session, so single frames need no storyboard — but the background grid pass walks a file start to end and would decode … → [torrents-playback](../../docs/rules/torrents-playback.md)
- **Three things a torrent gains once its file is on disk, and one it cannot.** A torrent routinely **ships its own subtitles** (Sintel carries ten) — `sub-auto` finds nothing, since there is no local file to look beside, so they … → [torrents-playback](../../docs/rules/torrents-playback.md)
