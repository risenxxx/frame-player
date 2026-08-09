# Vendored librqbit crates

Unmodified copies of two crates from crates.io, each carrying one small fix
that upstream does not have in any release compatible with librqbit 8.1.1.
Wired in via `[patch.crates-io]` in `../Cargo.toml`, so Cargo uses these
directories instead of the registry copies — same versions, different source.

## librqbit-dht 5.3.1 — tolerate UDP recv errors (Windows)

`src/dht.rs`, the `framer` reader loop. Upstream propagates any
`recv_from` error with `?`, which kills the whole DHT worker permanently
("framer quit"). On Windows recv errors are routine and say nothing about the
socket: `WSAECONNRESET` (10054) arrives when a previously contacted node
answered with ICMP "port unreachable" — a DHT talks to dead nodes constantly —
and `WSAEMSGSIZE` (10040) when a node sends a datagram larger than the read
buffer (Unix silently truncates; Windows errors). Both were caught live on a
dev machine; the stock DHT died 89 s into a session, taking magnet resolution
and peer discovery with it for the rest of the app's life. The patch logs and
continues, and only bails after 100 *consecutive* errors (a socket that is
genuinely dead). This mirrors what librqbit's own UDP tracker client already
does with recv errors. Not fixed upstream as of 9.0.0-rc.0.

## librqbit-tracker-comms 3.0.0 — keep the announce URL's own query

`src/tracker_comms.rs`, `task_single_tracker_monitor_http`. Upstream builds
the announce with `Url::set_query`, which REPLACES the query string — so a
tracker URL that carries one (rutracker's `/ann?magnet`, private trackers'
`/ann?ak=<passkey>`) loses it and the tracker refuses the announce (rutracker
answers 403). The patch remembers the original query and re-appends it to
every request. Upstream fixed this in the 9.x line only; this is that fix
backported.

## librqbit-tracker-comms 3.0.0 — send a User-Agent

`src/tracker_comms.rs`, `tracker_one_request_http`. librqbit announces through
a default `reqwest::Client`, which sends **no `User-Agent` header at all**, and
the WAF several trackers sit behind refuses such a request outright: measured
against rutracker with a byte-identical query, **403 without the header and 200
with it**, and the value does not matter (a single letter passes) — only its
presence does. The patch sets `rqbit`, matching what the peer id already says.

## librqbit-tracker-comms 3.0.0 — `complete`/`incomplete` are optional

`src/tracker_comms_http.rs`, `TrackerResponse`. Upstream requires both fields.
BEP 3 lists them, but they are seeder/leecher *statistics* and real trackers
omit them — rutracker answers `d8:intervali3595e12:min intervali3595e5:peers
180:…e` and nothing more. Required, the whole response fails to deserialize and
**every peer in it is discarded**, leaving the DHT as the sole peer source. The
failure is invisible from outside: the announce is a clean 200, the error is one
level below it, and the torrent merely sits at "connecting to the swarm" with
twenty seeders showing on the tracker page. Nothing reads either field.

Both of these were found together: a rutracker release with 24 seeders that
would not download at all, next to another that did — the second one was living
on DHT peers alone. `FP_TEST_SWARM=<magnet> cargo test --lib swarm_probe` is the
diagnostic that separated the two (peers seen vs peers connected vs bytes).

## Maintenance

`[patch.crates-io]` overrides *every* version of these crates in the
dependency graph. If a future librqbit release requires a newer
librqbit-dht/tracker-comms, the build fails loudly with a version conflict —
re-vendor the new version and re-apply the (small, commented) patches, or drop
the patch entry if upstream has fixed the issue by then. Each patch site is
marked with a comment explaining itself; diff against the same version on
crates.io to see the exact changes.
