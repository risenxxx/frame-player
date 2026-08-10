package main

import (
	"container/list"
	"os"
	"path/filepath"
	"sync"
	"time"
)

// ---- JSON responses, in memory ---------------------------------------------

// One cached response. The bytes are TMDB's own, verbatim — nothing is parsed
// on the way through, which is what keeps this service ignorant of the schema
// and therefore unaffected by TMDB adding a field.
type entry struct {
	body    []byte
	kind    string // Content-Type, passed through
	expires time.Time
}

// An LRU with a per-entry TTL.
//
// Both halves are needed and they answer different questions. The TTL is the
// *licence*: the TMDB terms forbid caching their content for longer than six
// months, so nothing here may be kept indefinitely even when there is room. The
// LRU is the *machine*: a search is a free-text query, so the key space is
// unbounded and without a cap a long-running instance would hold every string
// anybody ever typed.
//
// Deliberately counted in entries rather than bytes. A TMDB JSON response is a
// few tens of kilobytes with little variance, so entries are a good enough
// proxy for memory and a far cheaper thing to measure on every write.
type jsonCache struct {
	mu    sync.Mutex
	max   int
	items map[string]*list.Element
	order *list.List // front = most recently used
}

type node struct {
	key string
	val entry
}

func newJSONCache(max int) *jsonCache {
	return &jsonCache{max: max, items: map[string]*list.Element{}, order: list.New()}
}

func (c *jsonCache) get(key string) (entry, bool) {
	c.mu.Lock()
	defer c.mu.Unlock()
	el, ok := c.items[key]
	if !ok {
		return entry{}, false
	}
	n := el.Value.(*node)
	// An expired entry is removed rather than merely reported missing: leaving
	// it in place would let a key that is read often and never re-stored pin a
	// stale body at the front of the LRU for good.
	if time.Now().After(n.val.expires) {
		c.order.Remove(el)
		delete(c.items, key)
		return entry{}, false
	}
	c.order.MoveToFront(el)
	return n.val, true
}

func (c *jsonCache) put(key string, val entry) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if el, ok := c.items[key]; ok {
		el.Value.(*node).val = val
		c.order.MoveToFront(el)
		return
	}
	c.items[key] = c.order.PushFront(&node{key: key, val: val})
	for c.order.Len() > c.max {
		oldest := c.order.Back()
		if oldest == nil {
			break
		}
		c.order.Remove(oldest)
		delete(c.items, oldest.Value.(*node).key)
	}
}

func (c *jsonCache) len() int {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.order.Len()
}

// ---- Images, on disk -------------------------------------------------------

// Posters are the whole of the bandwidth here — a grid of twenty at w342 is
// roughly 800 KB against ~25 KB of JSON for the same screen — so they are the
// one thing worth keeping on disk across restarts. They are also immutable:
// a TMDB image path names one rendering of one file and never changes content,
// which is what makes a long TTL correct rather than merely convenient. It is
// still bounded, because the terms bound it.
type imageCache struct {
	dir string
	ttl time.Duration
	// One flight per key. Twenty clients opening the same trending grid at once
	// would otherwise fetch the same twenty posters twenty times each, which is
	// exactly the burst the 20-connection upstream limit cannot absorb.
	mu      sync.Mutex
	waiting map[string]chan struct{}
}

func newImageCache(dir string, ttl time.Duration) (*imageCache, error) {
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return nil, err
	}
	return &imageCache{dir: dir, ttl: ttl, waiting: map[string]chan struct{}{}}, nil
}

// The file this key is stored at. `key` has already been validated by the
// caller against a strict pattern — see `validImagePath` — so it cannot contain
// a separator or a dot segment, and `filepath.Join` cannot be walked out of the
// directory. Validation lives at the edge rather than here because the edge is
// where the untrusted string arrives, and a check applied once at the boundary
// is easier to prove than one repeated at each use.
func (c *imageCache) path(key string) string {
	return filepath.Join(c.dir, key)
}

func (c *imageCache) get(key string) ([]byte, bool) {
	name := c.path(key)
	info, err := os.Stat(name)
	if err != nil || time.Since(info.ModTime()) > c.ttl {
		return nil, false
	}
	body, err := os.ReadFile(name)
	if err != nil {
		return nil, false
	}
	return body, true
}

// Written through a temporary file in the same directory and renamed, so a
// crash or a full disk mid-write cannot leave a truncated image that later
// reads as a valid cache hit. `os.Rename` is atomic within one filesystem,
// which is why the temporary file is a sibling rather than in the system's
// temporary directory.
func (c *imageCache) put(key string, body []byte) error {
	name := c.path(key)
	tmp, err := os.CreateTemp(c.dir, ".tmp-*")
	if err != nil {
		return err
	}
	defer os.Remove(tmp.Name())
	if _, err := tmp.Write(body); err != nil {
		tmp.Close()
		return err
	}
	if err := tmp.Close(); err != nil {
		return err
	}
	return os.Rename(tmp.Name(), name)
}

// Claim the right to fetch `key`, or wait for whoever already has it.
//
// Returns `true` when this caller is the one that must fetch, along with the
// function to call when it is done. Returns `false` after the other flight has
// finished, at which point the cache is worth re-reading.
func (c *imageCache) claim(key string) (mine bool, done func()) {
	c.mu.Lock()
	if ch, ok := c.waiting[key]; ok {
		c.mu.Unlock()
		<-ch
		return false, func() {}
	}
	ch := make(chan struct{})
	c.waiting[key] = ch
	c.mu.Unlock()
	return true, func() {
		c.mu.Lock()
		delete(c.waiting, key)
		c.mu.Unlock()
		close(ch)
	}
}

// Delete cached images past their TTL. Called on a timer rather than on read,
// because a read only ever sees the keys somebody asks for and the whole point
// of the sweep is the ones nobody asks for any more.
func (c *imageCache) sweep() (removed int, err error) {
	entries, err := os.ReadDir(c.dir)
	if err != nil {
		return 0, err
	}
	for _, e := range entries {
		info, err := e.Info()
		if err != nil {
			continue
		}
		if time.Since(info.ModTime()) > c.ttl {
			if err := os.Remove(filepath.Join(c.dir, e.Name())); err == nil {
				removed++
			}
		}
	}
	return removed, nil
}
