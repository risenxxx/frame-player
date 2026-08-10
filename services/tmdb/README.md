# TMDB metadata proxy

A caching proxy in front of [TMDB](https://www.themoviedb.org), so the player
never carries an API key. It is what the catalog's *what to watch* half talks
to; the *where to get it* half is a separate, third-party indexer and has
nothing to do with this service.

```bash
FP_TMDB_KEY=… go run ./services/tmdb          # :8090
FP_TMDB_KEY=… TMDB_ADDR=:9000 go run ./services/tmdb
go test -race frameplayer/tmdb/...
```

## Why it exists

Three reasons, and the key is the least of them.

**The key stays here.** Baked into the client it would be one anonymous
credential shared by every copy of the player — which is what the TMDB API terms
call attempting to conceal the identity of an application (§1.C), and handing it
to every user reads like sublicensing a licence that is explicitly
non-sublicensable (§1.A). One identified server calling on behalf of one
identified application raises neither question. Note what is *not* the argument:
TMDB rate-limit by IP rather than by key, so a shared key would have cost them no
**load** — a fact about capacity, not a permission.

**TMDB is not reachable everywhere.** Access from Russia is unreliable in
practice, which is why every comparable player grew a proxy of its own. Without
one the catalog is simply blank for those viewers.

**Caching is what makes the rate limit a non-issue**, and TMDB recommend it
themselves. Their CDN allows roughly 50 requests/second and 20 concurrent
connections *per IP*. A proxy turns that from a per-user budget into a shared
ceiling, so the cache is not an optimisation here — it is the thing that makes
the architecture work at all.

## Capacity

The shape of the traffic is what makes the shared ceiling generous. Trending is
one upstream request per language per TTL, serving every viewer's panel. A
film's metadata does not change, so details cache for days. Only free-text
search has a real tail, and it alone sets the rate.

Taking a session to be one trending view, three searches and two title pages:

| Cache behaviour | Upstream calls per session | Sessions/second | Sessions/day |
|---|---|---|---|
| Cold, no hits at all | 6 | 8 | ~700 000 |
| Pessimistic (search 50 %, details 30 %) | ~2.4 | 21 | ~1 800 000 |
| Realistic (search 35 %, details 15 %) | ~1.35 | 37 | ~3 200 000 |

The rate limit is therefore not the binding constraint for any plausible number
of users. What binds first is bandwidth, and that is a decision made on the
client — see below.

## Posters

**Poster bytes are the whole of the traffic**: a grid of twenty at `w342` is
roughly 800 KB against ~25 KB of JSON for the same screen. So the player fetches
them **straight from TMDB's own CDN** by default, which costs this service
nothing and is faster for the viewer than any server of ours could be.

The proxy's `/img/…` route exists for the viewers TMDB is not reachable from,
and the player switches to it when an image fails to load. Trying is the
measurement; geolocating the client IP would need a database, would be wrong for
anybody on a VPN, and would infer a fact the browser can simply report. The
verdict is remembered, so the discovery is paid once.

## Routes

| Route | What it does |
|---|---|
| `GET /3/…` | Forwarded to TMDB with the key attached, cached in memory. The path is TMDB's own, so a route added in the player needs no deploy here |
| `GET /img/{size}/{file}` | Forwarded to `image.tmdb.org`, cached on disk |
| `GET /health` | Liveness and cache counters |

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `FP_TMDB_KEY` | — | **Required.** The TMDB API key; the only place it exists |
| `TMDB_ADDR` | `:8090` | Listen address |
| `TMDB_CACHE_DIR` | `./cache/images` | Where poster bytes are kept |
| `TMDB_TOKEN` | unset | When set, requests must carry it as `X-FP-Token` |

`TMDB_TOKEN` is off by default so a self-hosted instance needs no setup. Our own
deployment sets it, because an *open* TMDB proxy that any application may point
at is re-serving the API to third parties — which is the sublicensing this whole
design exists to avoid. It is a statement of scope rather than a security
boundary, and the code says so rather than letting the name imply otherwise.

## Deploying, and the one thing that bites first

The image runs as an unprivileged user (uid 65534) and keeps its poster cache in
a volume. Those two facts collide in a way that produces exactly one error:

```
image cache: mkdir /cache/images: permission denied
```

`scratch` has no shell, so the cache directory is created in the build stage and
carried over with `COPY --chown=65534:65534`. What happens next depends on how
the volume is attached, and the difference is not obvious:

| Mount | Ownership comes from | Works out of the box |
|---|---|---|
| none | the image | yes — but the cache is lost on every redeploy |
| **named volume** (`-v tmdb-cache:/cache`) | the image, copied in when the volume is **first created** | **yes** |
| bind mount (`-v /host/path:/cache`) | the host directory | no — the host path must be chowned |

**Prefer a named volume.** In Dokploy that is a *Volume Mount* rather than a
*Bind Mount*, with mount path `/cache`.

**A named volume is seeded once, at creation.** Rebuilding the image after a
failed deploy does not re-seed a volume that already exists — it was created
from the old root-owned directory and stays root-owned. Remove it and let it be
recreated:

```bash
docker volume rm <name>     # or delete it in Dokploy, then redeploy
```

**If it must be a bind mount**, fix the host directory once:

```bash
mkdir -p /host/path/images && chown -R 65534:65534 /host/path
```

To check what the image itself carries:

```bash
docker run --rm --entrypoint "" <image> /tmdb -h   # scratch has no shell to ls with
docker run --rm -e FP_TMDB_KEY=x <image> &         # then: curl .../health
```

`"images":{"cache":"off"}` in `/health` is the service telling you the cache is
not working. It keeps serving `/3/` regardless — that half needs no disk, and a
misconfigured mount should not mean no service — but `/img/…` answers 503 and
posters are not proxied, which for a viewer behind a TMDB block means no posters
at all.

## What it does not do

**It does not log queries.** A proxy sees what everybody is searching for, which
is a step backwards from the client calling TMDB directly, and the only honest
compensation is to keep none of it. Access logs are deliberately absent; what is
counted is totals, never terms.

## Cache lifetimes

Every one is bounded by the TMDB terms, which forbid caching their content for
longer than six months (§1.C) — a legal limit expressed as code, and pinned by a
test rather than by a comment. Within that bound each figure is about how fast
the underlying fact changes.

| What | TTL |
|---|---|
| Trending | 1 hour |
| Search | 6 hours |
| Film and series details | 7 days |
| Anything else | 1 hour |
| Images | 30 days |

## Attribution

The player displays the notice the terms require (§3) wherever TMDB data is
shown. The TMDB logo is **still missing** and is a real obligation, not an
optional courtesy — it needs their official asset committed to the repository.
