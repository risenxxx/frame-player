# Changesets

One file here per change a viewer of the player will notice, written with the
change. `bun run set-version` turns them into `changelog/<version>.md` and
deletes them; that file is what the GitHub release, the site's `/updates` page
and the release cards on its home page are rendered from.

```md
---
"frameplayer": minor
---

Lists fade at their edges

The queue, chapters and track menus fade out at the top and bottom when there
is more to scroll.
```

- `minor` (or `major`) is listed under **New**, `patch` under **Improvements and
  fixes**, and the largest one pending decides the bump of
  `bun run set-version release`.
- The first line is the headline (under 80 characters); what follows is one
  paragraph of detail, and may be left out.
- A file with **empty** frontmatter (`---` twice) is the release's summary: the
  one sentence a card on the home page prints. At most one.
- Everything here is public and read by people who use the player: English, no
  names from the code, no hosts, accounts, machines or people.

`bun run changelog:preview` shows what the next release's notes would say;
`bun run changelog:check` (part of `bun run gates`) checks every file here and
in `changelog/`. The full rules are in `docs/rules/build-and-release.md`.
