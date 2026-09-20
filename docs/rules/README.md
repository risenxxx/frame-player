# The rule book

What is true of the code right now, and what breaks if it stops being true.
[`CLAUDE.md`](../../CLAUDE.md) in the repository root carries one line per rule —
enough to know a rule exists — and links here for the whole of it: the
identifiers, the measurements, and the failure the rule was written against.

These chapters answer **what**. The documents one level up in [`docs/`](../)
answer **why**: what was tried, what was measured, and which approaches are not
worth walking into twice. Where the two disagree, this wins.

| Chapter | The rules for |
|---|---|
| [mpv-playback.md](mpv-playback.md) | Driving libmpv: state mirrors, the seek contract, the playlist and the end of a file, chapters, picture geometry, the A–B loop, audio |
| [thumbnails.md](thumbnails.md) | The seekbar storyboard and every frame decoded outside playback: the budget, the colour space, which frame a hover must show |
| [tracks-and-subtitles.md](tracks-and-subtitles.md) | Choosing a track and remembering the choice, subtitle search and placement, closed captions, content languages |
| [torrents-core.md](torrents-core.md) | The session, adding a torrent, where the data lives, what may be deleted — and the macOS descriptor budget under it |
| [torrents-network.md](torrents-network.md) | The swarm: DHT, trackers, encryption, the proxy, port forwarding, and the preferences that rebuild the session |
| [torrents-playback.md](torrents-playback.md) | Playing one: the queue, the buffer map, the readouts, subtitles inside a release, replacing a torrent with its re-upload |
| [casting.md](casting.md) | Google Cast and DLNA: the ladders, the LAN server, the remote-control rules, what one television taught us |
| [sync.md](sync.md) | Watching together: the wire, the reconciler, readiness, what a room may know |
| [catalog.md](catalog.md) | The metadata proxy, the indexer, and what leaves this machine when somebody searches |
| [sources-and-privacy.md](sources-and-privacy.md) | What a source *is* against how it is reached, files arriving from the system, the watch history and the seven privacy enforcement points |
| [window.md](window.md) | The transparent window, the macOS title bar, fullscreen, the mini player, geometry, the idle and cursor rules |
| [ui-surfaces.md](ui-surfaces.md) | The settings sheet, the context menu, the start screen, tooltips, the OSD, and the arithmetic that places anything floating |
| [css.md](css.md) | The box model, the cascade between borrowed classes, and the rendering traps measured in both engines |
| [frontend.md](frontend.md) | Where state lives and which way it may depend, the split, hotkeys, localisation, what is worth a test |
| [build-and-release.md](build-and-release.md) | What ships inside the app, the LGPL/GPL obligations, signing on both platforms, the release workflow |

## Where a new rule goes

A finding earns a line here when breaking it would break the player; it belongs
one level up, in [`docs/`](../), when its value is saving the next
investigation. A rule added here earns its one-line form in
[`CLAUDE.md`](../../CLAUDE.md) at the same time — a chapter nothing points at is
a chapter nobody reads.

**The repository is public.** Nothing written here may name a host, an account,
a machine, a person or an address on somebody's network. Device behavior is
recorded by model and by what it did, never by whose device it was.
