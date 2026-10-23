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
is in `CLAUDE.md`). What follows is the spike as it ran.*

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

## To research: tracks over HLS (option B)

**What option A found.** The first attempt was one MP4 carrying every audio
track and every text subtitle track as `mov_text`, switched on the receiver
through AVFoundation's selection groups. Locally AVFoundation lists such a copy
correctly — one audible group, and each subtitle twice in the legible group
(the track and a "forced" twin of its own making). On the LG UR78006LK neither
half worked: the set played the first audio track whatever the Mac selected,
and showed no subtitle at all — including a file whose only subtitle track was
flagged default, and one flagged default+forced, each sent untouched. What
shipped instead is one chosen audio track per copy and no subtitles; an Apple
TV is still unmeasured on both counts.

The alternative, and the way streaming services reach an Apple TV, is HLS: a
master playlist with the video, each audio track as an `EXT-X-MEDIA
TYPE=AUDIO` rendition and each subtitle track as a WebVTT `TYPE=SUBTITLES`
rendition. What it could buy and what has to be measured before it is worth
building:

- **Whether the LG shows WebVTT at all.** It is the question everything else
  waits on: the set refused `mov_text` outright, and a subtitle rendition in HLS
  reaches a different part of its player. Measure one H.264 + AAC + WebVTT VOD
  rendition before building anything.
- **Whether a rendition choice reaches the set.** The LG ignored a selection
  made on the Mac for an MP4; HLS renditions are selected the same way, through
  `selectMediaOption`, so this may fail identically.
- **Styling and timing.** WebVTT keeps positioning that `mov_text` loses, and a
  subtitle rendition can be regenerated with a delay without touching the
  video — the copy cannot apply `sub-delay` at all today.
- **No copy on disk for the tracks.** Audio renditions are separate segments,
  so adding a track or a subtitle file is a new playlist entry rather than a
  new film-sized MP4. Measure the segmenting cost against the remux.
- **Receivers.** The Cast HLS rung measured H.264-only on its receiver; whether
  an AirPlay receiver (an Apple TV, and the LG this was built against) plays
  HEVC in fMP4 HLS, E-AC-3 renditions and WebVTT has to be measured per set.
- **A torrent still downloading.** HLS segments can be produced as pieces
  arrive, which a single MP4 copy cannot — the one path by which AirPlay could
  stream a torrent before it finishes (feature bit 4, `VideoHTTPLiveStreams`).
- **The existing segmenter.** `cast_hls_prepare` already writes a VOD rendition
  for Cast; the work is renditions per track and a master playlist, and the
  research is whether AVPlayer's external playback carries the selection to the
  set the way it does for an MP4.
