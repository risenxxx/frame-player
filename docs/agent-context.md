# The agent's context: what it costs and the plan to cut it

A coding agent works in this repository every day, and everything in
`CLAUDE.md` is sent with every one of its requests. This is the measurement of
what that costs, and the plan for spending less of it without the agent
checking its work any worse. Status: **plan, not yet applied.**

## The measurement (2026-10-27)

Forty most recent sessions in this repository, the current one excluded. The
context of a request is `input + cache_creation + cache_read` tokens.

| What | Measured |
|---|---|
| `CLAUDE.md` | 462 lines, 104 385 bytes, ≈ 26 000 tokens, no `@` imports. It was 70 KB a month ago |
| — Where the documentation is | 49 lines, 4.7 KB — needed for any task |
| — Commands | 178 lines, 9.2 KB — a quarter of it for any task, the rest one area each (signing, NSIS, smoke tests) |
| — Layout | 67 lines, **31.3 KB** — a row per module; any one task needs two or three rows |
| — Critical gotchas | 21 lines, 8.0 KB — about half apply anywhere, half to one area |
| — Conventions & flows | 141 lines, **50.7 KB** — ten apply anywhere; the rest link one `docs/rules/` chapter each. 62 of them are already a truncated stub of their chapter; ~80 are full paragraphs (up to 1.7 KB a line) found nowhere else |
| Other instruction files | none (`AGENTS.md`, `.claude/rules/`, `.cursor/rules`, Copilot) |
| `.claude/` in the repository, `.mcp.json` | none |
| Checks, green | `bun run gates` 1 162 bytes in 5 s; Go `vet` + `test -race` 203 bytes; `cargo check` 15 390 bytes after a pull (454 `Checking` lines), 467 bytes with `-q` |
| Checks in the sessions | 125 `gates` runs, median result 482 bytes; 78 `cargo check`, median 299 bytes — the agent already pipes cargo through `tail`/`grep` (119 times) |
| LSP | `rust-analyzer-lsp` enabled, binary present — **0 diagnostics surfaced in 40 sessions**. No TypeScript/Svelte or Go plugin; `gopls` is installed, `svelteserver` is not. TypeScript 5.6, Go 1.24 |
| How the agent reads | **3 871 reads through Bash** (`cat`, `sed -n`, `grep`…) against 253 `Read` calls; 61 `Edit`, 129 `Write` |
| Rule chapters | 283 tool calls touch `docs/rules/` — the chapters *are* being read on demand |

| Metric | Now | Target |
|---|---|---|
| **M1** context before the first word (median of first requests) | **70 458** tokens (61 842 – 108 768) | ≤ 52 000 |
| **M2** sessions whose largest request passed 400 K | **8 of 39 (21 %)**; 17 passed 200 K | ≤ 10 % |
| **M3** bytes of a green check run (`gates`; `cargo check`) | 1 162; 15 390 cold / 467 quiet | ≤ 1 500; ≤ 1 000 |
| **M4** tool results over 20 KB | 30 of 4 135 text results (0.7 %); 237 more are images, whose cost is set by their size, not their bytes | ≤ 1 % |

Across the forty sessions: 4 210 requests, 1.34 billion context tokens. The
file is in each of them, so it is about 8 % of the whole — worth cutting, but
the larger lever is M2, the length of a session, which is a habit and not a file.
The other ~44 000 tokens of a first request are the harness, its tools, and
the user-level skills and MCP servers — none of them in this repository.

## The plan

### Stage A — quiet checks and the LSP

Almost nothing to do, and the plan says so rather than building something.

- `bun run gates` is already quiet and already shows a failure in full. **No change.**
- `cargo check` is the one loud command, and the line in `CLAUDE.md` is also
  wrong on macOS with a fresh target directory (the root `.cargo/config.toml`
  points `FFMPEG_DIR`/`LIBCLANG_PATH` at Windows paths). Add `-q` and the macOS
  environment to that line — one line, no new script.
- LSP, offered and not installed: `rust-analyzer` is on but has never reported
  anything, most likely because the build script needs the same
  `FFMPEG_DIR` that `cargo check` does. `gopls-lsp` covers only `services/`.
  A Svelte server would need a plugin of our own (`.lsp.json` → `svelteserver`)
  and `svelte-check` already answers in 4 s. **Recommendation: leave the LSP as it is.**
- Accepted when: `cargo check -q` from the documented line passes on a fresh
  target directory on macOS, and a deliberate error in a `.rs` file shows up in
  its output.

### Stage B — `CLAUDE.md` to ≤ 200 lines and ≤ 30 KB

What stays, for any task: the two-paragraph description, the documentation
index, the core commands, a layout of one short line per directory or module
group, the gotchas that apply anywhere (1, 3, 4, 5, 8, 12, 13 in full; the
area-specific ones as their bold headline plus a pointer), and the conventions
that apply anywhere — validation, changesets, localisation, the three READMEs,
CI, the public repository.

Everything else moves **verbatim** into `.claude/rules/<chapter>.md`, one file
per existing `docs/rules/` chapter, each with `paths:` globs over the code that
chapter governs: the convention lines that link it, the layout rows of its
modules, its gotcha bodies and its smoke-test commands. About 75 KB across
fifteen files, 5 KB each, against 104 KB on every request today.

The measurement changes how this has to be done. Path-scoped rules load when
the agent *opens* a matching file with a tool, and this agent reads through
Bash fifteen times as often as it opens. So the moved text cannot rely on being
loaded automatically:

- `CLAUDE.md` keeps one line per chapter — the paths it covers and "before
  changing these, read `.claude/rules/<chapter>.md`" — which is the same habit
  that already sends the agent to `docs/rules/` 283 times in forty sessions.
- Editing a file still loads its rules, since `Edit` and `Write` require a
  `Read` of the file first.
- A question that edits nothing gets the index line and nothing else, which is
  what it gets today for any of the 62 stub lines.

Also in this stage: `docs/rules/README.md` and the "Part of the rule book"
header of every chapter describe the new layer; the 38 files that mention
`CLAUDE.md` are checked, and the ones naming a section that moved (two
workflows, `entitlements.plist`, `services/relay/go.mod`, `docs/watch-together.md`,
`docs/airplay.md`, a script) point at its new place; the three READMEs change
together if they change at all.

Accepted when: `CLAUDE.md` ≤ 200 lines and ≤ 30 KB; a script finds every
non-empty line of the old file in the new set (`CLAUDE.md` + `.claude/rules/` +
`docs/`); `bun run gates` is green; and a week later the session logs show the
area's rule file loaded or read in at least nine of ten sessions that edit
that area.

### Stage C — the length of a session (user settings: proposed, not changed)

- Automatic compaction at a smaller window, since one session in five passes
  400 K.
- `/clear` between unrelated tasks: the two longest sessions (945 K and 955 K)
  each ran several.
- A subagent for a wide search, so the dumps stay in its context.
- Turn off in `/mcp`, for this repository, the servers that have nothing to do
  with it (a résumé service, Notion, Docs). The deferred ones cost a line each
  in the tool list and their instructions; the user-level skill list costs more.

## What this does not do

- It does not measure correctness on our own tasks: a 10-point difference
  needs 120–200 tasks. The guard is the gates and CI, as it is for any change.
- No memory store with similarity search — it does not help on coding tasks.
- No generated architecture overview, and the moved text is not rewritten:
  shortening the content is how a context loses what it was for.
- No hook rewriting loud commands: the measurement shows the agent already
  keeps them quiet. Revisit only if a week's logs say otherwise.
- No product code changes.

## Re-measuring

Same method a week after Stage B: the forty newest session logs, the four
metrics above, plus the count of sessions that edited an area and loaded or
read its rule file.
