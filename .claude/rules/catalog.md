---
paths:
  - "src-tauri/src/catalog.rs"
  - "src/lib/catalog.svelte.ts"
  - "src/lib/components/CatalogDialog.svelte"
  - "services/tmdb/**"
---

# catalog — the short form

The short form of [`docs/rules/catalog.md`](../../docs/rules/catalog.md): the modules this area is made of,
its rules one paragraph each, and the commands that exercise it. Claude Code loads this file
by itself when a file it covers is opened with Read/Edit/Write — not when one is read through
Bash, which is why `CLAUDE.md` says to read it before changing the area.

Each line is the rule; the chapter it links to is the whole of it.

## Modules

| Path | What lives there |
|---|---|
| `src-tauri/src/catalog.rs` + `src/lib/catalog.svelte.ts` + `CatalogDialog.svelte` | Finding something to watch: TMDB for *what* (posters, localised titles, seasons) and a Torznab-compatible indexer for *where from* (trackers, quality, dubs, seeders, a magnet). On by default with a switch to turn it off; the metadata proxy and the indexer are both settings. See "The catalog" below |

## Rules

- **The catalog is two services and one rule about which answers what.** A metadata service says *what* to watch — posters, localised titles, descriptions, how many seasons a series has — and a Torznab-compatible indexer says … → [catalog](../../docs/rules/catalog.md)
