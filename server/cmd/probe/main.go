// A headless peer, so that watching together can be tested by one person.
//
// The feature needs at least two players, and two players normally means two
// machines: the app is single-instance, so a second `npm run tauri dev` signals
// the first rather than starting one. That makes the ordinary development loop —
// change something, see whether it still syncs — cost a second computer, which
// in practice means it does not get run.
//
// This is the other end of the room. It joins, reports itself ready, follows the
// timeline and prints what it would have done, so the real player can be driven
// by hand and watched from here. It can also drive, and it can lie about its own
// clock, which is the only way to see drift correction working as something
// other than a coincidence.
//
//	go run ./server &
//	go run ./server/cmd/probe                      # creates a room, prints the code
//	go run ./server/cmd/probe -room ABC123          # joins the player's room
//	go run ./server/cmd/probe -room ABC123 -skew 300ms
//	go run ./server/cmd/probe -room ABC123 -hold 20s   # freeze the room on purpose
//
// With `-drive`, stdin takes commands: `p` toggles pause, `<number>` seeks,
// `r`/`n` report ready/not ready, `q` quits.
package main

import (
	"bufio"
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"log"
	"math"
	"os"
	"os/signal"
	"sort"
	"strconv"
	"strings"
	"sync"
	"syscall"
	"time"

	"github.com/coder/websocket"

	"frameplayer/relay/internal/wire"
)

func main() {
	relay := flag.String("relay", "ws://127.0.0.1:8080", "relay websocket address")
	room := flag.String("room", "", "room code; empty creates one")
	name := flag.String("name", "probe", "name shown to the other viewers")
	skew := flag.Duration("skew", 0, "pretend this machine's clock is off by this much")
	hold := flag.Duration("hold", 0, "stay not-ready for this long after joining")
	drive := flag.Bool("drive", false, "read commands from stdin")
	play := flag.Bool("play", false, "put a made-up film on the timeline, so two probes can exercise a room with no player")
	flag.Parse()

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	conn, _, err := websocket.Dial(ctx, strings.TrimRight(*relay, "/")+"/ws", nil)
	if err != nil {
		log.Fatalf("dial %s: %v", *relay, err)
	}
	defer conn.CloseNow()
	conn.SetReadLimit(wire.MaxFrameBytes)

	p := &probe{conn: conn, ctx: ctx, skew: *skew}
	p.send(map[string]any{"t": "hello", "ver": wire.ProtocolVersion, "room": *room, "name": *name})

	if *hold > 0 {
		// A member who has not reported is a member the room waits for. Holding
		// on purpose is how the freeze and its overlay get exercised.
		go func() {
			log.Printf("holding the room for %s", *hold)
			select {
			case <-ctx.Done():
			case <-time.After(*hold):
				p.send(map[string]any{"t": "ready", "ready": true})
				log.Printf("ready")
			}
		}()
	} else {
		p.send(map[string]any{"t": "ready", "ready": true})
	}

	go p.pingLoop()
	go p.reportLoop()
	if *play {
		// After the handshake, so `welcome` has set the clock offset — a
		// timeline stamped before that would be a second off for no reason.
		go func() {
			time.Sleep(750 * time.Millisecond)
			p.openFakeFilm()
		}()
	}
	if *drive {
		go p.driveLoop()
	}
	p.readLoop()
}

type probe struct {
	conn *websocket.Conn
	ctx  context.Context
	skew time.Duration

	mu       sync.Mutex
	writeMu  sync.Mutex
	me       string
	offset   float64 // relay clock minus ours, milliseconds
	samples  []sample
	tl       wire.Timeline
	haveTL   bool
	members  []wire.Member
	waiting  []string
	hostOnly bool
	host     string
}

type sample struct {
	rtt    float64
	offset float64
}

// Our idea of the relay's clock. `skew` is added on purpose, so the drift the
// real player has to correct can be dialled in rather than waited for.
func (p *probe) relayNow() int64 {
	return time.Now().Add(p.skew).UnixMilli() + int64(p.offset)
}

func (p *probe) send(v any) {
	raw, err := json.Marshal(v)
	if err != nil {
		return
	}
	// One writer at a time: pings, drive commands and readiness all come from
	// different goroutines, and a websocket connection is not safe for
	// concurrent writes.
	p.writeMu.Lock()
	defer p.writeMu.Unlock()
	ctx, cancel := context.WithTimeout(p.ctx, 5*time.Second)
	defer cancel()
	if err := p.conn.Write(ctx, websocket.MessageText, raw); err != nil && p.ctx.Err() == nil {
		log.Printf("write: %v", err)
	}
}

func (p *probe) readLoop() {
	for {
		_, raw, err := p.conn.Read(p.ctx)
		if err != nil {
			if p.ctx.Err() == nil {
				log.Printf("connection closed: %v", err)
			}
			return
		}
		var head struct {
			T string `json:"t"`
		}
		if err := json.Unmarshal(raw, &head); err != nil {
			continue
		}
		switch head.T {
		case "welcome":
			var w wire.Welcome
			if err := json.Unmarshal(raw, &w); err != nil {
				continue
			}
			p.mu.Lock()
			p.me, p.host, p.hostOnly = w.Me, w.Host, w.HostOnly
			p.members, p.waiting = w.Members, w.Waiting
			p.tl, p.haveTL = w.Timeline, w.Timeline.Rev > 0
			// A usable offset before the first ping completes.
			p.offset = float64(w.Now - time.Now().Add(p.skew).UnixMilli())
			p.mu.Unlock()
			fmt.Printf("\n  room %s   you are %s   host %s\n\n", w.Room, w.Me, w.Host)

		case "timeline":
			var m wire.TimelineMsg
			if err := json.Unmarshal(raw, &m); err != nil {
				continue
			}
			p.mu.Lock()
			p.tl, p.haveTL = m.Timeline, true
			p.mu.Unlock()
			who := m.By
			if who == "" {
				who = "relay"
			}
			log.Printf("timeline rev %d by %-11s %s %7.2fs x%.2f  %s%s",
				m.Rev, who, pausedWord(m.Paused), m.Position, m.Speed,
				describeContent(m.Content), describeTracks(m.Tracks))

		case "members":
			var m wire.MembersMsg
			if err := json.Unmarshal(raw, &m); err != nil {
				continue
			}
			p.mu.Lock()
			p.members, p.waiting, p.host, p.hostOnly = m.Members, m.Waiting, m.Host, m.HostOnly
			p.mu.Unlock()
			names := make([]string, 0, len(m.Members))
			for _, mem := range m.Members {
				mark := ""
				if !mem.Ready {
					mark = " (loading)"
				}
				if mem.ID == m.Host {
					mark += " (host)"
				}
				names = append(names, mem.Name+mark)
			}
			mode := ""
			if m.HostOnly {
				mode = "  [host only]"
			}
			log.Printf("members: %s%s", strings.Join(names, ", "), mode)

		case "pong":
			var m wire.Pong
			if err := json.Unmarshal(raw, &m); err != nil {
				continue
			}
			p.noteSample(m)

		case "error":
			var m wire.ErrorMsg
			if err := json.Unmarshal(raw, &m); err != nil {
				continue
			}
			log.Printf("refused: %s %s", m.Code, m.Message)
		}
	}
}

// The same estimator the player runs, so a disagreement between the two shows
// up here rather than as unexplained drift.
func (p *probe) noteSample(m wire.Pong) {
	now := time.Now().Add(p.skew).UnixMilli()
	rtt := float64(now - m.C)
	if rtt < 0 {
		return
	}
	offset := float64(m.S) + rtt/2 - float64(now)

	p.mu.Lock()
	defer p.mu.Unlock()
	p.samples = append(p.samples, sample{rtt: rtt, offset: offset})
	if len(p.samples) > 8 {
		p.samples = p.samples[1:]
	}
	// The median of the fastest half: a slow round trip is asymmetric far more
	// often than a fast one, so averaging everything imports exactly the noise
	// worth discarding.
	byRTT := append([]sample(nil), p.samples...)
	sort.Slice(byRTT, func(i, j int) bool { return byRTT[i].rtt < byRTT[j].rtt })
	best := byRTT[:max(1, len(byRTT)/2)]
	offsets := make([]float64, len(best))
	for i, s := range best {
		offsets[i] = s.offset
	}
	sort.Float64s(offsets)
	p.offset = offsets[len(offsets)/2]
}

func (p *probe) pingLoop() {
	// Fast at first so the offset settles in a couple of seconds, then rarely.
	for i := 0; ; i++ {
		p.send(map[string]any{"t": "ping", "c": time.Now().Add(p.skew).UnixMilli()})
		wait := 500 * time.Millisecond
		if i > 7 {
			wait = 30 * time.Second
		}
		select {
		case <-p.ctx.Done():
			return
		case <-time.After(wait):
		}
	}
}

// What this peer would be showing, once a second — the line that makes a real
// player's behaviour legible from a terminal.
func (p *probe) reportLoop() {
	t := time.NewTicker(time.Second)
	defer t.Stop()
	for {
		select {
		case <-p.ctx.Done():
			return
		case <-t.C:
			p.mu.Lock()
			tl, have, waiting, offset := p.tl, p.haveTL, len(p.waiting), p.offset
			p.mu.Unlock()
			if !have {
				continue
			}
			state := "playing"
			if tl.Paused {
				state = "paused "
			}
			if waiting > 0 {
				state = "waiting"
			}
			fmt.Printf("\r  %s at %s   clock offset %+.0f ms      ",
				state, hms(tl.PositionAt(p.relayNow())), offset)
		}
	}
}

// Something to put on the timeline when there is no player in the room.
//
// The relay never parses content, so this only has to be shaped like a
// `ContentRef` — and a `url` one is the honest choice: a real player joining
// this room would try to open it, fail, and say so, which is better than a
// made-up torrent that would send it looking for a swarm.
func (p *probe) openFakeFilm() {
	p.send(map[string]any{"t": "timeline", "timeline": map[string]any{
		"content": map[string]any{
			"kind":     "url",
			"url":      "https://example.invalid/probe.mp4",
			"title":    "Probe test film",
			"duration": 5400,
		},
		"paused":   false,
		"position": 0,
		"speed":    1,
	}})
}

func (p *probe) driveLoop() {
	fmt.Println("  commands: p = pause/play, <seconds> = seek, o = open a test film, r/n = ready, q = quit")
	sc := bufio.NewScanner(os.Stdin)
	for sc.Scan() {
		line := strings.TrimSpace(sc.Text())
		p.mu.Lock()
		tl, have := p.tl, p.haveTL
		p.mu.Unlock()
		switch {
		case line == "q":
			p.send(map[string]any{"t": "bye"})
			return
		case line == "o":
			p.openFakeFilm()
		case line == "r":
			p.send(map[string]any{"t": "ready", "ready": true})
		case line == "n":
			p.send(map[string]any{"t": "ready", "ready": false})
		case line == "p":
			if !have {
				fmt.Println("  nothing is playing yet")
				continue
			}
			next := tl
			next.Position = tl.PositionAt(p.relayNow())
			next.Paused = !tl.Paused
			p.send(map[string]any{"t": "timeline", "timeline": next})
		default:
			secs, err := strconv.ParseFloat(line, 64)
			if err != nil || !have {
				continue
			}
			next := tl
			next.Position = math.Max(0, secs)
			p.send(map[string]any{"t": "timeline", "timeline": next})
		}
	}
}

func pausedWord(p bool) string {
	if p {
		return "paused "
	}
	return "playing"
}

// Enough of a ContentRef to see a handoff working — deliberately tolerant,
// since the relay itself never parses this and the probe must not be the one
// place that insists on a shape.
func describeContent(raw json.RawMessage) string {
	if len(raw) == 0 || string(raw) == "null" {
		return "(nothing)"
	}
	var c struct {
		Kind  string `json:"kind"`
		Title string `json:"title"`
		Index *int   `json:"index"`
		Hash  string `json:"infoHash"`
		URL   string `json:"url"`
	}
	if err := json.Unmarshal(raw, &c); err != nil {
		return string(raw)
	}
	switch c.Kind {
	case "torrent":
		idx := ""
		if c.Index != nil {
			idx = fmt.Sprintf(" #%d", *c.Index)
		}
		return fmt.Sprintf("torrent %s%s %q", short(c.Hash), idx, c.Title)
	case "hidden":
		return "(hidden — a private folder, so the room is not told what it is)"
	case "url":
		return "url " + c.URL
	case "file":
		return fmt.Sprintf("file %q", c.Title)
	}
	return string(raw)
}

// The room's audio choice, which travels as a description rather than an id —
// so this prints what a real player would try to match against its own copy.
func describeTracks(raw json.RawMessage) string {
	if len(raw) == 0 || string(raw) == "null" {
		return ""
	}
	var t struct {
		Audio *struct {
			Lang  *string `json:"lang"`
			Title *string `json:"title"`
		} `json:"audio"`
	}
	if err := json.Unmarshal(raw, &t); err != nil || t.Audio == nil {
		return ""
	}
	parts := []string{}
	if t.Audio.Lang != nil {
		parts = append(parts, *t.Audio.Lang)
	}
	if t.Audio.Title != nil {
		parts = append(parts, *t.Audio.Title)
	}
	return "  audio[" + strings.Join(parts, " ") + "]"
}

func short(s string) string {
	if len(s) > 8 {
		return s[:8]
	}
	return s
}

func hms(sec float64) string {
	if sec < 0 {
		sec = 0
	}
	d := time.Duration(sec * float64(time.Second))
	return fmt.Sprintf("%d:%02d:%02d", int(d.Hours()), int(d.Minutes())%60, int(d.Seconds())%60)
}
