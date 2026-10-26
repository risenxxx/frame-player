---
paths:
  - "src-tauri/src/torrent.rs"
  - "src-tauri/src/net_route.rs"
  - "src-tauri/src/upnp.rs"
  - "src-tauri/vendor/**"
  - "src/lib/components/VpnRouteDialog.svelte"
---

# torrents-network — the short form

The short form of [`docs/rules/torrents-network.md`](../../docs/rules/torrents-network.md): the modules this area is made of,
its rules one paragraph each, and the commands that exercise it. Claude Code loads this file
by itself when a file it covers is opened with Read/Edit/Write — not when one is read through
Bash, which is why `CLAUDE.md` says to read it before changing the area.

Each line is the rule; the chapter it links to is the whole of it.

## Modules

| Path | What lives there |
|---|---|
| `src-tauri/src/net_route.rs` | Which interface the swarm leaves by: the interface list (getifaddrs + SystemConfiguration on macOS, `GetAdaptersAddresses` on Windows), which of them is a tunnel, which is the way out past it (`Route::Direct`), and where the system route goes right now — a UDP `connect` to a public address, which sends nothing. It routes nothing itself; librqbit's `bind_device_name` does |
| `src-tauri/src/upnp.rs` | Asking the router whether the BitTorrent port is really forwarded: SSDP for the gateway, then `GetSpecificPortMappingEntry` for the mapping librqbit made. A read, never a write — the mapping is librqbit's to renew. Exists because its forwarder is a `run_forever` task that reports to nobody, so the setting would otherwise be a claim rather than a fact |
| `src-tauri/vendor/` | Three librqbit crates vendored via `[patch.crates-io]` (its README has the details, and is the thing to read before bumping librqbit). `librqbit-dualstack-sockets` carries the Windows half of binding a socket to an interface, which is what "past the VPN" rests on there — see `net_route.rs`. `librqbit-dht` carries a dozen lines so the DHT does not die on a UDP recv error, which Windows produces routinely — see "Windows kills the stock DHT" below. `librqbit` carries **Message Stream Encryption**, taken whole from an unmerged upstream PR and to be dropped the day it lands — see "A network can refuse a torrent by where it is going or by what it is". The `librqbit-tracker-comms` copy that used to be here went back to the registry with the bump to 9.x, where all three of its fixes now live |

## Rules

- **Windows kills the stock DHT, and the tracker was never a backup — two librqbit bugs whose combination left Windows with no peer source at all.** The symptom read as a network problem (magnet never resolves; a `.torrent` lists … → [torrents-network](../../docs/rules/torrents-network.md)
- **The HTTP announce was dead on every platform, and the DHT hid it.** Two more vendored fixes, found together and each fatal on its own. → [torrents-network](../../docs/rules/torrents-network.md)
- **A network can refuse a torrent by where it is going or by what it is, and the player can answer only the first.** That distinction is what decides everything below, and it was reported as one bug: a viewer whose VPN forbids … → [torrents-network](../../docs/rules/torrents-network.md)
- **Past the VPN is a choice of interface, not of route.** `SessionPrefs::route` (`auto` — the default, the system's choice — / `direct` / one named interface) becomes `bind_device_name`, which scopes every socket librqbit opens; Windows needs the vendored `librqbit-dualstack-sockets` (`IP_UNICAST_IF`). `direct` is re-resolved per session, a route that cannot be honoured refuses the torrent rather than falling back, and "is the route a tunnel" is asked of the system (`net_route.rs`), never guessed. → [torrents-network](../../docs/rules/torrents-network.md)
- **Five preferences are baked into the session and one comparison decides whether it can be reused.** Seeding, the port mapping, the proxy, the encryption mode and the route are all fixed when `Session::new_with_opts` runs, so changing any of them tears the … → [torrents-network](../../docs/rules/torrents-network.md)
- **The bump from librqbit 8.1.1 to 9.0.1 is what made those two possible, and it moved five things worth knowing.** `SessionOptions` grew sub-structs — `dht`, `listen`, `connect` — so the DHT persistence flag, the port range and … → [torrents-network](../../docs/rules/torrents-network.md)
- **Peer count has exactly one large lever, and it is not the one that suggests itself.** One private tracker's swarm was probed one address at a time, and the failures separate cleanly: of ~30 peers the tracker handed out, **20–22 … → [torrents-network](../../docs/rules/torrents-network.md)

## Commands

Torrent streaming against a real swarm (ROADMAP 21's spike, kept as a test — off
by default because it needs the network, live seeders and up to a minute):

```bash
cd src-tauri && FP_TEST_MAGNET=1 cargo test --lib torrent::tests::sintel_smoke -- --nocapture
```

And its sibling for when a torrent does *not* download — a minute of peers seen
vs peers connected vs bytes, then per-peer counters (attempts, errors, chunks).
`sintel_smoke` can only report that as a read which never returns; this says why:

```bash
cd src-tauri && FP_TEST_SWARM='magnet:?xt=…' cargo test --lib torrent::tests::swarm_probe -- --nocapture
```
