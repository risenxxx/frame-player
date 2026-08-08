# Casting to a television

The player hands the television a **URL and lets it fetch the file itself**. Our
machine serves bytes over the local network; the TV decodes. Nothing is
re-encoded into a video stream and nothing is mirrored.

That is the only model compatible with how the player renders — mpv draws into a
child view, so there are no frames to capture and no OS casting API that could
be fed from it — and it happens to be the model that produces the best result
anyway: the television uses its own decoder, at its own bitrate, with its own
HDR pipeline.

While a session runs, the player window is a **remote control**: mpv sits paused
on the file it handed over, so tracks, chapters, duration and the hover
storyboard keep feeding the interface, and disconnecting is a seek to the
television's position plus unpause rather than a reopen.

## Two transports, and why both

| | Google Cast | DLNA (UPnP AV) |
|---|---|---|
| Reaches | Chromecast, Android TV, sets with Chromecast built in | Most smart TVs, media renderers |
| Discovery | mDNS `_googlecast._tcp` | SSDP |
| Control | CASTV2 (protobuf over TLS, port 8009) | SOAP over HTTP |
| Plays | what the receiver's app can decode | what the **television** can decode |
| Preparation | often required | usually none |

The second row of that table is the whole story. A Cast receiver is an
application: on the progressive path it hands the URL to the platform player,
but the container and codec set it accepts is narrower than the set the same
television can play, and anything outside it has to be repacked first. A DLNA
renderer *is* the television's own player, so a release it already handles plays
untouched — a 4K HEVC HDR MKV with Dolby audio goes over as it is, seeking
included, with no repacking, no cache copy and no wait.

Neither is redundant. A Chromecast dongle has no DLNA; plenty of renderers are
audio-only; and the two are announced by different protocols, so a device may be
found by one and not the other.

## How the transport is chosen

Auto prefers DLNA whenever the renderer's own format list (`GetProtocolInfo`)
contains the file's container and the source is a local file. Otherwise Cast.
The viewer can pin either per device — the choice is remembered under the most
stable identifier the device has, with its name and model as a fallback, because
an address is a DHCP lease and would lose the setting silently.

Two rules keep the picker honest:

**It reasons about the resolved source, not the open file.** A completed torrent
is a loopback URL to the player and a file on disk to the cast pipeline; asking
the URL made "is this a network source?" true and auto answered Chromecast for a
release that in fact goes over DLNA untouched — the row contradicting what
clicking it does. A UI that predicts an action has to run the same resolution
the action runs, or it is a second implementation of the decision.

**Each row states a consequence, not a protocol.** "Plays as it is", "Prepared
before it starts", "Streamed · stereo sound", "Still downloading: needs the
whole file" — computed once per opening from the file's verdict. Protocol names
live under a per-device gear for whoever went looking for them. And a row does
not pass judgement while it is still discovering: SSDP answers seconds after
mDNS does, so a device whose DLNA half has not arrived is not a device without
DLNA.

## The Cast ladder

Applied per file from what mpv already reports — container, video format, audio
codec, channel count:

1. **Direct play** — MP4/WebM with a receiver-decodable codec set. Serve the
   original. Zero preparation, native Range seeking, zero disk.
2. **Prepare** — remux into MP4 with `+faststart`, video always stream-copied,
   audio copied when the receiver can take it and re-encoded otherwise. A
   container-only remux of a film is ~10 s; with an audio transcode ~30 s.
3. **Refuse, naming the offender** — incompatible *video*. There is no video
   transcode rung: it is the one that would be slower than realtime on 4K and
   needs hardware encoder selection.

Measured on a 120 s 1080p test file with video stream-copied throughout:

| operation | speed | CPU per 120 s of content |
|---|---|---|
| DTS 5.1 → E-AC-3 640k → MP4 | 276× realtime | 0.47 s |
| DTS 5.1 → Opus 5.1 384k → MP4 | 73× | 1.84 s |
| DTS 5.1 → AAC 5.1 384k → MP4 | 43× | 2.97 s |
| remux only, `-c copy` MKV → MP4 | 970× (I/O-bound) | 0.05 s |

Those numbers decided the seeking strategy. **The whole file is prepared before
playback starts**, because at 276× the wait is seconds, and after it the server
is a plain Range server on a real file: no state machine, no time-to-byte
mapping, native seeking, and the TV may buffer as aggressively as it likes.
Prepared copies are cached (keyed by path, size, mtime, track and rung) and
LRU-pruned, so re-casting the same film is instant.

Rejected with reasons: **restart-the-encode-at-the-offset** makes every seek a
visible reload; **serve-while-growing** cannot work with `+faststart`, whose
index is written by rewriting the file at the end.

## HLS, and why it stopped being a choice

Segmented streaming was built for the one case the prepare rung cannot serve — a
source that is not fully on disk. Then it was measured against a real receiver,
one rendition per cell:

| container | video | audio | result |
|---|---|---|---|
| TS | H.264 | AAC stereo | plays |
| fMP4 | H.264 | AAC stereo | plays |
| TS | H.264 | AAC 5.1 | starts, then the receiver app dies |
| TS / fMP4 | H.264 | E-AC-3 | refused |
| TS | H.264 | AC-3 | refused |
| fMP4 | HEVC | AAC | refused |
| either, with a master playlist declaring `CODECS` | HEVC or E-AC-3 | — | refused **before fetching a segment** |

The last row is the one that settles it: with the codecs declared in text, the
refusal arrives before any media is fetched, so it is the codec being turned
down and not the packaging. The explanation is architectural — a progressive
file reaches the television's own decoder, while HLS goes through the receiver's
browser pipeline, where the codec set is the browser's and passthrough of a
Dolby bitstream has no route at all.

So HLS carries H.264 with stereo. Both halves of the job it was built for went
elsewhere (DLNA carries a release untouched; a direct-play file streams over
Cast as it is), which left it strictly worse than the default on everything it
could carry and unable to carry the rest. It is no longer offered as a setting;
the code path remains for a receiver with no DLNA and a source that is not
complete, and as a debugging knob.

Two things learned while getting it to work at all, both worth keeping:

**`hlsSegmentFormat` is the format of an HLS *audio* segment.** Sending it
beside a muxed A/V stream made the receiver reject the load outright — a bare
failure with no reason, after it had already fetched the playlist, the
initialisation segment and segment zero. Only `hlsVideoSegmentFormat` belongs
there. The bug hid behind an unrelated one for two rounds, because every fMP4
test was also HEVC.

**HEVC in fMP4 must be tagged `hvc1`.** Without the tag ffmpeg writes `hev1`,
which Cast, Safari and every Apple-HLS consumer refuse. The progressive path had
always passed the tag; the segmenter had not, and the two renditions of one file
must be built from one set of decisions.

## What a DLNA renderer will and will not do

Read from the device's own service descriptions rather than discovered by
failing. On the set the transport was developed against:

**It plays what the television plays.** Its `GetProtocolInfo` lists 32 MIME
types including `video/x-matroska`; a 4K HEVC Main-10 HDR MKV with E-AC-3 5.1
plays with no preparation of any kind.

**Seekability is declared in the metadata, not in HTTP headers.** With a bare
`<res>` element the television greys out its own seek buttons before fetching a
byte and answers a sender `Seek` with "not available". With the DLNA flags in
`protocolInfo`, plus `size` and `duration`, it reads the container's index
itself and range-requests the offset. An HTTP-level capability advertisement
does not reach a decision made at the control layer.

**`Stop` goes before every `SetAVTransportURI`.** A renderer that is already
playing refuses a new URI with UPnP 701, and it will be playing more often than
not — the previous session, a session that did not end cleanly, another sender.

**Accepting a URI is not being ready for a command.** The first `Play` after a
set is answered by a dropped connection while the television brings its player
up; retry rather than believe it.

**An optimistic state must be armed before the round trip, not after.** A SOAP
call takes a few hundred milliseconds and the interface polls twice a second:
set the state afterwards and a poll lands in the gap, reads the old value, and
the button flips to the new icon, back, and forward again.

**A paused renderer may refuse to seek** (UPnP 501) and keeps reporting the
position it had before the seek until playback resumes. The remedy is the
classic one — resume, seek, pause again — and the reported position has to be
outranked by the target for a while, or the knob springs back and a seek that
worked looks like it failed.

**Audio tracks are the television's to choose.** The renderer's AVTransport
declares no action for them — and the vendor extensions present for subtitles
and 3D are what make that conclusive rather than an omission. The file goes over
with every track in it and the set picks; the player says so instead of showing
a selection it cannot vouch for.

**Volume may be declared and not honoured.** One television answers `GetVolume`
with a valid `0` while playing at almost full, and refuses the action to another
client outright. A level is therefore trusted only once a non-zero one has been
seen; until then the slider and the mute button are disabled and the keys
explain where volume lives.

## When a transport fails

Auto can be wrong about DLNA in three ways that look identical from the sender:
the container is listed but the codec inside is not decodable, the device
advertises less than it can, or it is a renderer-only device that cannot take
this file. Rather than reason about which, **the failure is the evidence**: a
DLNA start that fails falls back to the Cast ladder, which names its own
refusals, and the container is remembered as refused for that device *for this
run* — a season stops retrying, a fresh launch tries again.

Three detectors feed it, and the third is the one that matters:

- the load call returns an error;
- the renderer stops before it ever played (for it, "finished" and "failed to
  start" are one state);
- **it fetched bytes and never started playing** — the failure that used to be
  perfectly silent, no error frame and no state change.

That last one only became detectable once the load stopped ending in an
optimistic "playing": claiming playback the renderer had not confirmed blinded
every check downstream. Zero fetches is deliberately *not* a fallback case —
nothing was read, so the file is not the problem, and the other transport is
served by the same blocked server. That stays the firewall message.

**Errors are distinct facts and get distinct sentences**: the device could not
be reached, the receiver app would not launch, the load was rejected, or the
load was accepted and nothing was ever fetched. The last is the firewall
signature, and the picker warns about the platform's network prompt before the
first bind rather than after the failure.

## The device check

Every question above costs an investigation when a cast fails on a device
nobody has tested. The picker therefore carries a per-device check that runs
them on demand and writes the answers down: our address on the device's subnet,
the Cast port and handshake, the renderer's description, its accepted formats,
whether it offers `Seek` at all, and what it says about volume — with a copy
button, because the report exists to be pasted into a bug report.

It deliberately stops short of launching anything on the television. The check
that would need it — hand the device a file and see whether it fetches — is the
firewall test, and it belongs behind an announced, separate action.

## Privacy

Casting is a path by which facts about a file leave the machine, and it is
treated as one:

- The server answers for **exactly one registered file** behind a random 128-bit
  token, never a directory. An unknown token is a 404 and the token dies with
  the session — every transport that borrows the server has to release it.
- A file under a private root is cast **with its name withheld**: the URL
  basename becomes the token plus the extension (the extension is load-bearing —
  receivers probe by it) and the load carries no title, so neither the wire nor
  the television's screen names it.
- Discovery is browse-only. The player never advertises a service of its own.
- Nothing opens at startup: discovery runs while the picker is open, the server
  binds when a device is picked, and both stop on disconnect.

## Networking

The server binds **the interface whose subnet contains the chosen device**,
computed from interface netmasks — never loopback, never all interfaces. That
handles multi-homed machines (VPN tunnels, virtual adapters, a disconnected
adapter with a self-assigned address) by construction rather than by guessing at
the default route.

SSDP has to *leave* by a named interface for the same reason, and that one bites
only on some machines: the outgoing interface for a multicast datagram is chosen
by the routing table, and a machine with virtualisation or VPN adapters has
virtual interfaces that win it — the query leaves into a switch nobody is
listening on and discovery reports an empty network. One socket per usable
interface, with the multicast interface set explicitly, fixes it.

Worth knowing when discovery finds nothing on macOS: since macOS 15 a process
without Local Network permission has its multicast silently dropped, and the
system's own resolver is exempt. A command-line probe will report an empty
network whatever is on it, so anything discovery-shaped has to be measured from
inside the application.

## Deliberately not done

- **Pixel streaming / screen mirroring** — impossible with this embedding model
  and the wrong product.
- **Miracast** — the same, plus it needs Wi-Fi Direct and an encoder for the
  screen; no support on macOS at all.
- **AirPlay** — the transport is a URL handoff like the others and would be a
  day's work, but modern receivers gate everything behind pairing (SRP, HomeKit
  keys, an encrypted channel), there is no maintained Rust sender to build on,
  and the device class it would unlock that nothing else reaches is essentially
  the Apple TV. Considered, not planned.
- **A custom Cast receiver** — same hardware decoders, so no codec gain; it
  would buy a styled interface and codec probing at the cost of an app id and a
  hosted page.
- **DIAL** — launches pre-installed apps only, vendor-gated.
