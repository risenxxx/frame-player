# The catalog

Finding something to watch, and the release that carries it.

Everything here was measured against the live services rather than read from
documentation, and several of the measurements contradict what the obvious
design would assume. The rules that must not be broken are in
[CLAUDE.md](../CLAUDE.md); this is the reasoning, the numbers and the dead ends.

The worked example throughout is **Sintel** — the Blender Foundation's Creative
Commons short, which this repository already uses as its torrent test fixture
(`sintel_smoke` in `torrent.rs`). It is a real query against the real service,
and it happens to exhibit most of the awkward cases at once.

## Two services, and which answers what

A metadata service says **what** to watch — posters, localised titles,
descriptions, how many seasons a series has. An indexer says **where from** —
quality, dynamic range, size, dubs, seeders, a magnet.

Neither can do the other's job, and that is what decides the shape. An indexer
holds no posters and no descriptions, so browsing one directly is a list of raw
release names — precisely the experience a catalog exists to replace. A metadata
service knows nothing about what is available.

The bridge between them is a text search by title and year, and it is the part
that would have been most expensive to build. It turned out not to need
building: the indexer already parses release names into a local name, an
original name, a year, a quality, a dynamic range, a dub list and a season list.

## Why there is no TMDB key in the player

The first design baked one in, on the grounds that **TMDB rate-limit by IP
address rather than by key** — 50 requests/second and 20 connections per address
— so one key shared by every copy creates no contention: each viewer spends
their own budget.

That is true and it is beside the point. It is a fact about **load**, not a
permission. Read against the actual [API terms](https://www.themoviedb.org/api-terms-of-use):

- §1.A — the licence is **non-transferable and non-sublicensable**. Handing the
  key to every user is close to sublicensing it.
- §1.C — you may not "attempt to cloak or conceal Your identity, or the identity
  of any website, program, service, application". One anonymous credential
  shared by every copy is exactly that.
- §2.A — the free licence is **non-commercial**, which a free player satisfies.
  There is no "personal use" tier; that is not the right frame. Note GPL-3.0
  lets anyone sell copies, so a fork that starts charging owes TMDB its own
  commercial licence.
- §3 — **attribution is mandatory**: the exact sentence, plus the TMDB logo kept
  less prominent than the application's own marks.

The OpenSubtitles analogy that motivated the first design is weaker than it
looks and **inverted in one place**: OpenSubtitles *bans* applications that make
their users register keys of their own, so a shipped key is the required design
there. TMDB has no such rule, and per-user keys are ordinary for them.

So metadata goes through [`services/tmdb`](../services/tmdb), which holds the
key. One identified server calling on behalf of one identified application
raises neither §1.A nor §1.C.

The attribution is in the catalog panel: the sentence verbatim and untranslated,
with the logo above it at 13px — their own file, referenced rather than inlined
so nothing can alter it. That file carries a `viewBox` and no intrinsic
dimensions, so `width: auto` collapses it to a square; the ratio is stated
explicitly in CSS. Measured in a WKWebView harness: 13×13 with nothing painted
before, 179.6×13.0 after.

## Capacity

A proxy turns a per-IP budget into a shared ceiling, so the cache is not an
optimisation — it is what makes the architecture work. TMDB recommend caching
themselves.

The shape of the traffic is what makes the shared ceiling generous. Trending is
one upstream request per language per TTL, serving every viewer's panel. A
film's metadata does not change, so details cache for days. Only free-text
search has a real tail, and it alone sets the rate.

Taking a session to be one trending view, three searches and two title pages:

| Cache behaviour | Upstream calls/session | Sessions/s | Sessions/day |
|---|---|---|---|
| Cold, no hits at all | 6 | 8 | ~700 000 |
| Pessimistic (search 50 %, details 30 %) | ~2.4 | 21 | ~1 800 000 |
| Realistic (search 35 %, details 15 %) | ~1.35 | 37 | ~3 200 000 |

The rate limit is therefore not the binding constraint for any plausible number
of users. Bandwidth is — and that is a decision made on the client.

## Posters: the expensive nine tenths

A grid of twenty posters at `w342` is roughly **800 KB** against **~25 KB** of
JSON for the same screen. Proxying images by default would mean paying for the
expensive part of the traffic in order to serve a minority.

So posters load **straight from TMDB's own CDN**, which costs the proxy nothing
and is closer to the viewer than any server of ours. The proxy's `/img/…` route
exists for the viewers TMDB is not reachable from — a real population, which is
why every comparable player grew a proxy of its own.

**How the client decides is the interesting part.** The backend returns the
poster *path*, never a URL, and the frontend composes it against whichever base
works. Geolocating the client IP was considered and rejected: it needs a
database, is wrong for anyone on a VPN or a corporate network, and infers a fact
the browser reports for free. Trying is the measurement.

One failure was not enough of a measurement, though. An `<img>` cannot tell a
refused connection from a 404, and the live CDN **does** answer 404 for a stale
path — verified. A single missing poster would otherwise convict a perfectly
reachable CDN and push every image through the proxy, permanently and silently.
Hence `CDN_STRIKES`: three failures, with any success resetting the count. A
blocked CDN fails every image in a grid and reaches the threshold at once; a
stale path never does. The proxy passes 404 through as 404 for the same reason.

## Runtime configuration

No indexer address is compiled into the player. It reads a small document from
the update host — `catalog.json`, beside `latest.json` — each time the panel is
opened.

Ordinary remote configuration, for the ordinary reason: a value built into the
binary can be changed only by cutting a release, and the installed base keeps
the old one until each copy updates, so it is a per-build constant rather than a
default.

```jsonc
{
  "indexer": "https://…",   // where the catalog looks when the viewer has not said
  "disabled": false,        // stand the feature down, one level above the address
  "notice": "…"             // shown in place of the panel when disabled
}
```

It lives on the update host rather than on the metadata proxy so that it does
not inherit that service's availability: the one thing that has to be changeable
quickly should not depend on the one thing that does the most work.

Four properties it depends on:

- **A viewer's own setting always wins.** This only fills a gap, so changing it
  never affects somebody who chose their own indexer.
- **Any failure means "no configuration"** — a missing file, a blocked host, a
  404 and a truncated document are not told apart. The player then asks the
  viewer, which is a working state.
- **The player never persists what it reads.** Storing it would pin whatever the
  answer happened to be the first time the panel was opened, which is the
  per-build constant this replaced, reintroduced in the client.
- **How fast a change lands is a CDN setting, not a property of the file.**
  Setting `Cache-Control` as object metadata at upload is the obvious move and
  is the wrong one: it is the only part of the mechanism that lives in an
  object's headers, so it is lost the first time somebody re-uploads without the
  flag — silently, because the file is correct and only the timing changes. (The
  R2 dashboard has no field for it either, which is its own argument.)

  What is in place instead is a **Cache Rule** on this path, set to ignore the
  origin's `Cache-Control` and use a five-minute edge TTL. That makes the timing
  independent of how the object got there. Matching the path and not the host
  matters: a rule over the whole update host would also cover `latest.json` and
  delay the updater's view of a release.

  **How to tell it is actually matching**, which is not obvious from the
  dashboard: ask for the object twice and read `cf-cache-status`. A rule that
  applies gives `MISS` then `HIT` with an `age` header. **`DYNAMIC` means the
  rule is not matching at all** — Cloudflare decided not to cache, which for
  this file is harmless (changes land instantly) but means the rule is not doing
  what it looks like it is doing.

## The indexer's search is fuzzy; the filtering is ours

An indexer answers a query with everything that resembles it. For a widely
released title that is **well over a thousand rows**, most of them nothing to do
with the film — measured, one such query returned sports broadcasts among the
results. So the list has to be filtered here.

Sintel is small enough to show the whole pipeline: **19 rows returned, 19
matching the name, 14 after de-duplicating by info hash.** The five that fell
out were the same releases cross-posted, which is why the hash and not the title
is the identity.

Three things that filter must get right, each measured:

1. **Compare the indexer's parsed name fields, never the raw release title.**
   That string carries the year, the codec and the dub list, so a substring test
   against it matches anything that merely mentions the film.
2. **Match on *either* the local or the original name.** Some sources fill the
   original-name field with the entire release line — visible in the Sintel
   rows, where it runs to a full sentence of format and audio detail — leaving
   only the local name usable.
3. **Keep rows whose year is 0.** Not every source fills the year in, and a
   strict year filter drops them all; Sintel returns a mixture of `2010` and
   `0` in the same response.

`fold()` is what makes the comparison work across alphabets: release names mix
Latin and Cyrillic homoglyphs freely, so a title written with a Latin `a` is a
different string and the same film. Its limit is visible in the same data —
Sintel comes back under two different Cyrillic transliterations, which differ by
a real letter rather than a look-alike, and no folding can join those.

## Ordering

Quality is the outer key and dynamic range the inner one — 4K HDR → 4K SDR →
1080p HDR → 1080p SDR → … — with seeders deciding inside a group.

The first version sorted by seeders alone and put a live 480p rip above a 4K
remux with a healthy swarm. That answers "what is busiest"; the question a
viewer is asking is "what is the best copy I can get".

**One departure from a pure quality order: a release nobody seeds sinks to the
bottom.** Measured — of 95 4K rows in one response, **13 had no seeders at
all**. Without this the first row is routinely the best-looking thing that will
never download. Nothing is hidden: they are still listed, marked and pickable.

`dynamic_rank` is deliberately two buckets rather than a ladder. Across 768 rows
the field only ever held two values, ordinary and high, so ranking Dolby Vision
against HDR10 would be a distinction invented here — and it is not obviously the
right way round anyway, since DV looks better on a display that handles it and
worse on one that does not.

Because this also decides *which* releases survive the cap, it is a selection
order as well as a display one, **and the cap does bind on a popular title**:
measured, one such query yielded 326 releases after filtering against a cap of
120. So the frontend's "by seeders" pill re-orders the best 120 *by quality*
rather than the best 120 by seeders. Raising the cap is not the fix — a list
nobody reads to the end is not more useful — but if this ever matters, the
honest answer is to make the cap a property of the requested order.

## What the data does not support: translation types

The obvious next feature is filtering by the kind of translation — full dub,
multi-voice, two-voice, single-voice. The data does not carry it, and this is
worth writing down so it is not attempted twice.

**The dub field is a name, not a type.** Across 768 rows:

- **empty in 67 %** of them;
- only **17 distinct values** in total;
- of the rows that have any, **70 % carry the single value meaning "full dub"** —
  the only entry that is a *type* at all;
- everything else is the name of a studio, an individual translator or a
  broadcaster;
- the values are not even normalised: two spellings of the same person's surname
  appear as separate entries and would show up twice in any filter built from
  them.

Sintel shows the same shape at small scale: **9 of its 14 releases carry no dub
information at all**, while their raw titles plainly list several.

**The type can be read from the raw title, but almost nowhere.** Only **17 %**
of rows carry a recognisable marker, and that figure is an upper bound — short
abbreviations over-match. It is also wildly source-dependent: of the nine
sources in one response, **one wrote the type in 80 % of its rows and four never
wrote it at all**, and those four accounted for 38 % of the sample.

So a full taxonomy would be right for one source and wrong-by-omission for a
third of the results, and a filter for a particular kind of voice-over would
silently hide releases whose type merely was not written down. **Absence of a
marker is not absence of the thing**, and any filter here needs an explicit
"unknown" bucket rather than folding it into "no".

What *is* supportable, in descending order of reliability: a **full-dub** flag;
a coarse **"has a translated track"**; and a **filter by studio or translator
name**, which is how people actually choose for a series. None of it is built.

## Privacy

This is the only surface in the player that tells a third party what somebody is
*looking for* rather than acting on a file they already hold. It is on by
default with a switch to turn it off, and the switch exists precisely because
that argument is real — it is an argument for a way out, not for hiding the
feature from everyone who never opens the settings.

The proxy **does not log queries**. A proxy sees what everybody searches for,
which is a step backwards from the client calling TMDB directly, and the only
honest compensation is to keep none of it. Its `/health` counts totals, never
terms.

Nothing here is derived from a path, so there is no privacy root to gate
against. **That stops being true the moment anything asks "which releases exist
for the file I am watching"** — such a feature would need the gate before it
ships, and would become the eighth enforcement point.
