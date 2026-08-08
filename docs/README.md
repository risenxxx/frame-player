# Design notes

Why Frame Player is built the way it is: the constraints that shaped it, the
measurements behind the decisions, and the dead ends that are not worth walking
into twice.

These documents answer **why**. [`CLAUDE.md`](../CLAUDE.md) in the repository
root answers **what is true of the code right now** — it is the rule book, and
where the two disagree, it wins. A finding earns a line there when breaking it
would break the player; it stays here when its value is saving the next
investigation.

| Document | What it covers |
|---|---|
| [architecture.md](architecture.md) | The stack: Tauri 2 + in-process libmpv, video behind a transparent webview, and the constraints that follow from it. Read before touching mpv interop, seeking, fullscreen or zoom. |
| [macos.md](macos.md) | Why stock mpv cannot embed on macOS, what the patch in [`patches/`](../patches/) changes, and the platform's own traps — window chrome, permissions, bundling. |
| [casting.md](casting.md) | Playing on a television: Google Cast and DLNA, what each transport can carry, and how the player decides between them. |
| [torrents.md](torrents.md) | Streaming from a swarm: piece priority driven by playback, what a partially downloaded file can and cannot be used for, and casting one to a TV. |
| [distribution.md](distribution.md) | Shipping: code signing, Gatekeeper, SmartScreen, updates, and what a store listing would cost. |
| [ROADMAP.md](ROADMAP.md) | What shipped, what is planned, and what is only being considered. |

## Conventions

**Numbers are measured, not estimated.** Where a document states a figure —
a seek costing 1.9 s, an audio transcode running at 276× realtime, a poster
frame scoring 2.7 — it came from running the thing, and the conditions are
stated next to it. A claim that could not be measured says so.

**Dead ends are recorded with their reason.** An approach that was tried and
abandoned is more useful written down than deleted: the next person will have
the same idea, and the interesting part is why it failed rather than that it
did.

**Device behaviour is observation, not specification.** Televisions, receivers
and swarms do not document themselves. Where a document says a device refuses
something, that is what one device did on one network; the phrasing tries to
keep the difference between "the protocol forbids this" and "this set refused
it" visible, because they lead to different fixes.
