// A room: the shared timeline, who is in it, and who is holding it up.
//
// **A mutex, not an actor.** The obvious shape for this is one goroutine per
// room reading a command channel, and it was rejected: its whole advantage is
// that a broadcast cannot deadlock and that ordering is total, and both of
// those hold here anyway — every send into a member's queue is non-blocking, so
// nothing can block while the lock is held, and the lock itself makes ordering
// total. What the actor adds on top is a lifecycle (start, stop, the race
// between the hub handing over a client and the room having exited) that has to
// be got right for nothing. Hold times here are microseconds and a room is at
// most sixteen people.
//
// The one rule that keeps that true: **never do I/O under `mu`**. A member's
// outbox is a buffered channel and a full one costs that member their
// connection, never the room its progress.
package main

import (
	"encoding/json"
	"errors"
	"sync"
	"time"

	"frameplayer/relay/internal/wire"
)

var (
	errRoomFull   = errors.New("room_full")
	errNotAllowed = errors.New("not_allowed")
)

// How long a member may hold the room frozen before it gives up on them.
//
// Without this one stuck client — a player that died mid-buffer, a laptop shut
// while the socket stayed open — pauses everybody else for good, and the room
// has no way to say why. Past the grace they are simply counted as ready; if
// they were genuinely still buffering, their own drift correction catches up
// when they return.
const readyGrace = 45 * time.Second

type room struct {
	code string

	mu       sync.Mutex
	members  map[string]*client
	order    []string // join order: stable display, and host succession
	host     string
	hostOnly bool

	tl  wire.Timeline
	rev int64
	// The current pause is the relay's own (somebody is buffering), so it may
	// be lifted by the relay. A pause a person asked for never is.
	autoPaused bool
	// When each member stopped being ready, for `readyGrace`.
	notReadySince map[string]time.Time

	// Zero while anyone is here; the moment the last member left otherwise.
	// The hub sweeps on it.
	emptySince time.Time
}

func newRoom(code string, now time.Time) *room {
	return &room{
		code:          code,
		members:       map[string]*client{},
		notReadySince: map[string]time.Time{},
		emptySince:    now,
		tl:            wire.Timeline{Speed: 1, At: now.UnixMilli()},
	}
}

// ---- membership -------------------------------------------------------------

func (r *room) join(c *client, maxMembers int, now time.Time) (wire.Welcome, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if len(r.members) >= maxMembers {
		return wire.Welcome{}, errRoomFull
	}
	r.members[c.id] = c
	r.order = append(r.order, c.id)
	r.emptySince = time.Time{}
	if r.host == "" {
		r.host = c.id
	}
	// A member is not ready until they say so: they have a file to open before
	// they can be. Joining therefore freezes the room, which is the correct
	// behaviour — the others should wait for the person who just arrived — and
	// `readyGrace` is what stops it lasting for ever if they never report.
	r.notReadySince[c.id] = now

	w := wire.Welcome{
		T:        "welcome",
		Ver:      wire.ProtocolVersion,
		Room:     r.code,
		Me:       c.id,
		Host:     r.host,
		HostOnly: r.hostOnly,
		Members:  r.membersLocked(now),
		Timeline: r.tl,
		Waiting:  r.waitingLocked(now),
		Now:      now.UnixMilli(),
	}
	// Everyone else hears about the arrival, and about the freeze it caused.
	r.reconcileReadyLocked(now)
	r.broadcastMembersLocked(now)
	return w, nil
}

// leave removes a member and reports whether the room is now empty.
func (r *room) leave(id string, now time.Time) bool {
	r.mu.Lock()
	defer r.mu.Unlock()
	if _, ok := r.members[id]; !ok {
		return len(r.members) == 0
	}
	delete(r.members, id)
	delete(r.notReadySince, id)
	for i, m := range r.order {
		if m == id {
			r.order = append(r.order[:i], r.order[i+1:]...)
			break
		}
	}
	if r.host == id {
		// Succession by join order rather than by nothing: with `hostOnly` on,
		// a room whose host left and whose host field stayed behind is a room
		// where the controls belong to a member id that no longer exists.
		r.host = ""
		if len(r.order) > 0 {
			r.host = r.order[0]
		}
	}
	if len(r.members) == 0 {
		r.emptySince = now
		return true
	}
	// Their leaving may be exactly what unfreezes the room.
	r.reconcileReadyLocked(now)
	r.broadcastMembersLocked(now)
	return false
}

func (r *room) membersLocked(now time.Time) []wire.Member {
	out := make([]wire.Member, 0, len(r.order))
	for _, id := range r.order {
		c, ok := r.members[id]
		if !ok {
			continue
		}
		out = append(out, wire.Member{ID: id, Name: c.name(), Ready: r.readyLocked(id, now)})
	}
	return out
}

// A member counts as ready when they said so, or when they have held the room
// up for longer than anyone should have to wait.
func (r *room) readyLocked(id string, now time.Time) bool {
	since, notReady := r.notReadySince[id]
	return !notReady || now.Sub(since) > readyGrace
}

func (r *room) waitingLocked(now time.Time) []string {
	out := []string{}
	for _, id := range r.order {
		if _, ok := r.members[id]; ok && !r.readyLocked(id, now) {
			out = append(out, id)
		}
	}
	return out
}

// ---- the timeline -----------------------------------------------------------

// stampLocked makes `next` the room's timeline and tells everyone.
//
// `At`, `Rev` and `By` are written here and nowhere else: a client that fills
// them in is simply overwritten, which is what keeps the revision monotonic
// however many people are pressing space at once.
func (r *room) stampLocked(next wire.Timeline, by string, now time.Time) {
	r.rev++
	next.Rev = r.rev
	next.At = now.UnixMilli()
	next.By = by
	r.tl = next
	r.sendAllLocked(wire.NewTimelineMsg(r.tl))
}

func (r *room) setTimeline(from string, next wire.Timeline, now time.Time) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.hostOnly && from != r.host {
		// Refused, and then corrected: the guest changed their own player
		// optimistically before sending, so leaving them on their own value
		// would show a room that disagrees with itself. Re-sending the current
		// timeline is what pulls them back.
		r.sendLocked(from, wire.NewTimelineMsg(r.tl))
		return errNotAllowed
	}
	// A person deciding the pause state takes it out of the relay's hands. If
	// somebody is still buffering, `reconcileReadyLocked` below immediately
	// freezes it again and marks *that* pause as the relay's — so "resume while
	// a guest is loading" correctly means "resume when they are done".
	r.autoPaused = false
	r.stampLocked(next, from, now)
	r.reconcileReadyLocked(now)
	return nil
}

func (r *room) setReady(id string, ready bool, now time.Time) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if _, ok := r.members[id]; !ok {
		return
	}
	was := r.readyLocked(id, now)
	if ready {
		delete(r.notReadySince, id)
	} else if _, already := r.notReadySince[id]; !already {
		r.notReadySince[id] = now
	}
	if was == r.readyLocked(id, now) {
		return
	}
	r.reconcileReadyLocked(now)
	r.broadcastMembersLocked(now)
}

func (r *room) setHostOnly(from string, v bool, now time.Time) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	// Only the host may hand control away or take it back — otherwise a guest
	// could lock everyone else out, the host included.
	if from != r.host {
		return errNotAllowed
	}
	if r.hostOnly == v {
		return nil
	}
	r.hostOnly = v
	r.broadcastMembersLocked(now)
	return nil
}

// reconcileReadyLocked freezes the room while anyone is buffering, and lifts
// its own freeze when nobody is.
//
// Freezing is a real timeline change rather than a flag beside it, and that is
// the whole point: every client already projects the position from `paused`,
// `position` and `at`, so a frozen room needs no special case anywhere — a
// viewer who joins mid-wait sees the same state as everyone else, and drift
// correction keeps working through it.
func (r *room) reconcileReadyLocked(now time.Time) {
	// Nothing is playing: there is no timeline to hold still.
	if r.tl.Content == nil {
		return
	}
	waiting := len(r.waitingLocked(now)) > 0
	switch {
	case waiting && !r.tl.Paused:
		next := r.tl
		next.Position = r.tl.PositionAt(now.UnixMilli())
		next.Paused = true
		r.stampLocked(next, "", now)
		r.autoPaused = true
	case !waiting && r.autoPaused && r.tl.Paused:
		next := r.tl
		next.Paused = false
		r.stampLocked(next, "", now)
		r.autoPaused = false
	}
}

// tick is the sweeper's call: the only thing that can change without an event
// is a member ageing out of `readyGrace`.
func (r *room) tick(now time.Time) {
	r.mu.Lock()
	defer r.mu.Unlock()
	before := r.rev
	// Idempotent, so there is nothing to check first: it changes the room only
	// when somebody has just aged past the grace.
	r.reconcileReadyLocked(now)
	if r.rev != before {
		r.broadcastMembersLocked(now)
	}
}

// ---- sending ----------------------------------------------------------------

func (r *room) broadcastMembersLocked(now time.Time) {
	r.sendAllLocked(wire.MembersMsg{
		T:        "members",
		Members:  r.membersLocked(now),
		Host:     r.host,
		HostOnly: r.hostOnly,
		Waiting:  r.waitingLocked(now),
	})
}

// Encoded once for the whole room. With sixteen members that is fifteen
// marshals saved, but the reason is correctness rather than cost: everyone
// must receive the same bytes, and a per-member encode is a place for them to
// differ.
func (r *room) sendAllLocked(v any) {
	raw, err := json.Marshal(v)
	if err != nil {
		return
	}
	for _, c := range r.members {
		c.send(raw)
	}
}

func (r *room) sendLocked(id string, v any) {
	c, ok := r.members[id]
	if !ok {
		return
	}
	raw, err := json.Marshal(v)
	if err != nil {
		return
	}
	c.send(raw)
}

// ---- read-only views (for tests and /metrics) --------------------------------

func (r *room) size() int {
	r.mu.Lock()
	defer r.mu.Unlock()
	return len(r.members)
}

func (r *room) snapshot() wire.Timeline {
	r.mu.Lock()
	defer r.mu.Unlock()
	return r.tl
}
