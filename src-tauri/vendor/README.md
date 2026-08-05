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

## Maintenance

`[patch.crates-io]` overrides *every* version of these crates in the
dependency graph. If a future librqbit release requires a newer
librqbit-dht/tracker-comms, the build fails loudly with a version conflict —
re-vendor the new version and re-apply the (small, commented) patches, or drop
the patch entry if upstream has fixed the issue by then. Each patch site is
marked with a comment explaining itself; diff against the same version on
crates.io to see the exact changes.
