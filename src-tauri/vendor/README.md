# Vendored librqbit crates

Copies of two crates from crates.io, each carrying changes upstream does not
have in any release. Wired in via `[patch.crates-io]` in `../Cargo.toml`, so
Cargo uses these directories instead of the registry copies — same versions,
different source.

Both are **Apache-2.0** (`../../licenses/spdx/Apache-2.0.txt`), which is
one-way compatible with this application's GPL-3.0-or-later. Their files are
**modified**, as §4(b) requires to be said out loud; every change is marked with
a comment explaining itself, and `diff` against the same version on crates.io
shows the whole of it.

## librqbit 9.0.1 — Message Stream Encryption (MSE/PE)

`src/mse/` (new, ~1200 lines) plus ~500 lines across `stream_connect.rs`,
`peer_connection.rs` and `session.rs`. **This is somebody else's work, taken
whole**: [ikatson/rqbit#633](https://github.com/ikatson/rqbit/pull/633), opened
by @lingdiansr in August 2026 and not merged at the time of writing.

Why it is here rather than waited for: an ordinary BitTorrent handshake opens
with the literal bytes `\x13BitTorrent protocol`, which is what a middlebox
filtering on the protocol matches — and cutting the connection immediately after
it is what DPI on several ISPs measurably does (see the swarm probe in
`torrent.rs`: a peer that completes a TCP connection and then dies mid-stream).
That failure is invisible from inside the player: peers are *seen*, some even
connect, and nothing downloads. qBittorrent works on the same networks, and the
difference is this.

What the patch does: DH-768 key exchange, RC4 stream encryption, SHA-1 key
derivation, on both the initiator and the responder side, with a three-state
mode (`Disabled`/`Enabled`/`Forced`) and a plaintext redial when a peer will not
encrypt. The cryptography itself is **not** in the patch — the modular
exponentiation comes from `crypto-bigint` and the cipher from RustCrypto's `rc4`
— so what is hand-written is the handshake framing, where a mistake costs a
connection rather than a secret.

Four things were checked before taking it, in this order, because each one could
have ended it:

1. **It applies to 9.0.1 unchanged.** Every source hunk of the PR — which is
   written against a later `main` — lands with no conflict; only the manifest
   differs, because the packaged one names concrete versions where the workspace
   one names the workspace. The two dependency lines are added by hand.
2. **It compiles** with this player's feature set (`rust-tls`, `disable-upload`).
3. **Its own 16 tests pass**, including DH-768 against external vectors, a
   duplex handshake preserving its payload, the responder after a plaintext
   sniff, and every fallback path.
4. **It interoperates with the real world**, which is the only one of the four
   that could not be answered by reading. With the strictest mode — every peer
   that will not encrypt is dropped — `FP_TEST_MAGNET=1 FP_TEST_MSE=only cargo
   test --lib sintel_smoke` resolved the magnet **from the swarm** in 840 ms and
   streamed at 2.2 MB/s from 24 peers. Both halves matter: the resolve proves
   the metadata fetch goes over MSE too, and it is the step this player was
   reported hanging on.

`webui/` is **not** copied. It is 564 KB of an npm project behind the `webui`
feature, which is off here (`default-features = false`), and `build.rs` would
run `npm install` if it were ever turned on. Re-vendoring means copying the
crate from the registry again, minus that directory.

**Drop this the day #633 merges.** It is the largest thing in this repository
that belongs to somebody else, and a bump of librqbit means re-applying ~1700
lines rather than reading a diff.

## librqbit-dht 9.0.1 — tolerate UDP recv errors (Windows)

`src/dht.rs`, the `framer` reader loop. Upstream propagates any `recv_from`
error with `?`, which kills the whole DHT worker permanently ("framer quit").
On Windows recv errors are routine and say nothing about the socket:
`WSAECONNRESET` (10054) arrives when a previously contacted node answered with
ICMP "port unreachable" — a DHT talks to dead nodes constantly — and
`WSAEMSGSIZE` (10040) when a node sends a datagram larger than the read buffer
(Unix silently truncates; Windows errors). Both were caught live on a dev
machine; the stock DHT died 89 s into a session, taking magnet resolution and
peer discovery with it for the rest of the app's life. The patch logs and
continues, and only bails after 100 *consecutive* errors (a socket that is
genuinely dead). This mirrors what librqbit's own UDP tracker client already
does with recv errors. **Still unfixed in 9.0.1** — read rather than assumed:
the line is `socket.recv_from(&mut buf).await.map_err(Error::Recv)?`.

## What used to be here, and why it is not

`librqbit-tracker-comms 3.0.0` carried three patches, and **all three are in the
9.x line**, so the bump from librqbit 8.1.1 took that whole crate off this list
and back to the registry. They are worth remembering, because each one was
invisible from outside — a clean HTTP exchange with the error one level below
it, and a torrent that merely said "connecting to the swarm":

- **The announce URL's own query was replaced.** Upstream built the request with
  `Url::set_query`, so a tracker URL carrying one (`/ann?magnet`, a private
  tracker's `/ann?ak=<passkey>`) lost it and the announce was refused with 403.
  9.x appends the original query instead.
- **No `User-Agent` was sent at all.** reqwest sends none by default, and the WAF
  several trackers sit behind refuses such a request: measured with a
  byte-identical query, **403 without the header and 200 with it**, and the value
  did not matter — only its presence. In 9.x the session builds the HTTP client
  with `.user_agent(client_name_and_version)`, and that string is a
  `SessionOptions` field.
- **`complete`/`incomplete` were required fields.** They are seeder/leecher
  statistics and real trackers omit them, so the whole response failed to
  deserialize and **every peer in it was discarded**. 9.x marks both
  `#[serde(default)]`.

Both remaining diagnoses live in `FP_TEST_SWARM=<magnet> cargo test --lib
swarm_probe`, which answers the question these bugs all hid behind: peers *seen*
against peers *live* against bytes.

## Maintenance

`[patch.crates-io]` overrides *every* version of these crates in the dependency
graph, so a librqbit release that wants a different version of either fails
loudly with a conflict rather than quietly using the registry copy. Re-vendor
the new version and re-apply the changes, or drop the entry if upstream has
fixed the thing by then — which is the expected end of both of these, and the
one for librqbit is worth checking on every bump, because ~1700 lines of
somebody else's cryptography is not a thing to carry a day longer than needed.

The two are very different to maintain and should not be treated alike. The DHT
change is a dozen lines around one `recv_from` and diffing it takes a minute.
The MSE patch is a whole feature: re-applying it means fetching the PR again
(`gh pr diff 633 --repo ikatson/rqbit`), filtering it to `crates/librqbit/`,
applying with `-p3`, adding the two dependency lines, and then running the four
checks listed above — the fourth one especially, since the only thing that can
say whether the handshake still interoperates is a real swarm.
