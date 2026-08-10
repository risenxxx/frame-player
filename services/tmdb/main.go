// A caching proxy in front of TMDB, so the player never carries an API key.
//
// It exists for three reasons, and the key is the least of them.
//
//  1. **The key stays here.** Baked into the client it would be one anonymous
//     credential shared by every copy, which is what the TMDB terms call
//     attempting to conceal the identity of an application (§1.C) — and handing
//     it to every user reads uncomfortably like sublicensing a licence that is
//     explicitly non-sublicensable (§1.A). One identified server calling on
//     behalf of one identified application is the ordinary shape and raises
//     neither question.
//  2. **TMDB is not reachable everywhere.** Access from Russia is unreliable in
//     practice, which is why every comparable player grew a proxy of its own.
//     Without one the catalog is simply blank for those viewers.
//  3. **Caching is what makes the rate limit a non-issue**, and TMDB recommend
//     it themselves. Their CDN allows roughly 50 requests/second and 20
//     concurrent connections *per IP*, and a proxy turns that from a per-user
//     budget into a shared ceiling — so the cache is not an optimisation here,
//     it is the thing that makes the architecture work at all.
//
// The shape of the traffic is what makes the ceiling generous: trending is one
// upstream request per language per TTL, serving every viewer's panel; a film's
// metadata does not change, so details cache for days. Only free-text search
// has a real tail, and it is the only thing that sets the rate.
//
// **What this does not do is log queries.** A proxy sees what everybody is
// searching for, which is a step backwards from the client calling TMDB
// directly, and the only honest compensation is to keep none of it. Access logs
// are deliberately absent; what is counted is totals, never terms.
//
//	FP_TMDB_KEY=… go run ./services/tmdb        # :8090
//	FP_TMDB_KEY=… TMDB_ADDR=:9000 go run ./services/tmdb
package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"io"
	"log"
	"net/http"
	"os"
	"os/signal"
	"regexp"
	"strings"
	"sync/atomic"
	"syscall"
	"time"
)

const (
	tmdbAPI   = "https://api.themoviedb.org"
	tmdbImage = "https://image.tmdb.org/t/p"

	// TMDB's CDN allows ~20 concurrent connections per IP. Staying under it is
	// this service's job, because every client now shares one address: a burst
	// that would be spread across a hundred machines arrives here as a hundred
	// simultaneous dials. Sixteen leaves headroom for the sweep and for a retry.
	maxUpstream = 16

	// How long a client may hold its own copy. Long enough to matter, short
	// enough that a corrected title reaches somebody this week.
	clientJSONMaxAge  = 30 * time.Minute
	clientImageMaxAge = 7 * 24 * time.Hour
)

// **Nothing here may exceed six months**: the TMDB terms forbid caching their
// content for longer (§1.C), so these are a licence term expressed as code
// rather than a tuning choice. The rest of each figure is about how fast the
// underlying fact changes.
var (
	// The list is computed weekly upstream; an hour keeps it lively without
	// making it the thing that sets the request rate.
	ttlTrending = time.Hour
	// A free-text query. The tail of these is what the whole cap exists for.
	ttlSearch = 6 * time.Hour
	// A film's year, runtime and season count do not change.
	ttlDetails = 7 * 24 * time.Hour
	ttlDefault = time.Hour
	// An image path names one immutable rendering, so this is bounded by the
	// terms and by the disk, not by staleness.
	ttlImage = 30 * 24 * time.Hour
)

// A TMDB image request is `/img/<size>/<file>`, and both halves are matched
// against a strict pattern before anything is joined to a path. This is the
// only place an untrusted string reaches the filesystem, so it is checked here
// and trusted afterwards — `..`, a separator and a leading dot are all
// unrepresentable in these patterns rather than stripped out of them.
var (
	sizePattern = regexp.MustCompile(`^(w\d{2,4}|h\d{2,4}|original)$`)
	filePattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$`)
)

type server struct {
	key    string
	token  string
	json   *jsonCache
	images *imageCache
	client *http.Client
	// A counting semaphore over upstream calls. A channel rather than
	// `golang.org/x/sync/semaphore` because that would be a dependency, and a
	// buffered channel is the same thing in four lines.
	slots chan struct{}

	hits, misses, imageHits, imageMisses atomic.Int64
}

func main() {
	addr := flag.String("addr", envOr("TMDB_ADDR", ":8090"), "listen address")
	cacheDir := flag.String("cache", envOr("TMDB_CACHE_DIR", "./cache/images"), "image cache directory")
	entries := flag.Int("entries", 20000, "how many JSON responses to keep in memory")
	flag.Parse()

	key := strings.TrimSpace(os.Getenv("FP_TMDB_KEY"))
	if key == "" {
		log.Fatal("FP_TMDB_KEY is not set — this service is the only place the key lives")
	}

	// **A cache that cannot be opened is not a reason to refuse to start.** The
	// job this service exists for — holding the API key and answering /3/ — needs
	// no disk at all; the image cache is a bandwidth optimisation for the
	// viewers TMDB is unreachable from. Dying here turns a misconfigured mount
	// into no service at all, which is what `mkdir /cache/images: permission
	// denied` did on the first deploy.
	//
	// So it degrades instead, loudly: the reason is logged, /health reports the
	// cache as off, and the image route answers 503 rather than pretending. The
	// one thing it must not do is fail quietly.
	images, err := newImageCache(*cacheDir, ttlImage)
	if err != nil {
		log.Printf("WARNING: image cache unavailable at %s: %v", *cacheDir, err)
		log.Printf("WARNING: posters will not be proxied. If this is a bind mount, "+
			"chown it to the container's user (65534) or use a named volume instead.")
		images = nil
	}

	s := &server{
		key: key,
		// Optional and off by default, so a self-hosted instance needs no
		// setup. Our own deployment sets it, because an *open* TMDB proxy that
		// any application may point at is re-serving the API to third parties —
		// which is the sublicensing this whole design exists to avoid. It is a
		// statement of scope rather than a security boundary, and the comment
		// says so instead of the name pretending otherwise.
		token:  strings.TrimSpace(os.Getenv("TMDB_TOKEN")),
		json:   newJSONCache(*entries),
		images: images,
		client: &http.Client{
			Timeout: 20 * time.Second,
			Transport: &http.Transport{
				MaxIdleConns:        maxUpstream,
				MaxIdleConnsPerHost: maxUpstream,
				MaxConnsPerHost:     maxUpstream,
				IdleConnTimeout:     90 * time.Second,
			},
		},
		slots: make(chan struct{}, maxUpstream),
	}

	mux := http.NewServeMux()
	mux.HandleFunc("GET /health", s.health)
	mux.HandleFunc("GET /3/", s.serveJSON)
	mux.HandleFunc("GET /img/{size}/{file}", s.serveImage)

	go s.sweepImages()

	srv := &http.Server{
		Addr:              *addr,
		Handler:           mux,
		ReadHeaderTimeout: 10 * time.Second,
	}

	// Graceful shutdown, so an in-flight image write is not interrupted between
	// its temporary file and the rename.
	stop := make(chan os.Signal, 1)
	signal.Notify(stop, os.Interrupt, syscall.SIGTERM)
	go func() {
		<-stop
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		_ = srv.Shutdown(ctx)
	}()

	log.Printf("tmdb proxy on %s, images in %s", *addr, *cacheDir)
	if err := srv.ListenAndServe(); err != nil && !errors.Is(err, http.ErrServerClosed) {
		log.Fatal(err)
	}
}

func envOr(name, fallback string) string {
	if v := strings.TrimSpace(os.Getenv(name)); v != "" {
		return v
	}
	return fallback
}

// Totals, never terms. Enough to see whether the cache is working and whether
// the instance is alive; not enough to reconstruct what anybody watched.
func (s *server) health(w http.ResponseWriter, _ *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	// `cache` is here so a degraded instance is visible from outside rather than
	// only in a log line somebody read once at deploy time.
	cache := "on"
	if s.images == nil {
		cache = "off"
	}
	fmt.Fprintf(w,
		`{"ok":true,"json":{"hits":%d,"misses":%d,"entries":%d},"images":{"hits":%d,"misses":%d,"cache":%q}}`,
		s.hits.Load(), s.misses.Load(), s.json.len(), s.imageHits.Load(), s.imageMisses.Load(), cache)
}

func (s *server) allowed(r *http.Request) bool {
	return s.token == "" || r.Header.Get("X-FP-Token") == s.token
}

// Acquire an upstream slot, respecting the caller going away.
func (s *server) acquire(ctx context.Context) error {
	select {
	case s.slots <- struct{}{}:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

func (s *server) release() { <-s.slots }

func ttlFor(path string) time.Duration {
	switch {
	case strings.HasPrefix(path, "/3/trending/"):
		return ttlTrending
	case strings.HasPrefix(path, "/3/search/"):
		return ttlSearch
	case strings.HasPrefix(path, "/3/movie/"), strings.HasPrefix(path, "/3/tv/"):
		return ttlDetails
	default:
		return ttlDefault
	}
}

// The cache key: the path plus the query with our own credential removed.
//
// **`api_key` is stripped rather than merely ignored**, and both halves matter.
// A client that sends one must not be able to store an entry under a key nobody
// else will ever hit — that is a cache poisoned into uselessness by one caller
// — and it must certainly not be able to make requests on somebody else's
// credential through this service.
func cacheKey(path string, query map[string][]string) string {
	var b strings.Builder
	b.WriteString(path)
	keys := make([]string, 0, len(query))
	for k := range query {
		if k == "api_key" {
			continue
		}
		keys = append(keys, k)
	}
	// Sorted, or the same request typed twice arrives as two entries whenever
	// the client's map iteration order differs.
	sortStrings(keys)
	for _, k := range keys {
		for _, v := range query[k] {
			b.WriteByte('&')
			b.WriteString(k)
			b.WriteByte('=')
			b.WriteString(v)
		}
	}
	return b.String()
}

// A four-line insertion sort rather than importing `sort` for one call on a
// list that is never longer than a handful of query parameters.
func sortStrings(xs []string) {
	for i := 1; i < len(xs); i++ {
		for j := i; j > 0 && xs[j] < xs[j-1]; j-- {
			xs[j], xs[j-1] = xs[j-1], xs[j]
		}
	}
}

func (s *server) serveJSON(w http.ResponseWriter, r *http.Request) {
	if !s.allowed(r) {
		http.Error(w, "forbidden", http.StatusForbidden)
		return
	}
	key := cacheKey(r.URL.Path, r.URL.Query())
	if hit, ok := s.json.get(key); ok {
		s.hits.Add(1)
		writeBody(w, hit.kind, hit.body, clientJSONMaxAge, "HIT")
		return
	}
	s.misses.Add(1)

	// Rebuilt rather than forwarded: the client's own query minus anything
	// pretending to be a credential, plus ours.
	q := r.URL.Query()
	q.Del("api_key")
	q.Set("api_key", s.key)
	upstream := tmdbAPI + r.URL.Path + "?" + q.Encode()

	if err := s.acquire(r.Context()); err != nil {
		return
	}
	body, kind, status, err := s.fetch(r.Context(), upstream)
	s.release()
	if err != nil {
		http.Error(w, "upstream unavailable", http.StatusBadGateway)
		return
	}
	if status != http.StatusOK {
		// Not cached: a 401 means our key is wrong and a 429 means we are being
		// throttled, and storing either would turn a transient fault into one
		// that outlives its cause by the whole TTL.
		w.Header().Set("Content-Type", kind)
		w.WriteHeader(status)
		_, _ = w.Write(body)
		return
	}
	s.json.put(key, entry{body: body, kind: kind, expires: time.Now().Add(ttlFor(r.URL.Path))})
	writeBody(w, kind, body, clientJSONMaxAge, "MISS")
}

func (s *server) serveImage(w http.ResponseWriter, r *http.Request) {
	if !s.allowed(r) {
		http.Error(w, "forbidden", http.StatusForbidden)
		return
	}
	// Answered rather than proxied uncached: without a cache every poster would
	// be a fresh upstream fetch against a 20-connection budget, which is the one
	// way this service can take TMDB's rate limit down for everybody. The client
	// falls back to loading posters from TMDB directly, which is what it does by
	// default anyway.
	if s.images == nil {
		http.Error(w, "image cache unavailable", http.StatusServiceUnavailable)
		return
	}
	size, file := r.PathValue("size"), r.PathValue("file")
	if !sizePattern.MatchString(size) || !filePattern.MatchString(file) {
		http.Error(w, "bad image path", http.StatusBadRequest)
		return
	}
	key := size + "_" + file

	if body, ok := s.images.get(key); ok {
		s.imageHits.Add(1)
		writeBody(w, contentTypeFor(file), body, clientImageMaxAge, "HIT")
		return
	}

	// Only one fetch per key at a time; everybody else waits and then re-reads.
	mine, done := s.images.claim(key)
	if !mine {
		if body, ok := s.images.get(key); ok {
			s.imageHits.Add(1)
			writeBody(w, contentTypeFor(file), body, clientImageMaxAge, "HIT")
			return
		}
		// The other flight failed. Falling through to fetch it ourselves would
		// be a retry storm against an upstream that just refused.
		http.Error(w, "upstream unavailable", http.StatusBadGateway)
		return
	}
	defer done()

	s.imageMisses.Add(1)
	if err := s.acquire(r.Context()); err != nil {
		return
	}
	body, kind, status, err := s.fetch(r.Context(), tmdbImage+"/"+size+"/"+file)
	s.release()
	// **A missing poster is not an unreachable upstream**, and collapsing the
	// two is worse here than it looks: the client reads a failed image as
	// evidence that TMDB's CDN is blocked and switches every poster to this
	// proxy. One 404 must not be able to do that, so the status is passed
	// through rather than flattened. Measured against the live CDN, which does
	// answer 404 for a path that no longer exists.
	if err == nil && status == http.StatusNotFound {
		http.Error(w, "no such image", http.StatusNotFound)
		return
	}
	if err != nil || status != http.StatusOK {
		http.Error(w, "upstream unavailable", http.StatusBadGateway)
		return
	}
	if err := s.images.put(key, body); err != nil {
		// Serve it anyway: failing to *store* an image the client is waiting
		// for is a disk problem, not a reason to show them a broken poster.
		log.Printf("image cache write failed: %v", err)
	}
	if kind == "" {
		kind = contentTypeFor(file)
	}
	writeBody(w, kind, body, clientImageMaxAge, "MISS")
}

func (s *server) fetch(ctx context.Context, url string) (body []byte, kind string, status int, err error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return nil, "", 0, err
	}
	// **reqwest is not the only client that sends nothing by default**, and a
	// nameless request is exactly what a WAF refuses — the lesson this
	// repository has now paid for twice, on a tracker announce and on UPnP.
	// Go does send one, but its default names the language rather than us, and
	// §1.C asks that an application not conceal its identity.
	req.Header.Set("User-Agent", "FramePlayer-TMDBProxy/1.0")
	resp, err := s.client.Do(req)
	if err != nil {
		return nil, "", 0, err
	}
	defer resp.Body.Close()
	// Bounded: an upstream that answers with something enormous must not be
	// able to spend this process's memory. A TMDB image at `original` is a few
	// megabytes; 32 is well clear of it and well short of trouble.
	body, err = io.ReadAll(io.LimitReader(resp.Body, 32<<20))
	if err != nil {
		return nil, "", 0, err
	}
	return body, resp.Header.Get("Content-Type"), resp.StatusCode, nil
}

func writeBody(w http.ResponseWriter, kind string, body []byte, maxAge time.Duration, cacheState string) {
	if kind == "" {
		kind = "application/json"
	}
	w.Header().Set("Content-Type", kind)
	// The client caching too is what keeps a viewer scrolling back and forth
	// through a grid from costing this service anything at all.
	w.Header().Set("Cache-Control", fmt.Sprintf("public, max-age=%d", int(maxAge.Seconds())))
	w.Header().Set("X-Cache", cacheState)
	_, _ = w.Write(body)
}

func contentTypeFor(file string) string {
	switch {
	case strings.HasSuffix(file, ".png"):
		return "image/png"
	case strings.HasSuffix(file, ".svg"):
		return "image/svg+xml"
	case strings.HasSuffix(file, ".webp"):
		return "image/webp"
	default:
		return "image/jpeg"
	}
}

// Expired images are removed on a timer, because a read only ever touches the
// keys somebody still wants and the sweep is about the ones nobody does.
func (s *server) sweepImages() {
	if s.images == nil {
		return
	}
	for {
		time.Sleep(6 * time.Hour)
		if n, err := s.images.sweep(); err != nil {
			log.Printf("image sweep: %v", err)
		} else if n > 0 {
			log.Printf("image sweep: removed %d expired", n)
		}
	}
}
