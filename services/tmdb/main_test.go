package main

import (
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
)

// **The licence expressed as an assertion.** The TMDB terms forbid caching
// their content for longer than six months (§1.C), so every TTL in this service
// is bounded by a legal document rather than by taste. Raising one past the
// bound is a terms violation that nothing else would report — the service would
// simply keep working — which is exactly the class of mistake worth a test.
func TestTTLsStayInsideTheLicence(t *testing.T) {
	const sixMonths = 180 * 24 * time.Hour
	for name, ttl := range map[string]time.Duration{
		"trending": ttlTrending,
		"search":   ttlSearch,
		"details":  ttlDetails,
		"default":  ttlDefault,
		"image":    ttlImage,
		// The figures handed to clients bind for the same reason: a browser
		// holding a copy for longer than we may is still our cache.
		"client json":  clientJSONMaxAge,
		"client image": clientImageMaxAge,
	} {
		if ttl > sixMonths {
			t.Errorf("%s TTL is %v, over the six months the TMDB terms allow", name, ttl)
		}
		if ttl <= 0 {
			t.Errorf("%s TTL is %v, which caches nothing at all", name, ttl)
		}
	}
}

func TestTTLForRoute(t *testing.T) {
	cases := map[string]time.Duration{
		"/3/trending/all/week": ttlTrending,
		"/3/search/multi":      ttlSearch,
		"/3/movie/603":         ttlDetails,
		"/3/tv/1396":           ttlDetails,
		"/3/configuration":     ttlDefault,
	}
	for path, want := range cases {
		if got := ttlFor(path); got != want {
			t.Errorf("ttlFor(%q) = %v, want %v", path, got, want)
		}
	}
}

// The cache key decides what two requests share, and both of its jobs fail
// silently. A key that kept `api_key` would let one caller store entries nobody
// else can ever hit — a cache poisoned into uselessness rather than into
// danger, and invisible from outside. A key that did not sort would store the
// same request twice whenever a client's parameter order differed.
func TestCacheKeyStripsTheCredentialAndSorts(t *testing.T) {
	withKey, _ := url.ParseQuery("query=dune&api_key=SECRET&language=ru-RU")
	without, _ := url.ParseQuery("language=ru-RU&query=dune")
	a := cacheKey("/3/search/multi", withKey)
	b := cacheKey("/3/search/multi", without)
	if a != b {
		t.Errorf("the credential or the order leaked into the key:\n  %q\n  %q", a, b)
	}
	if strings.Contains(a, "SECRET") {
		t.Errorf("the API key reached the cache key: %q", a)
	}
	// And genuinely different requests must still differ, or one language's
	// results would be served for the other.
	other, _ := url.ParseQuery("query=dune&language=en-US")
	if cacheKey("/3/search/multi", other) == a {
		t.Error("two different languages produced one key")
	}
}

// The only untrusted string in this service that reaches the filesystem. The
// patterns are written so a separator and a dot segment are unrepresentable
// rather than stripped, and this is what says so.
func TestImagePathValidationRefusesEscapes(t *testing.T) {
	bad := []struct{ size, file string }{
		{"w342", "../../../etc/passwd"},
		{"w342", "..%2Fescape.jpg"},
		{"w342", "/absolute.jpg"},
		{"w342", ".hidden"},
		{"w342", ""},
		{"../w342", "poster.jpg"},
		{"original/../..", "poster.jpg"},
		{"drop", "poster.jpg"},
		{"w342", strings.Repeat("a", 200) + ".jpg"},
	}
	for _, c := range bad {
		if sizePattern.MatchString(c.size) && filePattern.MatchString(c.file) {
			t.Errorf("accepted %q/%q, which must never reach a path join", c.size, c.file)
		}
	}
	good := []struct{ size, file string }{
		{"w342", "aBc123_-.jpg"},
		{"w92", "poster.png"},
		{"original", "x.jpg"},
		{"h632", "y.webp"},
	}
	for _, c := range good {
		if !sizePattern.MatchString(c.size) || !filePattern.MatchString(c.file) {
			t.Errorf("refused %q/%q, which is an ordinary TMDB image", c.size, c.file)
		}
	}
}

func TestJSONCacheEvictsAndExpires(t *testing.T) {
	c := newJSONCache(2)
	live := time.Now().Add(time.Hour)
	c.put("a", entry{body: []byte("A"), expires: live})
	c.put("b", entry{body: []byte("B"), expires: live})
	// Touching "a" makes "b" the oldest, so the third insert must drop "b".
	if _, ok := c.get("a"); !ok {
		t.Fatal("a should still be cached")
	}
	c.put("c", entry{body: []byte("C"), expires: live})
	if _, ok := c.get("b"); ok {
		t.Error("b should have been evicted as least recently used")
	}
	if _, ok := c.get("a"); !ok {
		t.Error("a was used most recently and must survive")
	}

	// An expired entry is dropped rather than merely reported missing, or a key
	// that is read often and never re-stored pins a stale body for good.
	c.put("stale", entry{body: []byte("S"), expires: time.Now().Add(-time.Second)})
	if _, ok := c.get("stale"); ok {
		t.Error("an expired entry was served")
	}
	if _, ok := c.items["stale"]; ok {
		t.Error("an expired entry was left in the map")
	}
}

func TestImageCacheRoundTripAndSweep(t *testing.T) {
	dir := t.TempDir()
	c, err := newImageCache(dir, time.Hour)
	if err != nil {
		t.Fatal(err)
	}
	if err := c.put("w342_poster.jpg", []byte("bytes")); err != nil {
		t.Fatal(err)
	}
	got, ok := c.get("w342_poster.jpg")
	if !ok || string(got) != "bytes" {
		t.Fatalf("round trip failed: %q %v", got, ok)
	}
	// The write goes through a temporary sibling and a rename, so no partial
	// file may be left behind to read later as a valid hit.
	entries, _ := os.ReadDir(dir)
	for _, e := range entries {
		if strings.HasPrefix(e.Name(), ".tmp-") {
			t.Errorf("a temporary file survived the write: %s", e.Name())
		}
	}

	// Past its TTL it is neither served nor kept.
	old := filepath.Join(dir, "w342_poster.jpg")
	past := time.Now().Add(-2 * time.Hour)
	if err := os.Chtimes(old, past, past); err != nil {
		t.Fatal(err)
	}
	if _, ok := c.get("w342_poster.jpg"); ok {
		t.Error("an expired image was served")
	}
	if n, err := c.sweep(); err != nil || n != 1 {
		t.Errorf("sweep removed %d (err %v), want 1", n, err)
	}
}

// Twenty clients opening the same grid must produce one upstream fetch per
// poster, not twenty — which is the burst the 20-connection upstream limit
// cannot absorb.
func TestImageCacheClaimAdmitsOneFlight(t *testing.T) {
	c, err := newImageCache(t.TempDir(), time.Hour)
	if err != nil {
		t.Fatal(err)
	}
	var mu sync.Mutex
	fetchers := 0
	var wg sync.WaitGroup
	start := make(chan struct{})
	for range 20 {
		wg.Add(1)
		go func() {
			defer wg.Done()
			<-start
			mine, done := c.claim("same.jpg")
			if !mine {
				return
			}
			mu.Lock()
			fetchers++
			mu.Unlock()
			// Long enough that the others are certainly waiting on the channel
			// rather than racing past the claim.
			time.Sleep(20 * time.Millisecond)
			done()
		}()
	}
	close(start)
	wg.Wait()
	if fetchers != 1 {
		t.Errorf("%d goroutines fetched the same key; exactly one may", fetchers)
	}
}
