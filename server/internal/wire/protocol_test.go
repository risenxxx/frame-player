package wire

import (
	"bufio"
	"encoding/json"
	"math"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"
)

// The shared field-name contract. See shared/sync-protocol.txt for what this is
// defending against and why it is a file rather than a constant here.
func loadProtocolFixture(t *testing.T) map[string][]string {
	t.Helper()
	path := filepath.Join("..", "..", "..", "shared", "sync-protocol.txt")
	f, err := os.Open(path)
	if err != nil {
		t.Fatalf("open %s: %v", path, err)
	}
	defer f.Close()

	out := map[string][]string{}
	sc := bufio.NewScanner(f)
	for sc.Scan() {
		line := sc.Text()
		if strings.TrimSpace(line) == "" || strings.HasPrefix(strings.TrimSpace(line), "#") {
			continue
		}
		name, fields, ok := strings.Cut(line, "\t")
		if !ok {
			t.Fatalf("no tab in %q", line)
		}
		out[strings.TrimSpace(name)] = strings.Split(strings.TrimSpace(fields), ",")
	}
	if err := sc.Err(); err != nil {
		t.Fatal(err)
	}
	// A floor on the parse, for the same reason path-under.txt has one: a file
	// that moved, or was reformatted so every line reads as a comment, would
	// otherwise pass in silence — which is the one way a shared contract quietly
	// stops being one.
	if len(out) < 12 {
		t.Fatalf("parsed only %d messages from %s — did the file move or change shape?", len(out), path)
	}
	return out
}

// keysOf marshals a value and reports the field names it actually produced.
// That is the only reading of a Go struct that matches what the other side sees:
// a tag typo or a stray `omitempty` shows up here and nowhere in the type.
func keysOf(t *testing.T, v any) []string {
	t.Helper()
	raw, err := json.Marshal(v)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	var m map[string]json.RawMessage
	if err := json.Unmarshal(raw, &m); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	out := make([]string, 0, len(m))
	for k := range m {
		out = append(out, k)
	}
	return out
}

func assertFields(t *testing.T, want []string, got []string, what string) {
	t.Helper()
	w := append([]string(nil), want...)
	g := append([]string(nil), got...)
	sort.Strings(w)
	sort.Strings(g)
	if strings.Join(w, ",") != strings.Join(g, ",") {
		t.Errorf("%s: fields are %v, shared/sync-protocol.txt says %v", what, g, w)
	}
}

func TestWireFieldsMatchSharedContract(t *testing.T) {
	// **This test must never be served from the build cache**, and getting that
	// wrong would quietly undo the whole contract.
	//
	// `go test` reuses a result when the package's own inputs are unchanged, and
	// the fixture lives outside this module — measured: rename a field in
	// `shared/sync-protocol.txt`, run `go test`, and it answers `(cached)` while
	// the two languages disagree. Which is precisely the failure this file
	// exists to prevent, wearing a different hat.
	//
	// `t.Setenv` marks a test uncacheable, which is the documented way to say
	// so. `-count=1` would work too and is what CI passes, but it is a flag a
	// person has to remember, and the whole point of a shared contract is that
	// it holds without anybody remembering anything.
	t.Setenv("FP_FIXTURE_IS_OUTSIDE_THIS_MODULE", "1")

	fx := loadProtocolFixture(t)
	yes := true

	tl := Timeline{Content: json.RawMessage(`{"kind":"url"}`), Paused: true, Position: 12.5, Speed: 1, At: 7, Rev: 3, By: "m1"}

	cases := []struct {
		name string
		v    any
	}{
		{"timeline", tl},
		{"member", Member{ID: "m1", Name: "n", Ready: true}},

		{"client:hello", ClientMsg{T: "hello", Ver: 1, Room: "ABC123", Name: "n"}},
		{"client:timeline", ClientMsg{T: "timeline", Timeline: &tl}},
		{"client:ready", ClientMsg{T: "ready", Ready: &yes, Reason: "buffering"}},
		{"client:mode", ClientMsg{T: "mode", HostOnly: &yes}},
		{"client:ping", ClientMsg{T: "ping", C: 99}},
		{"client:bye", ClientMsg{T: "bye"}},

		{"server:welcome", Welcome{T: "welcome", Ver: 1, Room: "ABC123", Me: "m1", Host: "m1",
			Members: []Member{}, Timeline: tl, Waiting: []string{}, Now: 1}},
		{"server:timeline", NewTimelineMsg(tl)},
		{"server:members", MembersMsg{T: "members", Members: []Member{}, Host: "m1", Waiting: []string{}}},
		{"server:pong", Pong{T: "pong", C: 1, S: 2}},
		{"server:error", ErrorMsg{T: "error", Code: "no_room", Message: "x"}},
	}

	for _, c := range cases {
		want, ok := fx[c.name]
		if !ok {
			t.Errorf("%s is not in shared/sync-protocol.txt", c.name)
			continue
		}
		assertFields(t, want, keysOf(t, c.v), c.name)
	}
	if len(cases) != len(fx) {
		t.Errorf("shared/sync-protocol.txt lists %d messages, this test covers %d", len(fx), len(cases))
	}
}

// ---- validation -------------------------------------------------------------

func TestValidateRejectsWhatARoomMustNeverSee(t *testing.T) {
	big := json.RawMessage("[" + strings.Repeat("0,", MaxContent/2) + "0]")
	tests := []struct {
		name string
		msg  ClientMsg
	}{
		{"unknown type", ClientMsg{T: "shrug"}},
		{"wrong version", ClientMsg{T: "hello", Ver: ProtocolVersion + 1}},
		{"code that is not a code", ClientMsg{T: "hello", Ver: 1, Room: "no"}},
		{"timeline with no timeline", ClientMsg{T: "timeline"}},
		{"ready with no flag", ClientMsg{T: "ready"}},
		{"mode with no flag", ClientMsg{T: "mode"}},
		{"NaN position", ClientMsg{T: "timeline", Timeline: &Timeline{Position: math.NaN(), Speed: 1}}},
		{"infinite position", ClientMsg{T: "timeline", Timeline: &Timeline{Position: math.Inf(1), Speed: 1}}},
		{"negative position", ClientMsg{T: "timeline", Timeline: &Timeline{Position: -1, Speed: 1}}},
		{"absurd position", ClientMsg{T: "timeline", Timeline: &Timeline{Position: MaxPosition + 1, Speed: 1}}},
		{"zero speed", ClientMsg{T: "timeline", Timeline: &Timeline{Speed: 0}}},
		{"speed past the ceiling", ClientMsg{T: "timeline", Timeline: &Timeline{Speed: MaxSpeed + 1}}},
		{"NaN speed", ClientMsg{T: "timeline", Timeline: &Timeline{Speed: math.NaN()}}},
		{"oversized content", ClientMsg{T: "timeline", Timeline: &Timeline{Content: big, Speed: 1}}},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			msg := tc.msg
			if err := msg.Validate(); err == nil {
				t.Fatalf("accepted %+v", tc.msg)
			}
		})
	}
}

func TestValidateNormalises(t *testing.T) {
	msg := ClientMsg{T: "hello", Ver: 1, Room: " abc-123 ", Name: "  a\x00b  "}
	if err := msg.Validate(); err != nil {
		t.Fatalf("rejected: %v", err)
	}
	if msg.Room != "ABC123" {
		t.Errorf("room = %q, want ABC123", msg.Room)
	}
	if msg.Name != "ab" {
		t.Errorf("name = %q, want %q — control characters must not reach other viewers", msg.Name, "ab")
	}
}

// A JSON `null` and an absent field both mean "nothing is playing", and every
// later `Content == nil` depends on the two having been collapsed into one.
func TestExplicitNullContentBecomesNil(t *testing.T) {
	var msg ClientMsg
	if err := json.Unmarshal([]byte(`{"t":"timeline","timeline":{"content":null,"position":0,"speed":1}}`), &msg); err != nil {
		t.Fatal(err)
	}
	if err := msg.Validate(); err != nil {
		t.Fatal(err)
	}
	if msg.Timeline.Content != nil {
		t.Errorf("content = %q, want nil", msg.Timeline.Content)
	}
}

// ---- projection -------------------------------------------------------------

func TestPositionAt(t *testing.T) {
	content := json.RawMessage(`{"kind":"url"}`)
	playing := Timeline{Content: content, Position: 100, Speed: 1, At: 10_000}

	if got := playing.PositionAt(12_000); got != 102 {
		t.Errorf("two seconds later = %v, want 102", got)
	}
	if got := (Timeline{Content: content, Position: 100, Speed: 2, At: 10_000}).PositionAt(12_000); got != 104 {
		t.Errorf("at double speed = %v, want 104", got)
	}
	paused := playing
	paused.Paused = true
	if got := paused.PositionAt(999_000); got != 100 {
		t.Errorf("paused = %v, want 100 — a paused timeline must not advance", got)
	}
	// Nothing playing: `at` is still stamped, so without the content check the
	// position would run away from a film that does not exist.
	empty := Timeline{Position: 0, Speed: 1, At: 10_000}
	if got := empty.PositionAt(999_000); got != 0 {
		t.Errorf("empty = %v, want 0", got)
	}
	// A clock reading earlier than the stamp is legitimate — a client's offset
	// is an estimate — and the projection simply runs backwards with it.
	if got := playing.PositionAt(8_000); got != 98 {
		t.Errorf("two seconds before the stamp = %v, want 98", got)
	}
	// But it must never come out negative, because the caller seeks to it.
	if got := playing.PositionAt(-999_000); got != 0 {
		t.Errorf("far behind the stamp = %v, want 0", got)
	}
}
