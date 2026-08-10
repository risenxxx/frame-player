// The registry of rooms, and the three bounds that keep one machine's worth of
// relay from being anyone's problem: how many rooms may exist, how long an
// empty one survives, and how fast one address may knock on the door.
package main

import (
	"errors"
	"sync"
	"time"

	"frameplayer/relay/internal/wire"
)

var (
	errNoRoom      = errors.New("no_room")
	errBusy        = errors.New("busy")
	errRateLimited = errors.New("rate_limited")
)

type hub struct {
	cfg Config

	mu    sync.RWMutex
	rooms map[string]*room

	// Joins per address. Not a defence against anyone determined — an address
	// is cheap — but it is what stops a single script from walking the code
	// space, which is the only attack the code length makes worth considering.
	limitMu sync.Mutex
	limits  map[string]*bucket

	// /metrics, and nothing else reads them.
	statsMu   sync.Mutex
	roomsMade int64
	joins     int64
	dropped   int64
}

func newHub(cfg Config) *hub {
	return &hub{cfg: cfg, rooms: map[string]*room{}, limits: map[string]*bucket{}}
}

// An empty room is kept for `RoomTTL` rather than deleted at once, and that is
// the difference between a dropped Wi-Fi connection costing a reconnect and
// costing the evening: the last member to leave may be the only member, on a
// train, and the code they gave their friends still has to work.
func (h *hub) create(now time.Time) (*room, error) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if len(h.rooms) >= h.cfg.MaxRooms {
		return nil, errBusy
	}
	// Collisions are vanishing at 32^6 but not impossible, and taking over a
	// live room would be the worst possible way to find that out.
	for range 8 {
		code := wire.NewCode()
		if _, taken := h.rooms[code]; taken {
			continue
		}
		r := newRoom(code, now)
		h.rooms[code] = r
		h.statsMu.Lock()
		h.roomsMade++
		h.statsMu.Unlock()
		return r, nil
	}
	return nil, errBusy
}

func (h *hub) get(code string) (*room, error) {
	h.mu.RLock()
	defer h.mu.RUnlock()
	r, ok := h.rooms[code]
	if !ok {
		return nil, errNoRoom
	}
	return r, nil
}

func (h *hub) count() int {
	h.mu.RLock()
	defer h.mu.RUnlock()
	return len(h.rooms)
}

// sweep drops rooms nobody came back to, and gives the rest a chance to notice
// that somebody has been "buffering" for longer than anyone should wait.
func (h *hub) sweep(now time.Time) {
	h.mu.Lock()
	var live []*room
	for code, r := range h.rooms {
		r.mu.Lock()
		empty := r.emptySince
		r.mu.Unlock()
		if !empty.IsZero() && now.Sub(empty) > h.cfg.RoomTTL {
			delete(h.rooms, code)
			continue
		}
		live = append(live, r)
	}
	h.mu.Unlock()

	// Outside the registry lock: a room's own lock must never be taken while
	// holding this one in a path that could also go the other way.
	for _, r := range live {
		r.tick(now)
	}

	h.limitMu.Lock()
	for addr, b := range h.limits {
		if now.Sub(b.last) > time.Hour {
			delete(h.limits, addr)
		}
	}
	h.limitMu.Unlock()
}

// ---- rate limiting ----------------------------------------------------------

type bucket struct {
	tokens float64
	last   time.Time
}

func (h *hub) allowJoin(addr string, now time.Time) bool {
	return h.allow("j:"+addr, h.cfg.JoinPerSecond, h.cfg.JoinBurst, now)
}

// Whether this address may be told *whether a room exists*.
//
// The invitation page answers that, which makes it an oracle: without a limit,
// a script could walk the code space at HTTP speed and find live rooms. Its own
// budget rather than the join one, because a page view is not an attempt to
// join and must not spend the allowance a real person needs for mistyping a
// code.
func (h *hub) allowProbe(addr string, now time.Time) bool {
	return h.allow("p:"+addr, h.cfg.ProbePerSecond, h.cfg.ProbeBurst, now)
}

func (h *hub) allow(key string, rate, burst float64, now time.Time) bool {
	h.limitMu.Lock()
	defer h.limitMu.Unlock()
	b, ok := h.limits[key]
	if !ok {
		b = &bucket{tokens: burst, last: now}
		h.limits[key] = b
	}
	b.tokens += now.Sub(b.last).Seconds() * rate
	if b.tokens > burst {
		b.tokens = burst
	}
	b.last = now
	if b.tokens < 1 {
		return false
	}
	b.tokens--
	return true
}

func (h *hub) noteJoin()    { h.statsMu.Lock(); h.joins++; h.statsMu.Unlock() }
func (h *hub) noteDropped() { h.statsMu.Lock(); h.dropped++; h.statsMu.Unlock() }

func (h *hub) stats() (made, joins, dropped int64) {
	h.statsMu.Lock()
	defer h.statsMu.Unlock()
	return h.roomsMade, h.joins, h.dropped
}
