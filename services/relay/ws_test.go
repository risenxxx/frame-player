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
	mux.HandleFunc("GET /favicon.svg", s.serveFavicon)
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

// A code that answers to nothing gets an answer, not an invitation. The room
// exists from the moment the relay issues its code, so the only way to reach a
// well-formed code with no room behind it is that the room has ended — and
// handing somebody a code that will then fail inside the player, with nothing
// on either screen to explain it, is the outcome this replaces.
// A room under a chosen code. The page reports whether a room exists, so any
// test about the *invitation* has to have one — otherwise it is quietly testing
// the "this room has ended" page instead.
func withRoom(t *testing.T, s *server, code string) {
	t.Helper()
	r, err := s.hub.create(time.Now())
	if err != nil {
		t.Fatal(err)
	}
	s.hub.mu.Lock()
	delete(s.hub.rooms, r.code)
	r.code = code
	s.hub.rooms[code] = r
	s.hub.mu.Unlock()
}

func TestAPageForARoomThatHasEnded(t *testing.T) {
	s, _ := startRelay(t, nil)
	render := func(code string) string {
		rec := httptest.NewRecorder()
		req := httptest.NewRequest("GET", "/j/"+code, nil)
		req.SetPathValue("code", code)
		req.Header.Set("Accept-Language", "en")
		s.serveJoinPage(rec, req)
		return rec.Body.String()
	}

	// Nothing was ever created under this code.
	gone := render("ZZZZZZ")
	if !strings.Contains(gone, "This room has ended") {
		t.Error("an invitation was offered for a room that does not exist")
	}
	if strings.Contains(gone, "frameplayer://join/") {
		t.Error("the page still offers to open a room that is not there")
	}

	// A live room still gets the invitation.
	room, err := s.hub.create(time.Now())
	if err != nil {
		t.Fatal(err)
	}
	live := render(room.code)
	if !strings.Contains(live, "frameplayer://join/"+room.code) {
		t.Error("a live room was not offered")
	}
	if strings.Contains(live, "This room has ended") {
		t.Error("a live room was reported as ended")
	}
}

// The check makes the page an oracle, so it is a courtesy withdrawn under
// abuse: past the probe budget the existence check is simply not made and the
// invitation renders as it always did. A script gets a burst of truths and then
// noise; a person refreshing never notices there was a limit.
func TestWalkingTheCodeSpaceGetsNoise(t *testing.T) {
	s, _ := startRelay(t, func(c *Config) { c.ProbeBurst = 3; c.ProbePerSecond = 0.01 })
	render := func() string {
		rec := httptest.NewRecorder()
		req := httptest.NewRequest("GET", "/j/ZZZZZZ", nil)
		req.SetPathValue("code", "ZZZZZZ")
		req.Header.Set("Accept-Language", "en")
		req.RemoteAddr = "10.9.9.9:1234"
		s.serveJoinPage(rec, req)
		return rec.Body.String()
	}
	truths := 0
	for range 3 {
		if strings.Contains(render(), "This room has ended") {
			truths++
		}
	}
	if truths != 3 {
		t.Fatalf("%d of the first three probes told the truth, want 3", truths)
	}
	// Past the budget it stops answering the question rather than erroring:
	// a 429 to a human who refreshed too often would be the worse trade.
	if after := render(); strings.Contains(after, "This room has ended") {
		t.Error("the page kept answering whether a room exists past the probe budget")
	} else if !strings.Contains(after, "frameplayer://join/") {
		t.Error("past the budget the page should fall back to the plain invitation")
	}
}

func TestJoinPageAcceptsWhatAPersonTypes(t *testing.T) {
	s, _ := startRelay(t, nil)
	// Rooms have to exist, or the page answers "ended" and never prints the
	// scheme link this is about.
	withRoom(t, s, "ABC123")
	withRoom(t, s, "ABC023")
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

// The invitation page is the one thing here a person who does not have the
// player ever sees, so what it says has to follow the browser rather than the
// server's own idea of a language.
func TestJoinPageSpeaksTheVisitorsLanguage(t *testing.T) {
	s, _ := startRelay(t, func(c *Config) {
		c.DownloadWin = "https://example.invalid/FramePlayer-setup.exe"
		c.DownloadPage = "https://example.invalid/download"
	})
	withRoom(t, s, "ABC123")

	page := func(lang, ua string) string {
		rec := httptest.NewRecorder()
		req := httptest.NewRequest("GET", "/j/ABC123", nil)
		req.SetPathValue("code", "ABC123")
		req.Header.Set("Accept-Language", lang)
		req.Header.Set("User-Agent", ua)
		s.serveJoinPage(rec, req)
		if rec.Code != 200 {
			t.Fatalf("%s → %d", lang, rec.Code)
		}
		return rec.Body.String()
	}

	ru := page("ru-RU,ru;q=0.9,en;q=0.8", "Mozilla/5.0 (Windows NT 10.0)")
	if !strings.Contains(ru, `lang="ru"`) || !strings.Contains(ru, "Смотрим вместе") {
		t.Error("a Russian browser was answered in English")
	}
	// The tag that comes first wins, which is what a browser's ordering means.
	if got := page("en-GB,en;q=0.9,ru;q=0.8", ""); !strings.Contains(got, `lang="en"`) {
		t.Error("an English browser was not answered in English")
	}
	// Anything else, including nothing at all: a link is forwarded far more
	// often than it is generated.
	for _, header := range []string{"", "de-DE", "zh-CN,zh;q=0.9"} {
		if got := page(header, ""); !strings.Contains(got, `lang="en"`) {
			t.Errorf("Accept-Language %q was not answered in English", header)
		}
	}

	// The installer for the platform that asked, and the page as the fallback.
	if !strings.Contains(ru, "FramePlayer-setup.exe") {
		t.Error("a Windows visitor was not offered the Windows installer")
	}
	if got := page("en", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15)"); !strings.Contains(got, "example.invalid/download") {
		t.Error("a Mac visitor with no Mac installer configured was not offered the page")
	}
	// A shared cache must not hand one visitor's language to the next.
	rec := httptest.NewRecorder()
	req := httptest.NewRequest("GET", "/j/ABC123", nil)
	req.SetPathValue("code", "ABC123")
	s.serveJoinPage(rec, req)
	if v := rec.Header().Get("Vary"); !strings.Contains(v, "Accept-Language") {
		t.Errorf("Vary = %q", v)
	}
}

// The invitation page is something anyone can be linked to, so it stays a
// document: no script, no framework, nothing that runs. The typeface is the one
// thing it fetches, and that is a deliberate exception rather than a drift —
// see the note at the top of page.go.
func TestTheInvitationPageIsADocument(t *testing.T) {
	s, _ := startRelay(t, nil)
	rec := httptest.NewRecorder()
	req := httptest.NewRequest("GET", "/j/ABC123", nil)
	req.SetPathValue("code", "ABC123")
	s.serveJoinPage(rec, req)
	body := rec.Body.String()
	if strings.Contains(body, "<script") || strings.Contains(body, "onload=") {
		t.Error("the page runs something")
	}
	if !strings.Contains(body, "fonts.googleapis.com/css2?family=Rubik") {
		t.Error("the page is not set in the typeface the player uses")
	}
	// One host, and it is the font service. Anything else appearing here is a
	// dependency somebody added without deciding to.
	for _, off := range []string{"analytics", "gtag", "doubleclick", "cdn.jsdelivr", "unpkg"} {
		if strings.Contains(body, off) {
			t.Errorf("the page reaches %q", off)
		}
	}
}

// The one visitor this page exists for is the one who does not have the player,
// and until a default was set they were shown a code and nothing to do with it:
// the download link only rendered when an operator had configured a URL, which
// nobody had.
func TestSomebodyWithoutThePlayerIsGivenSomewhereToGo(t *testing.T) {
	s, _ := startRelay(t, nil)
	render := func() string {
		rec := httptest.NewRecorder()
		req := httptest.NewRequest("GET", "/j/ABC123", nil)
		req.SetPathValue("code", "ABC123")
		s.serveJoinPage(rec, req)
		return rec.Body.String()
	}
	page := render()
	if !strings.Contains(page, defaultDownloadPage) {
		t.Error("an out-of-the-box relay offers no way to get the player")
	}
	// In a tab of its own: the default is a page, and following it in place
	// would take the code away from somebody who has not used it yet.
	if !strings.Contains(page, `target="_blank"`) {
		t.Error("the download link navigates away from the code")
	}

	// ...and an operator who wants it gone can still say so. Checked on the
	// download link's own class rather than on "any http link": the page also
	// carries the font stylesheet, so the broad test passed for the wrong reason
	// and would have gone on passing if the link had never been removed.
	s.cfg.DownloadPage = ""
	if strings.Contains(render(), `class="get"`) {
		t.Error("clearing the download page left a link behind")
	}
}

// A product name broken across two lines reads as two products. Everything else
// on the page is left to text-wrap; this pair is worth forbidding outright, and
// the assertion has to be written with explicit escapes — the two spellings are
// indistinguishable in a source file, which is exactly how the first version of
// this test came to compare a string with itself.
func TestTheProductNameNeverBreaks(t *testing.T) {
	const joined = "Frame\u00a0Player"
	const broken = "Frame Player"
	s, _ := startRelay(t, nil)
	for _, lang := range []string{"ru", "en"} {
		rec := httptest.NewRecorder()
		req := httptest.NewRequest("GET", "/j/ABC123", nil)
		req.SetPathValue("code", "ABC123")
		req.Header.Set("Accept-Language", lang)
		s.serveJoinPage(rec, req)
		body := rec.Body.String()
		if !strings.Contains(body, joined) {
			t.Errorf("%s: the product name is missing or breakable", lang)
		}
		if strings.Contains(body, broken) {
			t.Errorf("%s: an ordinary space survives inside the product name", lang)
		}
	}
}

func TestTheTabIcon(t *testing.T) {
	s, _ := startRelay(t, nil)
	rec := httptest.NewRecorder()
	s.serveFavicon(rec, httptest.NewRequest("GET", "/favicon.svg", nil))
	if rec.Code != 200 {
		t.Fatalf("→ %d", rec.Code)
	}
	if ct := rec.Header().Get("Content-Type"); ct != "image/svg+xml" {
		t.Errorf("content type %q — a browser will not render it as an icon", ct)
	}
	icon := rec.Body.String()
	// The player's own accent, not a colour drifted from it: the mark is the
	// same glyph the application shows in its own title bar.
	if !strings.Contains(icon, "#6366f1") {
		t.Error("the icon is not drawn in the player's accent")
	}
	// No tile and no background rectangle. What sits behind the glyph has to be
	// the tab bar's own colour, or the icon is right on one theme and a dark
	// smudge on the other.
	if strings.Contains(icon, "<rect") || strings.Contains(icon, "background") {
		t.Error("the icon carries a background, which only works on one theme")
	}
	// A vector answers 16, 32 and 64 from the same bytes, which is the whole
	// reason there is one file rather than a set.
	if !strings.Contains(icon, "viewBox") {
		t.Error("the icon does not scale")
	}

	page := httptest.NewRecorder()
	req := httptest.NewRequest("GET", "/j/ABC123", nil)
	req.SetPathValue("code", "ABC123")
	s.serveJoinPage(page, req)
	if !strings.Contains(page.Body.String(), `href="/favicon.svg"`) {
		t.Error("the page does not point at the icon this relay serves")
	}
}

// The invitation page's markup and CSS live in a raw string literal, and a
// backtick anywhere inside one ends it. That has broken the build three times
// while writing comments about CSS, where backticks around property names are
// the natural way to write. The compiler catches it, but only after the fact and
// with an error pointing at whatever line the string now runs into; this says
// what actually happened.
func TestTheTemplateHasNoBackticks(t *testing.T) {
	s, _ := startRelay(t, nil)
	rec := httptest.NewRecorder()
	req := httptest.NewRequest("GET", "/j/ABC123", nil)
	req.SetPathValue("code", "ABC123")
	s.serveJoinPage(rec, req)
	if strings.Contains(rec.Body.String(), "`") {
		t.Error("a backtick reached the rendered page — the raw string literal it lives in cannot contain one")
	}
}

// What made the page jump in a browser that injects into the document: an
// element appended to body became a second grid item, the grid grew a row, and
// the centred content moved into the upper half.
func TestExtensionsCannotReachTheLayout(t *testing.T) {
	s, _ := startRelay(t, nil)
	rec := httptest.NewRecorder()
	req := httptest.NewRequest("GET", "/j/ABC123", nil)
	req.SetPathValue("code", "ABC123")
	s.serveJoinPage(rec, req)
	body := rec.Body.String()

	// The centring happens in an element of ours, so anything appended to body
	// lands after it in normal flow rather than inside the layout.
	if !strings.Contains(body, `<body><div class="page">`) {
		t.Error("the content is no longer wrapped, so body is a layout container for whatever gets injected into it")
	}
	// And body itself must stay an ordinary block: making it a flex or grid
	// container is what turned an injected sibling into a layout participant.
	for _, laid := range []string{"body {\n    margin: 0;\n    display: grid", "body {\n    margin: 0;\n    display: flex"} {
		if strings.Contains(body, laid) {
			t.Error("body is a layout container again")
		}
	}
}
