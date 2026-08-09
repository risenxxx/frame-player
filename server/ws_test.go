package main

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/coder/websocket"

	"frameplayer/relay/internal/wire"
)

// The unit tests above drive rooms directly; this drives the relay the way the
// player does — over a real socket, through the handshake, with JSON on the
// wire. It is what catches everything that lives between the two: a field the
// handshake forgets to send, a refusal the client never sees because the close
// frame won the race, a message type wired to the wrong room call.

type peer struct {
	t    *testing.T
	conn *websocket.Conn
	ctx  context.Context
	me   string
	room string
}

func startRelay(t *testing.T, tweak func(*Config)) (*server, string) {
	t.Helper()
	cfg := defaultConfig()
	if tweak != nil {
		tweak(&cfg)
	}
	s := &server{cfg: cfg, hub: newHub(cfg)}
	mux := http.NewServeMux()
	mux.HandleFunc("GET /ws", s.serveWS)
	mux.HandleFunc("GET /j/{code}", s.serveJoinPage)
	ts := httptest.NewServer(mux)
	t.Cleanup(ts.Close)
	return s, "ws" + strings.TrimPrefix(ts.URL, "http")
}

func dial(t *testing.T, url string) *peer {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	t.Cleanup(cancel)
	conn, _, err := websocket.Dial(ctx, url+"/ws", nil)
	if err != nil {
		t.Fatalf("dial: %v", err)
	}
	t.Cleanup(func() { _ = conn.CloseNow() })
	return &peer{t: t, conn: conn, ctx: ctx}
}

func (p *peer) send(v any) {
	p.t.Helper()
	raw, err := json.Marshal(v)
	if err != nil {
		p.t.Fatal(err)
	}
	if err := p.conn.Write(p.ctx, websocket.MessageText, raw); err != nil {
		p.t.Fatalf("write: %v", err)
	}
}

// next reads until a message of this type arrives, or the test's deadline does.
func (p *peer) next(kind string) map[string]any {
	p.t.Helper()
	for {
		ctx, cancel := context.WithTimeout(p.ctx, 3*time.Second)
		_, raw, err := p.conn.Read(ctx)
		cancel()
		if err != nil {
			p.t.Fatalf("waiting for %q: %v", kind, err)
		}
		var m map[string]any
		if err := json.Unmarshal(raw, &m); err != nil {
			p.t.Fatalf("bad JSON: %v", err)
		}
		if m["t"] == kind {
			return m
		}
	}
}

// collect reads until one of each requested kind has arrived.
//
// Order-independent on purpose: a refusal is two messages — the error and the
// correction that pulls the guest back — and which of them the relay writes
// first is an implementation detail no client may depend on.
func (p *peer) collect(kinds ...string) map[string]map[string]any {
	p.t.Helper()
	out := map[string]map[string]any{}
	for len(out) < len(kinds) {
		ctx, cancel := context.WithTimeout(p.ctx, 3*time.Second)
		_, raw, err := p.conn.Read(ctx)
		cancel()
		if err != nil {
			p.t.Fatalf("waiting for %v, have %v: %v", kinds, keysOfMap(out), err)
		}
		var m map[string]any
		if err := json.Unmarshal(raw, &m); err != nil {
			p.t.Fatalf("bad JSON: %v", err)
		}
		for _, k := range kinds {
			if m["t"] == k {
				out[k] = m
			}
		}
	}
	return out
}

func keysOfMap(m map[string]map[string]any) []string {
	out := make([]string, 0, len(m))
	for k := range m {
		out = append(out, k)
	}
	return out
}

// hello performs the handshake. An empty code creates a room.
func (p *peer) hello(code, name string) map[string]any {
	p.t.Helper()
	p.send(wire.ClientMsg{T: "hello", Ver: wire.ProtocolVersion, Room: code, Name: name})
	w := p.next("welcome")
	p.me, _ = w["me"].(string)
	p.room, _ = w["room"].(string)
	return w
}

func TestTwoPeopleShareATimeline(t *testing.T) {
	_, url := startRelay(t, nil)

	host := dial(t, url)
	w := host.hello("", "host")
	if !wire.ValidCode(host.room) {
		t.Fatalf("welcome carried room %q, which is not a code", host.room)
	}
	if w["ver"].(float64) != wire.ProtocolVersion {
		t.Errorf("welcome ver = %v", w["ver"])
	}
	if w["now"].(float64) == 0 {
		t.Error("welcome carried no clock, so a client has no offset until its first ping")
	}
	if w["host"] != host.me {
		t.Errorf("the first member is not the host")
	}

	guest := dial(t, url)
	// Typed the way a person would: lower case, with a dash in it.
	guest.hello(strings.ToLower(host.room[:3]+"-"+host.room[3:]), "guest")
	if guest.room != host.room {
		t.Fatalf("guest landed in %q, host is in %q", guest.room, host.room)
	}

	host.send(wire.ClientMsg{T: "ready", Ready: ptr(true)})
	guest.send(wire.ClientMsg{T: "ready", Ready: ptr(true)})

	content := json.RawMessage(`{"kind":"url","url":"https://example.invalid/v"}`)
	host.send(wire.ClientMsg{T: "timeline", Timeline: &wire.Timeline{
		Content: content, Paused: false, Position: 42.5, Speed: 1,
	}})

	got := guest.next("timeline")
	if got["position"].(float64) != 42.5 {
		t.Errorf("position = %v, want 42.5", got["position"])
	}
	if got["by"] != host.me {
		t.Errorf("by = %v, want %s", got["by"], host.me)
	}
	if got["rev"].(float64) == 0 {
		t.Error("rev = 0 — the relay did not stamp it")
	}
	// Content travels untouched: the relay never parses it, which is what lets
	// a new kind of source ship without redeploying this.
	raw, _ := json.Marshal(got["content"])
	if !strings.Contains(string(raw), "example.invalid") {
		t.Errorf("content = %s", raw)
	}
}

func TestPongEchoesTheClientsOwnReading(t *testing.T) {
	_, url := startRelay(t, nil)
	p := dial(t, url)
	p.hello("", "a")

	before := time.Now().UnixMilli()
	p.send(wire.ClientMsg{T: "ping", C: 1234567})
	pong := p.next("pong")
	if pong["c"].(float64) != 1234567 {
		t.Errorf("c = %v — the client's reading must come back untouched, it is what the round trip is measured against", pong["c"])
	}
	if s := int64(pong["s"].(float64)); s < before {
		t.Errorf("s = %d, want the relay's clock at or after %d", s, before)
	}
}

func TestJoiningARoomThatIsNotThere(t *testing.T) {
	_, url := startRelay(t, nil)
	p := dial(t, url)
	p.send(wire.ClientMsg{T: "hello", Ver: wire.ProtocolVersion, Room: "ZZZZZZ"})
	// The refusal has to *arrive*: "the room does not exist" and "the relay is
	// unreachable" are different problems and only one of them is worth
	// retyping the code for.
	if got := p.next("error"); got["code"] != "no_room" {
		t.Errorf("code = %v, want no_room", got["code"])
	}
}

func TestAClientFromTheFutureIsTurnedAway(t *testing.T) {
	_, url := startRelay(t, nil)
	p := dial(t, url)
	p.send(wire.ClientMsg{T: "hello", Ver: wire.ProtocolVersion + 1})
	if got := p.next("error"); got["code"] != "bad_version" {
		t.Errorf("code = %v, want bad_version", got["code"])
	}
}

func TestSayingNothingCostsTheConnectionAndNotASlot(t *testing.T) {
	s, url := startRelay(t, func(c *Config) { c.HandshakeTimeout = 150 * time.Millisecond })
	p := dial(t, url)
	// No hello at all — an open socket that never introduces itself.
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if _, _, err := p.conn.Read(ctx); err == nil {
		t.Fatal("the connection was left open")
	}
	if n := s.hub.count(); n != 0 {
		t.Errorf("%d rooms exist after a connection that never spoke", n)
	}
}

func TestHostOnlyOverTheWire(t *testing.T) {
	_, url := startRelay(t, nil)
	host := dial(t, url)
	host.hello("", "host")
	guest := dial(t, url)
	guest.hello(host.room, "guest")
	host.send(wire.ClientMsg{T: "ready", Ready: ptr(true)})
	guest.send(wire.ClientMsg{T: "ready", Ready: ptr(true)})

	content := json.RawMessage(`{"kind":"url","url":"x"}`)
	host.send(wire.ClientMsg{T: "timeline", Timeline: &wire.Timeline{Content: content, Position: 10, Speed: 1}})
	guest.next("timeline")

	host.send(wire.ClientMsg{T: "mode", HostOnly: ptr(true)})
	guest.next("members")

	guest.send(wire.ClientMsg{T: "timeline", Timeline: &wire.Timeline{Content: content, Position: 900, Speed: 1}})
	// Two things have to arrive: the refusal, and the correction that pulls the
	// guest back — they already moved their own player before sending, so
	// refusing in silence would leave them somewhere the room is not.
	got := guest.collect("error", "timeline")
	if got["error"]["code"] != "not_allowed" {
		t.Errorf("code = %v, want not_allowed", got["error"]["code"])
	}
	if got["timeline"]["position"].(float64) == 900 {
		t.Error("the guest was left at their own position")
	}
}

func TestReadinessFreezesAndThawsOverTheWire(t *testing.T) {
	_, url := startRelay(t, nil)
	host := dial(t, url)
	host.hello("", "host")
	host.send(wire.ClientMsg{T: "ready", Ready: ptr(true)})
	host.send(wire.ClientMsg{T: "timeline", Timeline: &wire.Timeline{
		Content: json.RawMessage(`{"kind":"url","url":"x"}`), Position: 5, Speed: 1,
	}})
	host.next("timeline")

	guest := dial(t, url)
	guest.hello(host.room, "guest")
	// The newcomer has a file to open, so the room stops for them.
	if frozen := host.next("timeline"); frozen["paused"] != true {
		t.Fatal("the room kept playing while somebody was opening the file")
	}
	guest.send(wire.ClientMsg{T: "ready", Ready: ptr(true)})
	if thawed := host.next("timeline"); thawed["paused"] != false {
		t.Fatal("the room did not resume once everybody was ready")
	}
}

func TestJoinPageAcceptsWhatAPersonTypes(t *testing.T) {
	s, _ := startRelay(t, nil)
	for _, path := range []string{"/j/abc-123", "/j/ABC123", "/j/abcI23"} {
		rec := httptest.NewRecorder()
		req := httptest.NewRequest("GET", path, nil)
		req.SetPathValue("code", strings.TrimPrefix(path, "/j/"))
		s.serveJoinPage(rec, req)
		if rec.Code != 200 {
			t.Errorf("%s → %d", path, rec.Code)
			continue
		}
		if !strings.Contains(rec.Body.String(), "frameplayer://join/ABC123") {
			t.Errorf("%s did not offer the normalised code", path)
		}
	}
	rec := httptest.NewRecorder()
	req := httptest.NewRequest("GET", "/j/nope", nil)
	req.SetPathValue("code", "nope")
	s.serveJoinPage(rec, req)
	if rec.Code != 404 {
		t.Errorf("a non-code returned %d", rec.Code)
	}
}

func ptr[T any](v T) *T { return &v }
