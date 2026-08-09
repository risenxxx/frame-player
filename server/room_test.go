package main

import (
	"encoding/json"
	"testing"
	"time"

	"frameplayer/relay/internal/wire"
)

// A member with a deep outbox and no socket. `kill` is a no-op without a
// cancel func, which is what lets a test overflow the outbox on purpose.
func testClient(h *hub, name string) *client {
	return &client{id: wire.NewID(), hub: h, nameStr: name, out: make(chan []byte, 256)}
}

func testHub() *hub { return newHub(defaultConfig()) }

// Messages of one type, oldest first, drained from a member's outbox.
func drainOf(t *testing.T, c *client, kind string) []map[string]any {
	t.Helper()
	var out []map[string]any
	for {
		select {
		case raw := <-c.out:
			var m map[string]any
			if err := json.Unmarshal(raw, &m); err != nil {
				t.Fatalf("bad JSON on the wire: %v", err)
			}
			if m["t"] == kind {
				out = append(out, m)
			}
		default:
			return out
		}
	}
}

func lastOf(t *testing.T, c *client, kind string) map[string]any {
	t.Helper()
	all := drainOf(t, c, kind)
	if len(all) == 0 {
		t.Fatalf("no %q reached %s", kind, c.id)
	}
	return all[len(all)-1]
}

var someContent = json.RawMessage(`{"kind":"url","url":"x"}`)

func playing(pos float64) wire.Timeline {
	return wire.Timeline{Content: someContent, Position: pos, Speed: 1}
}

// A room with two members, both ready, something playing. The state every test
// below starts from, because getting there is four calls of ceremony.
func roomWithTwo(t *testing.T, now time.Time) (*room, *client, *client) {
	t.Helper()
	h := testHub()
	r := newRoom("ABC123", now)
	a, b := testClient(h, "a"), testClient(h, "b")
	if _, err := r.join(a, 16, now); err != nil {
		t.Fatal(err)
	}
	if _, err := r.join(b, 16, now); err != nil {
		t.Fatal(err)
	}
	r.setReady(a.id, true, now)
	r.setReady(b.id, true, now)
	if err := r.setTimeline(a.id, playing(0), now); err != nil {
		t.Fatal(err)
	}
	drainOf(t, a, "timeline")
	drainOf(t, b, "timeline")
	return r, a, b
}

func TestTimelineIsStampedByTheRelay(t *testing.T) {
	now := time.Now()
	r, _, b := roomWithTwo(t, now)

	// A client filling these in must not be believed: two people pressing space
	// at once would otherwise produce two revision 5s and the room would settle
	// on whichever arrived last at each peer.
	lie := playing(30)
	lie.Rev = 9999
	lie.At = 1
	lie.By = "someone-else"
	if err := r.setTimeline(r.host, lie, now); err != nil {
		t.Fatal(err)
	}

	got := lastOf(t, b, "timeline")
	if got["rev"].(float64) >= 9999 {
		t.Errorf("rev = %v — the client's own value was believed", got["rev"])
	}
	if got["at"].(float64) != float64(now.UnixMilli()) {
		t.Errorf("at = %v, want the relay's clock %d", got["at"], now.UnixMilli())
	}
	if got["by"] != r.host {
		t.Errorf("by = %v, want %q", got["by"], r.host)
	}
}

func TestRevisionsOnlyGoUp(t *testing.T) {
	now := time.Now()
	r, a, _ := roomWithTwo(t, now)
	last := r.snapshot().Rev
	for i := range 5 {
		if err := r.setTimeline(a.id, playing(float64(i)), now); err != nil {
			t.Fatal(err)
		}
		rev := r.snapshot().Rev
		if rev <= last {
			t.Fatalf("rev went %d → %d", last, rev)
		}
		last = rev
	}
}

// ---- readiness --------------------------------------------------------------

func TestJoiningFreezesTheRoomUntilTheNewcomerIsReady(t *testing.T) {
	now := time.Now()
	r, a, _ := roomWithTwo(t, now)

	c := testClient(testHub(), "c")
	if _, err := r.join(c, 16, now); err != nil {
		t.Fatal(err)
	}
	if !r.snapshot().Paused {
		t.Fatal("the room kept playing while somebody was still opening the file")
	}
	if !r.autoPaused {
		t.Fatal("the freeze must be marked as the relay's, or it will never be lifted")
	}
	// And the others are told who they are waiting for.
	members := lastOf(t, a, "members")
	waiting, _ := members["waiting"].([]any)
	if len(waiting) != 1 || waiting[0] != c.id {
		t.Fatalf("waiting = %v, want [%s]", members["waiting"], c.id)
	}

	r.setReady(c.id, true, now)
	if r.snapshot().Paused {
		t.Fatal("the room stayed frozen after everyone was ready")
	}
}

func TestFreezingKeepsThePositionItHadReached(t *testing.T) {
	now := time.Now()
	r, _, _ := roomWithTwo(t, now)

	// Ten seconds of playback, then somebody starts buffering.
	later := now.Add(10 * time.Second)
	c := testClient(testHub(), "c")
	if _, err := r.join(c, 16, later); err != nil {
		t.Fatal(err)
	}
	tl := r.snapshot()
	if !tl.Paused {
		t.Fatal("not frozen")
	}
	// The freeze must capture where playback had got to, not where the last
	// human gesture left it — otherwise every buffering hiccup rewinds the room.
	if tl.Position < 9.9 || tl.Position > 10.1 {
		t.Errorf("frozen at %v, want ~10", tl.Position)
	}
}

func TestAHumanPauseSurvivesTheThawing(t *testing.T) {
	now := time.Now()
	r, a, _ := roomWithTwo(t, now)

	c := testClient(testHub(), "c")
	if _, err := r.join(c, 16, now); err != nil { // freezes the room
		t.Fatal(err)
	}
	// Somebody deliberately pauses while the newcomer is still loading.
	paused := playing(5)
	paused.Paused = true
	if err := r.setTimeline(a.id, paused, now); err != nil {
		t.Fatal(err)
	}
	r.setReady(c.id, true, now)

	if !r.snapshot().Paused {
		t.Fatal("a pause somebody asked for was lifted by the relay when buffering ended")
	}
}

func TestResumingWhileSomebodyLoadsMeansResumeWhenTheyAreDone(t *testing.T) {
	now := time.Now()
	r, a, _ := roomWithTwo(t, now)

	c := testClient(testHub(), "c")
	if _, err := r.join(c, 16, now); err != nil { // freezes
		t.Fatal(err)
	}
	// The impatient case: somebody hits play while a guest is still opening.
	if err := r.setTimeline(a.id, playing(5), now); err != nil {
		t.Fatal(err)
	}
	if !r.snapshot().Paused {
		t.Fatal("the room resumed while a member was still loading")
	}
	r.setReady(c.id, true, now)
	if r.snapshot().Paused {
		t.Fatal("the room did not resume once everyone was ready")
	}
}

func TestAStuckMemberStopsHoldingTheRoomUp(t *testing.T) {
	now := time.Now()
	r, _, _ := roomWithTwo(t, now)

	c := testClient(testHub(), "c")
	if _, err := r.join(c, 16, now); err != nil {
		t.Fatal(err)
	}
	if !r.snapshot().Paused {
		t.Fatal("not frozen")
	}
	// Nothing further arrives from them — a player that died mid-buffer, or a
	// laptop shut with the socket still open. Without the grace the others wait
	// for ever with nothing on screen able to explain it.
	r.tick(now.Add(readyGrace + time.Second))
	if r.snapshot().Paused {
		t.Fatal("a member who never reported held the room past the grace")
	}
}

func TestLeavingCanBeWhatUnfreezesTheRoom(t *testing.T) {
	now := time.Now()
	r, _, _ := roomWithTwo(t, now)
	c := testClient(testHub(), "c")
	if _, err := r.join(c, 16, now); err != nil {
		t.Fatal(err)
	}
	if !r.snapshot().Paused {
		t.Fatal("not frozen")
	}
	r.leave(c.id, now)
	if r.snapshot().Paused {
		t.Fatal("the room stayed frozen for somebody who had left")
	}
}

func TestNothingPlayingIsNotFrozen(t *testing.T) {
	now := time.Now()
	h := testHub()
	r := newRoom("ABC123", now)
	a := testClient(h, "a")
	if _, err := r.join(a, 16, now); err != nil {
		t.Fatal(err)
	}
	// `a` has not reported ready, but there is no film to hold still — a freeze
	// here would stamp a revision saying a room with no content is paused.
	if r.snapshot().Rev != 0 {
		t.Errorf("an empty room stamped a timeline (rev %d)", r.snapshot().Rev)
	}
}

// ---- who may drive ----------------------------------------------------------

func TestHostOnlyRefusesAGuestAndCorrectsThem(t *testing.T) {
	now := time.Now()
	r, a, b := roomWithTwo(t, now)
	if err := r.setHostOnly(a.id, true, now); err != nil { // a is the host
		t.Fatal(err)
	}
	drainOf(t, b, "timeline")

	err := r.setTimeline(b.id, playing(600), now)
	if err != errNotAllowed {
		t.Fatalf("guest's seek returned %v, want not_allowed", err)
	}
	if r.snapshot().Position == 600 {
		t.Fatal("the guest moved the room anyway")
	}
	// The guest already moved their own player before sending, so silence would
	// leave them somewhere the room is not.
	back := lastOf(t, b, "timeline")
	if back["position"].(float64) == 600 {
		t.Fatal("the correction sent back the guest's own position")
	}
}

func TestOnlyTheHostMayChangeTheMode(t *testing.T) {
	now := time.Now()
	r, _, b := roomWithTwo(t, now)
	if err := r.setHostOnly(b.id, true, now); err != errNotAllowed {
		t.Fatalf("a guest set the mode: %v", err)
	}
	// ...and the trap this guards: a guest locking everybody out, host included.
	if r.hostOnly {
		t.Fatal("hostOnly was set by a guest")
	}
}

func TestHostSuccession(t *testing.T) {
	now := time.Now()
	r, a, b := roomWithTwo(t, now)
	if r.host != a.id {
		t.Fatalf("host = %s, want the first to join", r.host)
	}
	r.leave(a.id, now)
	if r.host != b.id {
		t.Fatalf("host = %q after the host left, want %s", r.host, b.id)
	}
	// The point of succession: with hostOnly on, a room whose host field named
	// somebody gone is a room nobody can drive.
	if err := r.setHostOnly(b.id, true, now); err != nil {
		t.Fatalf("the new host cannot set the mode: %v", err)
	}
}

func TestRoomFull(t *testing.T) {
	now := time.Now()
	h := testHub()
	r := newRoom("ABC123", now)
	for range 2 {
		if _, err := r.join(testClient(h, "x"), 2, now); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := r.join(testClient(h, "y"), 2, now); err != errRoomFull {
		t.Fatalf("third join returned %v, want room_full", err)
	}
}

// ---- the outbox -------------------------------------------------------------

func TestASlowMemberCostsOnlyThemselves(t *testing.T) {
	now := time.Now()
	h := testHub()
	r := newRoom("ABC123", now)
	fast := testClient(h, "fast")
	slow := &client{id: wire.NewID(), hub: h, nameStr: "slow", out: make(chan []byte, 1)}
	for _, c := range []*client{fast, slow} {
		if _, err := r.join(c, 16, now); err != nil {
			t.Fatal(err)
		}
		r.setReady(c.id, true, now)
	}

	// The slow member's outbox fills immediately and stays full. The room must
	// keep making progress regardless — this is the property that lets it hold
	// a plain mutex.
	for i := range 50 {
		if err := r.setTimeline(fast.id, playing(float64(i)), now); err != nil {
			t.Fatal(err)
		}
	}
	if got := r.snapshot().Position; got != 49 {
		t.Fatalf("the room ended at %v, want 49 — a slow member blocked it", got)
	}
	if _, _, dropped := h.stats(); dropped == 0 {
		t.Error("the slow member was never counted as dropped")
	}
	if len(drainOf(t, fast, "timeline")) < 50 {
		t.Error("the healthy member lost messages because of the slow one")
	}
}
