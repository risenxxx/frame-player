// The wire, and the whole of what the relay understands.
//
// Mirrored by `src/lib/sync/protocol.ts`. The two are kept honest by
// `shared/sync-protocol.jsonl`: canonical messages that both this package's
// tests and vitest decode and assert on, so a field renamed on one side turns
// the other side red. That is the same device as `shared/path-under.txt`, and
// it exists for the same reason — a rename here costs no error anywhere, it
// costs a room where nothing happens.
//
// **The relay is deliberately content-agnostic.** What is playing travels as
// `json.RawMessage` and is never parsed here: the sender writes a `ContentRef`,
// the receivers read one, and the relay only bounds its size. So a new kind of
// source — a new protocol, a new identity scheme — is a frontend change and
// never a redeploy of this.
package wire

import (
	"encoding/json"
	"errors"
	"math"
	"strings"
	"unicode/utf8"
)

// Protocol version. Bumped only for a change a peer cannot ignore; the relay
// refuses a client that speaks a different major.
const ProtocolVersion = 1

// Bounds. Every one of these is a refusal rather than a truncation: a message
// that does not fit is a bug or an attack, and silently keeping half of it is
// how the second becomes the first.
const (
	// A whole frame. Content is the only variable-length part and is capped
	// below at a quarter of this, so the rest is fixed overhead.
	MaxFrameBytes = 8 << 10
	MaxContent    = 4 << 10
	MaxNameRunes  = 32
	// mpv accepts far more, but a shared timeline running at 8x is nobody's
	// idea of watching together, and an unbounded speed makes the projection
	// meaningless.
	MinSpeed = 0.25
	MaxSpeed = 4.0
	// A film longer than this is not a film. Guards the projection against a
	// position that would overflow into nonsense.
	MaxPosition = 24 * 60 * 60
)

// Timeline is what the room agrees on: what is playing, and where in it.
//
// It is a snapshot of the state *after* a change, never a delta — which is what
// makes it idempotent, survivable across a dropped message, and directly
// readable by drift correction. `At`, `Rev` and `By` are the relay's to fill;
// a client that sets them is ignored.
type Timeline struct {
	// What is playing, opaque here. `null` means nothing is.
	Content json.RawMessage `json:"content"`
	Paused  bool            `json:"paused"`
	// Seconds into the file at `At`.
	Position float64 `json:"position"`
	Speed    float64 `json:"speed"`
	// Relay clock, milliseconds, when this snapshot was stamped.
	At int64 `json:"at"`
	// Monotonic within a room. A client applies only a strictly higher one,
	// which is what makes reordering and duplication harmless.
	Rev int64 `json:"rev"`
	// Member who caused it, or "" for the relay itself (the readiness freeze).
	By string `json:"by"`
}

// Where the timeline says playback is at `nowMs`.
//
// The one piece of arithmetic both ends run, and the reason `At` is a relay
// timestamp rather than a local one: every client knows its own offset from
// this clock and nothing has to agree about wall time.
func (t Timeline) PositionAt(nowMs int64) float64 {
	if t.Paused || t.Content == nil {
		return t.Position
	}
	pos := t.Position + float64(nowMs-t.At)/1000*t.Speed
	if pos < 0 {
		return 0
	}
	return pos
}

// Member is one participant, as everyone else sees them.
type Member struct {
	ID   string `json:"id"`
	Name string `json:"name"`
	// False while this member is buffering or opening. A room with anyone not
	// ready is frozen — see `room.reconcileReady`.
	Ready bool `json:"ready"`
}

// ---- client → relay ---------------------------------------------------------

// ClientMsg is every inbound message in one shape.
//
// One struct rather than a discriminated decode because the alternative is two
// passes over the same bytes for a handful of fields. Which fields are
// meaningful depends on `T`, and `Validate` is where that is settled — once,
// before anything reaches a room.
type ClientMsg struct {
	T string `json:"t"`

	// hello
	Ver  int    `json:"ver,omitempty"`
	Room string `json:"room,omitempty"`
	Name string `json:"name,omitempty"`

	// timeline
	Timeline *Timeline `json:"timeline,omitempty"`

	// ready
	Ready  *bool  `json:"ready,omitempty"`
	Reason string `json:"reason,omitempty"`

	// mode
	HostOnly *bool `json:"hostOnly,omitempty"`

	// ping — the client's own clock reading, echoed back untouched.
	C int64 `json:"c,omitempty"`
}

var (
	ErrBadMessage = errors.New("bad_message")
	ErrBadVersion = errors.New("bad_version")
)

// Validate rejects what a room must never see. Errors are protocol codes, so
// the client can say something specific instead of "connection failed".
func (m *ClientMsg) Validate() error {
	switch m.T {
	case "hello":
		if m.Ver != ProtocolVersion {
			return ErrBadVersion
		}
		m.Name = sanitizeName(m.Name)
		// Normalised rather than merely checked: the code arrives as a person
		// typed it, spacing, case, dashes and look-alike glyphs included, and
		// `NormalizeCode` is the one place that knows what those mean. An empty
		// field is "make me a room" and is not a failed code.
		if strings.TrimSpace(m.Room) != "" {
			if m.Room = NormalizeCode(m.Room); m.Room == "" {
				return ErrBadMessage
			}
		} else {
			m.Room = ""
		}
	case "timeline":
		if m.Timeline == nil {
			return ErrBadMessage
		}
		return m.Timeline.validate()
	case "ready":
		if m.Ready == nil {
			return ErrBadMessage
		}
		m.Reason = truncate(sanitizeName(m.Reason), 24)
	case "mode":
		if m.HostOnly == nil {
			return ErrBadMessage
		}
	case "ping", "bye":
		// nothing to check
	default:
		return ErrBadMessage
	}
	return nil
}

func (t *Timeline) validate() error {
	if len(t.Content) > MaxContent {
		return ErrBadMessage
	}
	// A JSON `null` decodes into a 4-byte RawMessage rather than a nil one, and
	// the two mean the same thing here: nothing is playing. Collapsing them
	// once is what keeps every later `== nil` honest.
	if isJSONNull(t.Content) {
		t.Content = nil
	}
	if math.IsNaN(t.Position) || math.IsInf(t.Position, 0) ||
		t.Position < 0 || t.Position > MaxPosition {
		return ErrBadMessage
	}
	if math.IsNaN(t.Speed) || t.Speed < MinSpeed || t.Speed > MaxSpeed {
		return ErrBadMessage
	}
	return nil
}

func isJSONNull(raw json.RawMessage) bool {
	return len(raw) == 0 || string(raw) == "null"
}

// A name is shown to other people, so it may not carry control characters that
// would let it impersonate the surrounding UI. Empty is legal — the frontend
// names an unnamed member itself, in the viewer's own language, which is
// something the relay cannot do.
func sanitizeName(s string) string {
	s = strings.Map(func(r rune) rune {
		if r < 0x20 || r == 0x7f {
			return -1
		}
		return r
	}, s)
	return truncate(strings.TrimSpace(s), MaxNameRunes)
}

func truncate(s string, runes int) string {
	if utf8.RuneCountInString(s) <= runes {
		return s
	}
	out := []rune(s)
	return string(out[:runes])
}

// ---- relay → client ---------------------------------------------------------
//
// One struct per outbound message rather than the inbound catch-all, because
// here `omitempty` is a trap: a false `hostOnly` or a zero `rev` would vanish
// from the wire and the receiver would read a default that means something
// else. These marshal every field they declare.

type Welcome struct {
	T        string   `json:"t"`
	Ver      int      `json:"ver"`
	Room     string   `json:"room"`
	Me       string   `json:"me"`
	Host     string   `json:"host"`
	HostOnly bool     `json:"hostOnly"`
	Members  []Member `json:"members"`
	Timeline Timeline `json:"timeline"`
	Waiting  []string `json:"waiting"`
	// The relay's clock at the moment this was written, so a client has a
	// usable offset before its first ping round trip completes.
	Now int64 `json:"now"`
}

// TimelineMsg embeds the timeline so it marshals flat — `{"t":"timeline",
// "content":…,"paused":…}` — rather than nesting it under a key the receiver
// would have to reach through.
type TimelineMsg struct {
	T string `json:"t"`
	Timeline
}

type MembersMsg struct {
	T        string   `json:"t"`
	Members  []Member `json:"members"`
	Host     string   `json:"host"`
	HostOnly bool     `json:"hostOnly"`
	// Ids of members holding the room up. Ids rather than names so the
	// frontend can render them in its own way, and so a rename cannot desync
	// the two lists.
	Waiting []string `json:"waiting"`
}

type Pong struct {
	T string `json:"t"`
	// The client's own reading, echoed untouched: the round trip is measured
	// against it without the relay having to remember anything.
	C int64 `json:"c"`
	S int64 `json:"s"`
}

type ErrorMsg struct {
	T    string `json:"t"`
	Code string `json:"code"`
	// Present for a human reading a log. The frontend translates `Code` and
	// never shows this.
	Message string `json:"message"`
}

func NewTimelineMsg(t Timeline) TimelineMsg {
	return TimelineMsg{T: "timeline", Timeline: t}
}
