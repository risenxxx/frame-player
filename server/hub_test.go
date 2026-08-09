package main

import (
	"testing"
	"time"
)

func TestCreateRespectsTheRoomCeiling(t *testing.T) {
	cfg := defaultConfig()
	cfg.MaxRooms = 3
	h := newHub(cfg)
	now := time.Now()
	for range 3 {
		if _, err := h.create(now); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := h.create(now); err != errBusy {
		t.Fatalf("created a fourth room: %v", err)
	}
}

func TestGetIsExactAboutTheCode(t *testing.T) {
	h := newHub(defaultConfig())
	r, err := h.create(time.Now())
	if err != nil {
		t.Fatal(err)
	}
	if got, err := h.get(r.code); err != nil || got != r {
		t.Fatalf("get(%q) = %v, %v", r.code, got, err)
	}
	if _, err := h.get("ZZZZZZ"); err != errNoRoom {
		t.Fatalf("a code nobody has returned %v, want no_room", err)
	}
}

// An empty room outliving its last member is the difference between a dropped
// Wi-Fi connection costing a reconnect and costing the evening: the code has
// already been given to other people.
func TestAnEmptyRoomSurvivesItsTTLAndThenGoes(t *testing.T) {
	cfg := defaultConfig()
	cfg.RoomTTL = time.Minute
	h := newHub(cfg)
	now := time.Now()
	r, err := h.create(now)
	if err != nil {
		t.Fatal(err)
	}
	c := testClient(h, "a")
	if _, err := r.join(c, 16, now); err != nil {
		t.Fatal(err)
	}
	r.leave(c.id, now)

	h.sweep(now.Add(30 * time.Second))
	if _, err := h.get(r.code); err != nil {
		t.Fatal("an empty room was collected before its TTL — a reconnect would land nowhere")
	}
	h.sweep(now.Add(2 * time.Minute))
	if _, err := h.get(r.code); err != errNoRoom {
		t.Fatal("an abandoned room was never collected")
	}
}

func TestAnOccupiedRoomIsNeverCollected(t *testing.T) {
	cfg := defaultConfig()
	cfg.RoomTTL = time.Millisecond
	h := newHub(cfg)
	now := time.Now()
	r, err := h.create(now)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := r.join(testClient(h, "a"), 16, now); err != nil {
		t.Fatal(err)
	}
	h.sweep(now.Add(time.Hour))
	if _, err := h.get(r.code); err != nil {
		t.Fatal("a room with somebody in it was collected")
	}
}

// The sweeper is also what notices a member ageing out of `readyGrace` — there
// is no event at that moment, so without this pass the room stays frozen.
func TestSweepUnfreezesARoomHeldByAStuckMember(t *testing.T) {
	h := newHub(defaultConfig())
	now := time.Now()
	r, err := h.create(now)
	if err != nil {
		t.Fatal(err)
	}
	a, stuck := testClient(h, "a"), testClient(h, "stuck")
	if _, err := r.join(a, 16, now); err != nil {
		t.Fatal(err)
	}
	r.setReady(a.id, true, now)
	if err := r.setTimeline(a.id, playing(0), now); err != nil {
		t.Fatal(err)
	}
	if _, err := r.join(stuck, 16, now); err != nil {
		t.Fatal(err)
	}
	if !r.snapshot().Paused {
		t.Fatal("not frozen")
	}
	h.sweep(now.Add(readyGrace + time.Second))
	if r.snapshot().Paused {
		t.Fatal("the sweeper did not lift a freeze whose reason had expired")
	}
}

func TestJoinRateLimit(t *testing.T) {
	cfg := defaultConfig()
	cfg.JoinBurst = 3
	cfg.JoinPerSecond = 1
	h := newHub(cfg)
	now := time.Now()

	for i := range 3 {
		if !h.allowJoin("10.0.0.1", now) {
			t.Fatalf("refused join %d of the burst", i+1)
		}
	}
	if h.allowJoin("10.0.0.1", now) {
		t.Fatal("the burst was not a limit")
	}
	// Another address is unaffected — the limit is per knocker, not global.
	if !h.allowJoin("10.0.0.2", now) {
		t.Fatal("one address exhausted the limit for everybody")
	}
	// And it refills.
	if !h.allowJoin("10.0.0.1", now.Add(2*time.Second)) {
		t.Fatal("the bucket never refilled")
	}
}
