# Torrent streaming

A magnet link becomes a list of files; each file is served from a loopback HTTP
server the player runs, and mpv opens that URL like any other stream. The
television, when casting, is served the same bytes from the LAN server instead.

The point of the design is that **reading is what drives downloading**. mpv's
Range requests tell the torrent client where the viewer is, and the client
prioritises pieces around it. Nothing is fetched speculatively, and nothing at
all is fetched until a file is actually opened.

## What the design costs and buys

**A torrent holding a season is a playlist**, and it costs nothing to offer as
one: the queue is a list of URLs, and a URL is only a string. Opening episode
three downloads episode three.

**The session is lazy.** The torrent client starts on the first magnet, not at
application startup. A player that joins the DHT when you launch it is not one
people keep installed.

**Seeding is off by default**, using the client's compile-time upload-disable:
no bitfield, no `have` messages, piece requests refused, peers dropped once a
file is complete. In several jurisdictions the exposure from uploading is
categorically worse than from downloading, and a player must not opt anyone into
it silently. What it does *not* do — and the setting says so — is make the
client invisible: finding peers means announcing to trackers and the DHT.

**The identity of a source is `torrent:<infohash>/<index>`, never the URL.** The
loopback port changes every run, so a key taken from the transport would forget
the film every session. Watch positions, chosen tracks, titles and posters are
all filed under that identity.

**What is remembered is a magnet built from the infohash**, not the path a
`.torrent` file came from: the file is thrown away after use and a tracker page
can go down, while the hash plus the cached metadata reopens the torrent with no
lookup at all.

## The parts that are not obvious

**The folder is the infohash.** The client's own output-folder field cannot be
read back, so a folder named after the torrent could never be mapped back to it.
Naming the folder after the hash also removes every question a torrent name
raises as a path — length, separators, two releases sharing a name — and makes
deletion cheap to validate: a legal folder is exactly 40 hex characters.

**Metadata is cached, so a magnet resolves once ever.** Without it, reopening a
season that is already on disk goes back to the DHT — measured at 1.5–10 s.
The metadata *is* the torrent file, so it is written beside the data and replayed
on the next open: 1.52 s → 4.45 ms, and a re-read of an already-fetched region
1.20 s → 0.8 ms.

**Piece verification is skipped through fast-resume**, which is worth the
persistence it requires. Without it the client hashes the *whole* torrent on
every open — holes included, since files are created at full length — so
reopening a 7.5 GB season with 46 MB fetched cost 3.2 s, growing with the
torrent rather than with what was watched. With it: 2.4 ms. The one real risk
that persistence brings is that a torrent recorded as running is restored
running, which would talk to peers for something nobody asked to watch; the
session therefore pauses everything it restores, and has to wait out the
initialising state first, because a client refuses to pause a torrent in it.

**A freshly added torrent is not ready to be touched**, and the order of the
steps is load-bearing: wait for initialisation, select the file, unpause, wait
again. Selection is refused outright while initialising, and streaming accepts
only a paused or live torrent. On a small release the window is invisible; on a
nine-episode season it swallowed the whole of playback. The server therefore
answers **503** rather than 404 for "not ready", because "the file does not
exist" and "the file is not ready yet" are different facts and a torrent that
plays on small releases and fails on large ones is otherwise indistinguishable
from a broken URL.

**Size on disk is measured in allocated blocks, not file length.** Files are
created at full size and the filesystem makes them sparse, so a season with one
episode part-fetched reports 7.45 GB by length and 142 MB by blocks. Reporting
the length would tell the viewer to delete something to reclaim space that was
never taken. (Windows is the exception: the client does not mark files sparse
there, so length is all there is.)

**The buffered map needs no fork.** The piece bitmap is not public API, but the
client's own debugging dump exposes the same bits as a formatted string; parsing
its bracketed tail is undignified rather than fragile, and it is pinned by the
lockfile and covered by a test. Intersecting the pieces with the file's own byte
span matters: the first and last piece of a file are shared with its neighbours,
so without it the seekbar would shade buffer belonging to another episode.

## What a partially downloaded file can be used for

This is the question that decides several features, and the answer has one hard
edge: **the holes in a sparse file read back as zeros, silently.** Nothing that
reads the file off the disk can tell "not here yet" from "black".

So:

| Wants the file | Works while downloading? |
|---|---|
| Playback | yes — the stream blocks on a missing piece instead of reading the disk |
| Hover preview at a position | yes, if that position is inside a buffered range |
| Background storyboard | no — it walks the file start to end and would decode straight through a hole |
| Poster for the start screen | only if captured *while playing*, from a buffered range |
| Casting to a TV | yes over DLNA, and over Cast when the file needs no repacking |
| Preparing a compatible copy | no, permanently: half a film remuxed is half a film |

That last line is worth being explicit about, because it looks like a limitation
to lift later and is not. No amount of progress changes it; only completion
does.

## Three things a file gains when it completes

**Its subtitles become reachable.** A release routinely ships subtitle files
beside the video, and automatic detection finds nothing because there is no
local file to look beside. They are attached from the server instead, matched by
name — a season carries one set per episode, and attaching all ninety would make
the menu useless.

**The next episode is prefetched** — one ahead and no further, which is the line
between "the season does not stop between entries" and a background client
filling the disk with what may never be watched.

**The seekbar storyboard runs**, because the data is now an ordinary file.
Since a file completes *during* playback, that has to be retryable rather than
decided once when the file opened.

## Casting a torrent

A complete file is an ordinary file and takes every rung. An incomplete one
streams, through the LAN server, from the torrent client's blocking reads — so
the television's Range requests become piece priority exactly as mpv's do. It is
served through the cast server's one-source-behind-a-token rule rather than by
exposing the loopback server to the network, which would publish every torrent
in the session under a guessable path.

There are two doors and neither is a preference:

- **DLNA** — the wide one, since torrents are MKV. The renderer lists the
  container, the release goes over untouched.
- **Cast** — the narrow one: only a file that needs no repacking at all, because
  then there is nothing to prepare and the receiver's own Range fetching is
  enough.

Everything else waits for the file, and says so. Before the load, a lead of a
few tens of megabytes is built with the figure shown, because the receiver's
tolerance for a blocked read is the one thing about this rung that cannot be
known from the sender side.

## Two clients patched, for reasons worth knowing

Both are one-line-ish fixes vendored with the source, because neither exists in
a compatible upstream release:

**The DHT must not die on a UDP receive error.** Upstream stops its reader on
the first one — permanently, for the life of the session. Windows produces such
errors in normal operation, where Unix does not: a connection-reset when any of
the thousands of contacted nodes turns out dead and its ICMP unreachable comes
back, and a message-size error when a node's datagram exceeds the read buffer.
Both are errors about one datagram, not about the socket. Left alone, the DHT
died within minutes on Windows and the player found no peers at all.

**A tracker announce must not replace the URL's own query.** Building the
announce with a fresh query string drops what the URL already carried — the
marker some trackers require, and any passkey — which comes back as a 403. With
the DHT also dead, that left Windows with no peer source whatever, which read as
a network problem and was not.

A third leg was needed for the same failure: announces must carry a real
listening port, or trackers refuse them outright.

## Known limits

**Cold seeks cost downloading.** A jump into a region nobody has fetched waits
for the swarm — 1.3 s on a healthy one, minutes on a weak one. The player says
why: peers and rate under the label, because "no peers" and "slow swarm" are
different problems that must not look the same.

**Encryption is not implemented.** Peers on some networks cut plaintext
handshakes mid-stream, which shows up as handshake noise in logs even when
everything works.

**Discovery of new episodes is out of scope.** A torrent cannot gain a file —
adding one changes the infohash and publishes a different torrent — so the
player supports *replacing* a torrent with its successor, moving the data and
re-keying watch positions by file name. Learning that a successor exists would
need feed polling, which is the resident BitTorrent client this application
declines to be.
