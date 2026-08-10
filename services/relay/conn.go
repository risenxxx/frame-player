// One connection: its outbox, its liveness, and the loop that turns bytes into
// room calls.
//
// The shape that matters here is the outbox. Every send from a room is
// non-blocking into a buffered channel, so a member on a bad connection costs
// themselves their session and never costs the room a millisecond — which is
// what lets the room hold a plain mutex (see the header of room.go). A full
// outbox is not a reason to wait; it is the definition of a client that has
// stopped keeping up.
package main

import (
	"context"
	"encoding/json"
	"net"
	"net/http"
	"strings"
	"sync"
	"time"

	"github.com/coder/websocket"

	"frameplayer/relay/internal/wire"
)

// Enough for a burst of members+timeline while a season is being switched, and
// far short of anything worth buffering for a client that has gone away.
const outboxDepth = 32

type client struct {
	id  string
	hub *hub

	mu      sync.Mutex
	nameStr string

	out chan []byte
	// Cancelling this is how anything — a full outbox, a protocol error, the
	// server shutting down — ends the connection from any goroutine.
	cancel context.CancelFunc
	once   sync.Once
}

func (c *client) name() string {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.nameStr
}

func (c *client) setName(s string) {
	c.mu.Lock()
	c.nameStr = s
	c.mu.Unlock()
}

// send never blocks. Called with a room's lock held.
func (c *client) send(raw []byte) {
	select {
	case c.out <- raw:
	default:
		c.hub.noteDropped()
		c.kill()
	}
}

func (c *client) kill() {
	c.once.Do(func() {
		if c.cancel != nil {
			c.cancel()
		}
	})
}

func (c *client) sendMsg(v any) {
	raw, err := json.Marshal(v)
	if err != nil {
		return
	}
	c.send(raw)
}

func (c *client) sendError(code string, err error) {
	msg := ""
	if err != nil {
		msg = err.Error()
	}
	c.sendMsg(wire.ErrorMsg{T: "error", Code: code, Message: msg})
}

// ---- the HTTP entry point ---------------------------------------------------

func (s *server) serveWS(w http.ResponseWriter, r *http.Request) {
	conn, err := websocket.Accept(w, r, &websocket.AcceptOptions{
		// The room code is the only secret here and there are no cookies, no
		// credentials and no ambient authority of any kind — so an Origin check
		// defends against nothing, while a wrong one would refuse the desktop
		// app, whose origin is `tauri://localhost` on macOS and
		// `http://tauri.localhost` on Windows. Configurable for an operator who
		// wants the relay reachable only from their own page.
		OriginPatterns:  s.cfg.OriginPatterns,
		CompressionMode: websocket.CompressionDisabled,
	})
	if err != nil {
		return
	}
	// Refuse anything larger than a frame can legitimately be, at the library
	// level: a client that sends more is closed rather than read into memory.
	conn.SetReadLimit(wire.MaxFrameBytes)

	ctx, cancel := context.WithCancel(r.Context())
	defer cancel()

	c := &client{id: wire.NewID(), hub: s.hub, out: make(chan []byte, outboxDepth), cancel: cancel}
	defer c.kill()

	room, err := s.handshake(ctx, conn, c, s.addrOf(r))
	if err != nil {
		// The client is told why before the socket goes: "the room does not
		// exist" and "the relay is full" are different problems and only one of
		// them is worth retyping the code for.
		c.sendError(err.Error(), nil)
		drain(ctx, conn, c)
		_ = conn.Close(websocket.StatusPolicyViolation, err.Error())
		return
	}
	defer func() {
		room.leave(c.id, time.Now())
	}()

	go c.writePump(ctx, conn)
	c.readLoop(ctx, conn, room)
	_ = conn.Close(websocket.StatusNormalClosure, "")
}

// handshake reads the one message that has to come first.
//
// Deliberately not part of the read loop: until a `hello` has arrived there is
// no member, no room and nothing to broadcast, and a connection that opens and
// says nothing must cost a few seconds rather than a slot.
func (s *server) handshake(ctx context.Context, conn *websocket.Conn, c *client, addr string) (*room, error) {
	hctx, cancel := context.WithTimeout(ctx, s.cfg.HandshakeTimeout)
	defer cancel()

	_, raw, err := conn.Read(hctx)
	if err != nil {
		return nil, wire.ErrBadMessage
	}
	var msg wire.ClientMsg
	if err := json.Unmarshal(raw, &msg); err != nil {
		return nil, wire.ErrBadMessage
	}
	if msg.T != "hello" {
		return nil, wire.ErrBadMessage
	}
	if err := msg.Validate(); err != nil {
		return nil, err
	}
	if !s.hub.allowJoin(addr, time.Now()) {
		return nil, errRateLimited
	}
	c.setName(msg.Name)

	now := time.Now()
	var room *room
	if msg.Room == "" {
		room, err = s.hub.create(now)
	} else {
		room, err = s.hub.get(msg.Room)
	}
	if err != nil {
		return nil, err
	}
	welcome, err := room.join(c, s.cfg.MaxMembers, now)
	if err != nil {
		return nil, err
	}
	s.hub.noteJoin()
	// Straight onto the socket rather than through the outbox: `welcome` has to
	// be the first thing the client sees, and the outbox already holds the
	// members broadcast that `join` produced.
	if err := writeJSON(ctx, conn, s.cfg.WriteTimeout, welcome); err != nil {
		room.leave(c.id, time.Now())
		return nil, wire.ErrBadMessage
	}
	return room, nil
}

func (c *client) readLoop(ctx context.Context, conn *websocket.Conn, room *room) {
	// A client with nothing to say is normal — a paused film says nothing for
	// an hour — so liveness is the ping's job, not a read deadline's.
	limiter := &bucket{tokens: c.hub.cfg.MsgBurst, last: time.Now()}
	for {
		_, raw, err := conn.Read(ctx)
		if err != nil {
			return
		}
		now := time.Now()
		if !allow(limiter, now, c.hub.cfg.MsgPerSecond, c.hub.cfg.MsgBurst) {
			c.sendError("rate_limited", nil)
			return
		}
		var msg wire.ClientMsg
		if err := json.Unmarshal(raw, &msg); err != nil {
			c.sendError("bad_message", nil)
			continue
		}
		if err := msg.Validate(); err != nil {
			c.sendError(err.Error(), nil)
			continue
		}
		switch msg.T {
		case "timeline":
			if err := room.setTimeline(c.id, *msg.Timeline, now); err != nil {
				c.sendError(err.Error(), nil)
			}
		case "ready":
			room.setReady(c.id, *msg.Ready, now)
		case "mode":
			if err := room.setMode(c.id, &msg, now); err != nil {
				c.sendError(err.Error(), nil)
			}
		case "ping":
			// Straight back, with the client's own reading untouched: the round
			// trip is measured against it, so the relay has to remember nothing
			// and a lost pong costs one sample rather than a wrong offset.
			c.sendMsg(wire.Pong{T: "pong", C: msg.C, S: now.UnixMilli()})
		case "hello":
			c.sendError("bad_message", nil)
		case "bye":
			return
		}
	}
}

func (c *client) writePump(ctx context.Context, conn *websocket.Conn) {
	ping := time.NewTicker(c.hub.cfg.PingInterval)
	defer ping.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case raw := <-c.out:
			if err := write(ctx, conn, c.hub.cfg.WriteTimeout, raw); err != nil {
				c.kill()
				return
			}
		case <-ping.C:
			// This is the liveness check: `Ping` waits for the pong, so a peer
			// that has gone away without closing the socket is discovered here
			// rather than by a read that never comes.
			pctx, cancel := context.WithTimeout(ctx, c.hub.cfg.WriteTimeout)
			err := conn.Ping(pctx)
			cancel()
			if err != nil {
				c.kill()
				return
			}
		}
	}
}

// Give a refused client a moment to read the error before the socket closes;
// without it the close frame routinely wins the race and the viewer is told
// "connection failed" when the relay had said exactly what was wrong.
func drain(ctx context.Context, conn *websocket.Conn, c *client) {
	select {
	case raw := <-c.out:
		_ = write(ctx, conn, time.Second, raw)
	default:
	}
}

func write(ctx context.Context, conn *websocket.Conn, timeout time.Duration, raw []byte) error {
	wctx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()
	return conn.Write(wctx, websocket.MessageText, raw)
}

func writeJSON(ctx context.Context, conn *websocket.Conn, timeout time.Duration, v any) error {
	raw, err := json.Marshal(v)
	if err != nil {
		return err
	}
	return write(ctx, conn, timeout, raw)
}

func allow(b *bucket, now time.Time, rate, burst float64) bool {
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

// The address a rate limit is keyed by.
//
// `X-Forwarded-For` is honoured only when the operator says the relay is behind
// a proxy, because a header anyone can set is otherwise a way to be somebody
// else — which for a rate limit means a way to have no limit at all.
func (s *server) addrOf(r *http.Request) string {
	if s.cfg.TrustProxy {
		if fwd := r.Header.Get("X-Forwarded-For"); fwd != "" {
			if i := strings.Index(fwd, ","); i >= 0 {
				fwd = fwd[:i]
			}
			return strings.TrimSpace(fwd)
		}
	}
	host, _, err := net.SplitHostPort(r.RemoteAddr)
	if err != nil {
		return r.RemoteAddr
	}
	return host
}
