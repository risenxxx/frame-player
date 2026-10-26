---
paths:
  - "src-tauri/src/cast.rs"
  - "src-tauri/src/dlna.rs"
  - "src-tauri/src/lan_sweep.rs"
  - "src-tauri/src/airplay*.rs"
  - "src/lib/cast.svelte.ts"
  - "src/lib/airplay-choice.ts"
  - "src/lib/components/Cast*.svelte"
---

# casting — the short form

The short form of [`docs/rules/casting.md`](../../docs/rules/casting.md): the modules this area is made of,
its rules one paragraph each, and the commands that exercise it. Claude Code loads this file
by itself when a file it covers is opened with Read/Edit/Write — not when one is read through
Bash, which is why `CLAUDE.md` says to read it before changing the area.

Each line is the rule; the chapter it links to is the whole of it.

## Modules

| Path | What lives there |
|---|---|
| `src-tauri/src/lan_sweep.rs` | Finding a television multicast does not reach: the sweep's address list (remembered hosts, then the private LAN subnets, tunnels and VM switches skipped) and the paced datagram send DLNA's unicast `M-SEARCH` uses. The Cast half (`probe_cast`: `:8009`, then `eureka_info`) lives in cast.rs |
| `src-tauri/src/cast.rs` | Casting to TV (Google Cast): mDNS browse (`mdns-sd`, on-demand), a hand-rolled CASTV2 client (TLS to the device's 8009, 4-byte-BE-framed protobuf envelopes with JSON payloads, tokio `select!` pump + dedicated reader task), and the LAN file server (one registered file behind a random token, bound to the interface on the device's subnet). Frontend half in `src/lib/cast.svelte.ts`; see "Casting" below and `casting.md` |
| `src-tauri/src/airplay.rs` | AirPlay, macOS only: an `AVPlayer` given the casting server's URL and AVKit's route picker, invisible over the bar's cast button and pressed from the menu's AirPlay row, so the system's own list chooses the television (there is no public API that lists or picks one). `airplay_open/replace/status/control/disconnect` mirror the other two transports' commands; `FP_AIRPLAY_PLAY=<file>` drives one session through them, logged. Frontend half in `castOverAirPlay` in `src/lib/cast.svelte.ts`. No Windows half, on purpose — see `docs/airplay.md` |
| `src-tauri/src/airplay_hls.rs` + `src/lib/airplay-choice.ts` | AirPlay's HLS rendition: every audio track and text subtitle in one ffmpeg run, WebVTT cut on the video's segments, the master declaring size, range and frame rate (`airplay_hls_prepare`); and the pure decisions around it — the mode (auto / tracks / original), which picture kinds each receiver has refused, when silence after the master is a refusal (`hlsRefused`), and the tracks plan. See the AirPlay bullet in [rules/casting](../../docs/rules/casting.md) |

## Rules

- **Casting (Google Cast) is a URL handoff, and while it runs the window is a remote control.** The TV fetches the file from a LAN HTTP server in cast.rs and decodes it itself; mpv stays **paused with the file loaded**, so tracks, … → [casting](../../docs/rules/casting.md)
- **Discovery is multicast, and a router may not carry multicast between the machine and the television — so every search is also a unicast sweep.** Wired/5 GHz vs 2.4 GHz is the case it is for (a precaution — not yet measured against such a router); `lan_sweep.rs` asks remembered addresses, then each LAN host, directly. → [casting](../../docs/rules/casting.md)
- **DLNA is the second way to a television, and the one that asks the least.** A UPnP MediaRenderer *is* the TV's own player, so a release it already decodes plays untouched — measured on the real set: a 6.3 GB 4K HEVC Main-10 … → [casting](../../docs/rules/casting.md)
- **Everything DLNA knows about a television has to be read off that television, because the next one is a different make.** Every rule above was measured against one set, and the failure mode of measuring against one device is … → [casting](../../docs/rules/casting.md)
- **The tolerance of the string-scanning XML lives in one function, and it is the price of not taking a parser.** `tag`/`chunks` compare a **local** name against everything up to the first whitespace, so an attribute … → [casting](../../docs/rules/casting.md)
- **AirPlay is the system's, on the Mac only, and the session starts before there is a television.** One menu row opens the system's list (an invisible `AVRoutePickerView` over the bar's cast button, pressed — the system picks the side, so the anchor is what decides it); the session is `connecting` with an `AVPlayer` muted beside mpv until the route goes external, and a television with AirPlay switched off — or a list closed without a pick — tells the Mac nothing, so `not_started` ends it. **Tracks and subtitles go as an HLS rendition and the original picture as an MP4 copy**: auto tries HLS, reads a refusal from our own server (the receiver asked for nothing past the master), falls back to the copy *fresh* (item away first) and remembers the refusal per receiver and picture kind. → [casting](../../docs/rules/casting.md)

## Commands

The verbose log — the Cast wire and discovery, DLNA, the LAN server's request
log, AirPlay's state changes — is a switch in Settings → Report a problem and in
the report itself, live and forgotten at restart. `FP_CAST_DEBUG=1` still turns
it on from the first line, which is the only way to have it for anything that
happens before the window exists. Either way it lands in the terminal *and* in
the ring the issue report reads:

```bash
FP_CAST_DEBUG=1 bun run tauri dev
```

Casting smoke test against whatever Cast devices the LAN has (browse + TLS +
receiver GET_STATUS; deliberately never launches an app on the device):

```bash
cd src-tauri && FP_TEST_CAST=1 cargo test --lib cast::tests::discover_smoke -- --nocapture
```

The unicast half of discovery, no multicast involved — one address, or one whole
round over the LAN with its timing (`sweep`, or an address instead of it):

```bash
cd src-tauri && FP_TEST_CAST_IP=sweep cargo test --lib cast::tests::unicast_probe_smoke -- --nocapture
```

A DLNA round against one LOCATION that never answers (loopback only, apart from
the one LAN sweep every round makes) — the live renderer must be shown long
before the round ends:

```bash
cd src-tauri && FP_TEST_DLNA_SLOW=1 cargo test --lib dlna::discovery_tests::slow_location_smoke -- --nocapture
```

AirPlay (macOS only) against a real receiver: the system's AirPlay list opens in
the window's top-left corner, you pick a television, and the session is scripted
from there — play, pause, resume, a seek forward and one back, stop — with one
log line per change of state. It runs inside the app, through the same functions
the cast menu calls, because the picker is a view in the window:

```bash
FP_AIRPLAY_PLAY=/abs/path/to/file.mp4 FP_CAST_DEBUG=1 bun run tauri dev 2>&1 | grep -E '\[airplay\]|\[cast\] http'
```
