# AirPlay on the Mac, through the system

**Status: built and measured against one receiver** (an AirPlay 2 LG
television, model UR78006LK), through both the spike and the cast menu. The
rules that came out of it are in [`rules/casting.md`](rules/casting.md); this
file keeps how they were reached — the route, the unknowns and what answered
them. Not yet run against an Apple TV itself, nor against a receiver that
restricts access (unknown 6).

**macOS only, and that is the decision.** AirPlay is handed to the system:
Apple's own `AVPlayer` does the external playback and Apple's own route picker
chooses the television. Windows gets nothing, deliberately — see "Why not
Windows".

## The route

An `AVPlayer` with `allowsExternalPlayback = true`, playing the **same URL** the
other transports use (the LAN file server behind its random token), plus an
`AVRoutePickerView` for choosing the receiver. When a route is chosen, the Mac
stops decoding and the receiver fetches the URL itself — the same handoff as
DLNA and Cast, with everything below it done by the system: discovery, pairing,
PIN, HomeKit access control, every tvOS generation.

What is reused from the other two transports is unchanged: `serve_one_file` with
its token and Range handling, the prepare rung (`-c:v copy`, `hvc1` tag,
`+faststart`, E-AC-3 when the audio must change), `CastStatus`, the poll, the
fallback and the privacy rules. What the receiver decodes is what AVFoundation
decodes: MP4/M4V/MOV; H.264, HEVC tagged `hvc1`, Dolby Vision 5; AAC, AC-3,
E-AC-3, ALAC, FLAC in MP4. WebM, VP9 and Opus are refusals, not attempts.

## What is different from DLNA and Cast

| | DLNA / Cast | AirPlay via `AVPlayer` |
|---|---|---|
| Who finds the receivers | Us (mDNS, SSDP, a sweep) | The system; the picker lists them |
| Who chooses one | Our `CastMenu` row | The system picker, a native view |
| Who talks to it | Our client | `AVPlayer`, which we command |
| Transport order in auto | DLNA → Cast | A pick in the picker *is* the choice; no automatic order |

So AirPlay does not become a third half of a row in `Cast.tvs`. It is **one
row in the cast menu** that opens the system's list from the bar's cast button —
an invisible picker over that button, pressed — and a session that exists from
that click and becomes the television's once the route goes external. A set
that speaks DLNA as well is therefore listed twice, once as itself and once
inside the system's list; the two cannot be joined before the pick, since only
the fetch after it says which address was chosen.

## The unknowns, in order of how much hangs on them

1. **Does `AVPlayer` hand the URL over with no `AVPlayerView`?** On macOS the
   documented path is `AVPlayerView`'s own picker button. Whether a bare
   `AVPlayer` plus a standalone picker in our window does a URL handoff — not
   mirroring — is the one thing the whole plan rests on. If it does not, the
   fallback is hosting an `AVPlayerView` in the window, and if that does not
   either, the plan is dropped, not worked around.
2. **Placing the picker.** It is a native view and cannot live inside the HTML
   panel. Options, in order of preference: a real, visible `NSView` laid over the
   web view at the button's rectangle (position and clicks decided natively); or
   a hidden picker whose inner button is pressed programmatically — a trick that
   depends on its private view hierarchy and is a last resort.
3. **Detecting the route.** `AVPlayer.isExternalPlaybackActive` and its KVO
   notification say a session is running; the receiver's name is available from
   the player's route description.
4. **Control.** `rate` for play/pause, `seek(to:)` for position, `volume` is the
   system's. Whether seeking during external playback is honoured is
   unmeasured; the DLNA rule applies (a seek that never lands is reported, not
   believed).
5. **Local Network permission and `NSBonjourServices`.** Not needed for the
   picker (the system browses), needed for our file server as it is today.
6. **Does the first session need the Mac's AirPlay settings on?** A receiver
   that restricts access to its own home is the system's refusal to show, not
   ours to explain.

## The spike

*Kept as the self-test `FP_AIRPLAY_PLAY=<file>`, which now drives a session
through the shipping commands rather than through code of its own (the command
is in `.claude/rules/casting.md`). What follows is the spike as it ran.*

Written: `src-tauri/src/airplay.rs`, macOS only, off unless
`FP_AIRPLAY_PLAY=<file>` is set. It serves the file through `serve_one_file`
on the way out of the machine past any VPN (`net_route`'s `direct`, else the
system route's interface), creates an `AVPlayer` for that URL with external
playback allowed and **muted until a route is active**, pins an
`AVRoutePickerView` to the window's top-left corner, logs the picker's view
tree, and then one line per change of state (and one every ten seconds
otherwise). The LAN server now records every address that fetched from it
(`cast::server_peers`), and that is the answer to unknown 1: a fetch from the
receiver's address is a URL handoff, a fetch only from this Mac is the Mac
decoding. Once external playback is active the script unmutes, plays fifteen
seconds, pauses, resumes, seeks a minute ahead and stops.

```bash
FP_AIRPLAY_PLAY=/abs/path/to/file.mp4 bun run tauri dev 2>&1 | grep '\[airplay\]'
# then click the AirPlay glyph in the window's top-left corner and pick the TV
```

Use an MP4 with H.264 or HEVC (`hvc1`) — the spike does no preparation.

**Answered without a device** (one local run, macOS 26, no receiver chosen):

- The player opens the URL from our server (`Range: bytes=0-1`, then open-ended
  ranges, all 206) and plays; only this Mac fetched, as expected with no route.
- Unknown 2, half of it: the picker's only subview is `AVRoutePickerButton`, an
  **`NSButton` subclass** — so a hidden picker pressed with `performClick:` is
  reachable. **This is what shipped**, and the order above was reversed for a
  reason the plan did not foresee: the row lives in an HTML menu that closes on
  the click, so a visible native view there would have to follow the menu's
  every layout, while an invisible one only has to be where the row *was* for
  the moment the system's list opens. What is relied on is "its subview is a
  button", never the class name; if a macOS release changes that,
  `airplay_open` fails with `picker_unavailable` and the viewer is told the
  list could not be opened.
- `AVRouteDetector.multipleRoutesDetected` answers `true` on a network with
  AirPlay receivers, before anything is picked — enough to show or hide the
  button.

**Measured on a receiver** (two runs against one AirPlay 2 television, an
H.264/AAC MP4 served as it is):

- **Unknown 1 — yes, it is a URL handoff.** Once a route is picked,
  `isExternalPlaybackActive` turns true about three seconds later and the
  television itself fetches from our server (`Range: bytes=0-1`, then
  open-ended ranges, all 206); the Mac stops fetching. No `AVPlayerView`
  needed, and nothing new in `Info.plist`.
- **Unknown 4 — control works through the player.** `pause`/`play` reach the
  set within a second and `currentTime` stops and moves with it. `seekToTime`
  forward (43.8 → 103.8) and back (113.2 → 83.2) both land, each as a fresh
  `Range` request from the television at the new offset; `currentTime` reports
  the receiver's position afterwards.
- **A television with AirPlay switched off in its own settings says so on its
  screen and tells the Mac nothing**: the route is chosen, `external` stays
  false, no error, no fetch. The transport therefore needs a timeout of its own
  ("picked, never started") with a hint to check the television's AirPlay
  setting.
- A seek past the end of the item is the end of the item (`duration` 0,
  paused, not keeping up) — the spike's own mistake on a two-minute file, kept
  here because a seekbar can ask for exactly that.

**Still open:** unknown 3 (the receiver's name — not exposed by `AVPlayer` on
macOS; the address that fetches is known), unknown 6 (access-restricted
receivers), and what the set shows once the player is stopped.

## What was built

- **Rust**, `src-tauri/src/airplay.rs`: commands `airplay_open` (serve, create
  the player, press the picker), `airplay_replace` (a new item on the live
  route — the next episode, another audio track), `airplay_status` (the
  `CastStatus` fields plus `peer`, the address fetching), `airplay_control`,
  `airplay_disconnect`, `airplay_detect`/`airplay_routes` for the row. They
  exist on Windows too and refuse, so the handler list has no platform branch.
  A picker delegate (`routePickerViewDidEndPresentingRoutes:`) starts the
  `not_started` clock and hides the picker again.
- **The server** records the addresses that connect for the current
  registration (`cast::server_peers`), and the wire log under `FP_CAST_DEBUG`
  now prints the peer on every request — which is how "who is fetching" became
  a line rather than an inference, for all three transports.
- **Frontend**: `castOverAirPlay` in `cast.svelte.ts`, shaped like
  `castOverDlna`; the poll's handover (`airplayLive`), the `not_started` error,
  the name from a known row by address; `transportCommand()` instead of the
  two-way ternaries; `airplayVerdict` and `airplaySummary` for the row. The
  row in `CastMenu.svelte` is shown on macOS while the system sees any route.
  `playback.svelte.ts` needed nothing: `can.volume` follows `volumeAdjustable`
  and `can.trackChoice` is already "not DLNA".
- **Tests**: `cast.test.ts` pins the verdict's edits against the Cast ladder.
- **Privacy**: the **eighth** enforcement point (watching together had become
  the seventh meanwhile); `hidden` reaches the same token naming.

## Why not Windows

Apple publishes no AirPlay sender for Windows, so one would have to speak the
protocol itself: Apple's pairing and encrypted channel, known only from
third-party reverse-engineering work, with no specification to build against and
no licence for a third-party sender. Three things make that a poor trade here:

- **Cost.** The design needed ten answers that only a real device gives, the
  one path that works on current tvOS has a single independent implementation,
  and a closed protocol can change under us in any tvOS release.
- **Audience.** An Apple TV fed from a Windows computer is rare, and most
  televisions that speak AirPlay 2 also speak DLNA, which Windows already has.
- **Standing.** The protocol is proprietary and unlicensed for senders. Whether
  shipping a reimplementation carries legal exposure was **not** assessed by a
  lawyer and is not claimed either way; it is one more reason not to take on a
  project that is expensive and rarely used. Using the system's own API on the
  Mac does not extend to Windows, so this is not a way round it.

So the Windows build has no AirPlay, the casting page says so, and
`docs/casting.md` records why. Reopening it should begin with that legal
question answered, not with code.

A client was begun and removed before it shipped (protocol modules and tests,
none of it merged). Nothing of it remains in the tree; if the decision is ever
reopened it starts from the question above, not from that code.

## What is left

- An Apple TV itself, and a receiver set to require a code or to admit only
  its home (unknown 6) — what the system's list does then, and whether the
  4 s grace is enough for a code typed on the Mac.

Measured since, on the same set: **the end of a file and the queue's advance
work** — `ended` at the last known duration (the first attempt stuck there
because AVPlayer reports zero at the end), then `airplay_replace` put the
folder's next file on the same route twice running, from zero, with no list to
choose from again. **The television's own remote is reflected**: pause, play
and seeking on the set show up in the window's state within a poll. Its
held-button scan is the set's own and behaves like one — four-second steps,
slow to answer, running on after the button is let go until pause or the
opposite direction — with nothing sent from our side meanwhile. **A seek from the window while
paused holds**: paused at 13.4, sought to 99.7, played from 99.8. (An earlier
run that resumed near the pause point had been resumed from the set's remote
after its own scan, not from the window.)
- Whether AV1 plays on a receiver with the hardware for it.
- An Apple TV's answer to the two things the LG refused: a track selection
  made on the Mac, and `mov_text` subtitles.

## Tracks over HLS (option B) — built; measured on one set up to 1080p SDR

**What option A found.** The first attempt was one MP4 carrying every audio
track and every text subtitle track as `mov_text`, switched on the receiver
through AVFoundation's selection groups. Locally AVFoundation lists such a copy
correctly — one audible group, and each subtitle twice in the legible group
(the track and a "forced" twin of its own making). On the LG UR78006LK neither
half worked: the set played the first audio track whatever the Mac selected,
and showed no subtitle at all — including a file whose only subtitle track was
flagged default, and one flagged default+forced, each sent untouched. What
shipped instead is one chosen audio track per copy and no subtitles.

**What HLS does on the same set: both halves work.** One research run on the
LG UR78006LK, through the shipping AirPlay path (an `AVPlayer`, the system's
picker, the LAN server serving a flat directory behind one token):

- The rendition: H.264 video stream-copied into 6 s MPEG-TS segments, each
  audio track (AAC, copied) as its own audio-only TS playlist under an
  `EXT-X-MEDIA TYPE=AUDIO` group, each subtitle track as WebVTT segments cut on
  the video's segment boundaries under an `EXT-X-MEDIA TYPE=SUBTITLES` group,
  and a master playlist written by hand. ffmpeg's own HLS muxer could not do
  it in one run — it refuses several subtitle variants (`No streams to mux`) —
  so each piece was its own ffmpeg run and the subtitles were cut in code.
- **Every WebVTT segment carries `X-TIMESTAMP-MAP=MPEGTS:126000,LOCAL:00:00:00.000`**:
  ffmpeg's TS segments start at 1.4 s (`start: 1.400000`), and without the map
  the cues would sit 1.4 s early.
- The set fetched the master, the video and the default audio rendition's
  segments itself, and played. On `selectMediaOption` for the audible group it
  fetched the other rendition's playlist and segments **from the next segment
  on**, and the tone changed by ear; on the legible group it fetched that
  language's `.vtt` segments, and the subtitles appeared on screen, switched
  language, and went away on a nil selection. The server log is the objective
  half of the measurement: a selection that reaches the set is a fetch of the
  new rendition within a second.
- Locally AVFoundation lists an HLS rendition's subtitles once each — no forced
  twins, unlike `mov_text` — and honours `DEFAULT=YES` as the group's default
  option.

**What was built from it** is in [`rules/casting.md`](rules/casting.md): the
rendition cut in one ffmpeg run (`airplay_hls.rs`), the selection on the
receiver, the refusal read from the server's own log, a fresh fallback to the
MP4 copy, and a per-receiver memory of what was refused, with the choice of
mode under the AirPlay row's gear. Two things the research did not show and
the first build found: AVFoundation refuses an HDR variant that declares a
range but no `FRAME-RATE` (`-1002`), and a new item on the player whose
rendition the set refused does not wake the set — the item has to be taken
away first.

**Measured since, on the same set — HEVC, E-AC-3 and seeking all work.** A
second rendition built like a real 4K release: HEVC Main 10 (`hvc1`, 10-bit)
in fMP4 segments with an `EXT-X-MAP` init segment, and two E-AC-3 5.1 audio
renditions, also fMP4:

- **HEVC in fMP4 HLS plays on the LG** — unlike the Cast receiver, which
  refused HEVC-in-HLS outright. The master carried no `CODECS` attribute and
  the set did not need one.
- **E-AC-3 5.1 renditions switch** like the AAC ones: the set fetched the other
  rendition's init segment and media segments on `selectMediaOption`, and the
  tone changed by ear "without any loading".
- **WebVTT over fMP4 needs `X-TIMESTAMP-MAP=MPEGTS:0`**, not the TS run's
  126000: ffmpeg's fMP4 segments start at zero. Subtitles appeared, switched
  and went away exactly as with TS.
- **Seeking works, playing and paused**: forward while playing (28.6 → 88.6 s,
  the set fetched the segment holding 84–90 s at once), back while paused
  (102.9 → 12.9 s, it fetched that segment and held the frame), playback after
  that seek resumed from 13.8 s, and back while playing to the start. A seek
  past the end of the stream is the end of the item, and the set came back from
  it confused (a later seek reported, then the end again, then playback from
  an older position) — `castSeek` already clamps to the duration, and that is
  the rule to keep.

**4K and HDR: the LG refuses both over HLS.** A 4K HDR10 web release (HEVC
Main 10, PQ, two E-AC-3 5.1 tracks) cut into the same kind of rendition, then
bisected with short clips:

| Rendition | AVPlayer on the Mac | The LG |
|---|---|---|
| 720p SDR HEVC Main 10, fMP4 | plays | plays, tracks and subtitles switch |
| 1080p SDR HEVC, fMP4 | plays | plays |
| 1080p HDR10 HEVC (`VIDEO-RANGE=PQ`) | plays | fetches the master and nothing else; AirPlay's splash screen stays |
| 4K SDR HEVC (`RESOLUTION=3840x2160`) | plays | the same refusal |
| 4K HDR10, the release itself | plays | the same refusal |

- The refusal is **of the variant as declared**: offered one master with four
  variants of the same 4K SDR video, the set took only the one declaring
  1280x720 and skipped the one with the same bandwidth declaring 3840x2160.
  Given real 4K segments under that 720p label it played unsteadily and fell
  back to its home screen after a while — so lying about the resolution is not
  a way round it.
- **AVFoundation itself refuses an HDR rendition declared without
  `VIDEO-RANGE=PQ`** (`CoreMediaErrorDomain -12927`, before any receiver is
  involved), so the attribute is not optional for HDR; it is the set that turns
  it down.
- The same set plays a 4K HEVC release over the **MP4** copy, so the limit is
  its AirPlay HLS player's, not its decoder's.
- Cutting the rendition costs next to nothing when nothing is transcoded: ten
  minutes of the 4K release, video and both audio tracks, in about a second.

**What it decides.** On this set HLS — and with it switchable tracks and
subtitles — covers **1080p and below, SDR**. A 4K or HDR file stays on the MP4
copy (one chosen audio track, no subtitles), which is what plays it at its own
quality. An Apple TV may well take 4K HDR over HLS; that is unmeasured, and
the choice between the two paths should be made per receiver once there is a
second one to measure.

**Still to measure before it ships.**

- **ASS styling** survives WebVTT only partly (positioning, no fonts); a
  conversion from ASS has to be checked on a styled release.
- **The segmenting cost with a transcode**: an audio track in DTS or TrueHD
  becomes an E-AC-3 pass of its own, which is the slow part; copying is a
  second for ten minutes of 4K.
- **An Apple TV**, for all of the above.
- **A torrent still downloading.** HLS segments can be produced as pieces
  arrive, which a single MP4 copy cannot — the one path by which AirPlay could
  stream a torrent before it finishes (feature bit 4, `VideoHTTPLiveStreams`).
