// A relay for watching together: rooms, a shared timeline, and nothing else.
//
// No media passes through here and none ever will — the players fetch the film
// themselves, from a disk, a website or a swarm — so what this holds is a few
// hundred bytes per room and a socket per viewer. There is no database, nothing
// is written to disk, and a room ceases to exist a few minutes after the last
// person leaves. That is a deliberate property rather than an unfinished one:
// the least this can know is the most it should.
//
//	go run ./server                       # :8080
//	RELAY_ADDR=:9000 go run ./server
package main

import (
	"context"
	"errors"
	"fmt"
	"log"
	"net/http"
	"os"
	"os/signal"
	"strconv"
	"strings"
	"syscall"
	"time"
)

type Config struct {
	Addr string
	// What `/j/<code>` prints as the address to share. Only ever cosmetic — the
	// player is told where to connect by its own setting.
	PublicURL string
	// Empty means "any", which is correct here: see the note in serveWS.
	OriginPatterns []string
	TrustProxy     bool

	// Where the invitation page sends somebody who does not have the player.
	//
	// Per platform where the operator has a direct installer — an invitation is
	// not the moment to hand somebody a release list — and a page as the
	// fallback, which is what ships by default. The default matters more than it
	// looks: without one the page rendered *no* download link at all, so the one
	// visitor the page exists for, the one who does not have the player, was
	// shown a code and nothing to do with it.
	//
	// Set `RELAY_DOWNLOAD_PAGE=` (empty) to suppress it deliberately.
	DownloadWin  string
	DownloadMac  string
	DownloadPage string

	MaxRooms   int
	MaxMembers int
	RoomTTL    time.Duration

	PingInterval     time.Duration
	WriteTimeout     time.Duration
	HandshakeTimeout time.Duration
	SweepInterval    time.Duration

	JoinPerSecond float64
	JoinBurst     float64
	MsgPerSecond  float64
	MsgBurst      float64
}

func defaultConfig() Config {
	return Config{
		Addr:             env("RELAY_ADDR", ":8080"),
		PublicURL:        strings.TrimRight(env("RELAY_PUBLIC_URL", ""), "/"),
		OriginPatterns:   splitList(env("RELAY_ORIGINS", "*")),
		TrustProxy:       env("RELAY_TRUST_PROXY", "") != "",
		DownloadWin:      env("RELAY_DOWNLOAD_WIN", ""),
		DownloadMac:      env("RELAY_DOWNLOAD_MAC", ""),
		DownloadPage:     env("RELAY_DOWNLOAD_PAGE", defaultDownloadPage),
		MaxRooms:         envInt("RELAY_MAX_ROOMS", 5000),
		MaxMembers:       envInt("RELAY_MAX_MEMBERS", 16),
		RoomTTL:          envDur("RELAY_ROOM_TTL", 5*time.Minute),
		PingInterval:     envDur("RELAY_PING", 20*time.Second),
		WriteTimeout:     envDur("RELAY_WRITE_TIMEOUT", 10*time.Second),
		HandshakeTimeout: envDur("RELAY_HANDSHAKE_TIMEOUT", 10*time.Second),
		SweepInterval:    envDur("RELAY_SWEEP", 15*time.Second),
		// Ten joins to start with and one every six seconds after: ample for a
		// person mistyping a code, useless for walking the code space.
		JoinPerSecond: 1.0 / 6.0,
		JoinBurst:     10,
		// A timeline change is a human gesture and a ping is one every thirty
		// seconds, so this is two orders of magnitude of headroom.
		MsgPerSecond: 20,
		MsgBurst:     40,
	}
}

// Where a visitor without the player is sent when the operator has not said
// otherwise.
//
// A page rather than an installer, because which installer is right depends on
// the platform and a wrong one is worse than a page that offers both. The
// releases page rather than a site: it is the same address the README hands
// people, it carries both platforms, and — the point — it exists. A default
// pointing at a page that has not been built yet would be a dead link on the
// one screen shown to somebody who does not have the player, which is worse
// than the release list it replaces.
const defaultDownloadPage = "https://github.com/risenxxx/frame-player/releases/latest"

type server struct {
	cfg Config
	hub *hub
}

func main() {
	cfg := defaultConfig()
	s := &server{cfg: cfg, hub: newHub(cfg)}

	mux := http.NewServeMux()
	mux.HandleFunc("GET /ws", s.serveWS)
	mux.HandleFunc("GET /j/{code}", s.serveJoinPage)
	mux.HandleFunc("GET /favicon.svg", s.serveFavicon)
	mux.HandleFunc("GET /healthz", func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "text/plain; charset=utf-8")
		_, _ = w.Write([]byte("ok\n"))
	})
	mux.HandleFunc("GET /metrics", s.serveMetrics)
	mux.HandleFunc("GET /{$}", s.serveIndex)

	httpSrv := &http.Server{
		Addr:    cfg.Addr,
		Handler: mux,
		// A websocket upgrade must not be cut off by a read header timeout, and
		// the connection's own deadlines are managed per message, so only the
		// header phase is bounded here.
		ReadHeaderTimeout: 10 * time.Second,
	}

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	go func() {
		t := time.NewTicker(cfg.SweepInterval)
		defer t.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case now := <-t.C:
				s.hub.sweep(now)
			}
		}
	}()

	go func() {
		<-ctx.Done()
		// Rooms are in memory and worth nothing once the process ends, so this
		// is about letting close frames go out rather than about saving state.
		shutdown, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = httpSrv.Shutdown(shutdown)
	}()

	log.Printf("relay listening on %s (max %d rooms, %d per room)", cfg.Addr, cfg.MaxRooms, cfg.MaxMembers)
	if err := httpSrv.ListenAndServe(); err != nil && !errors.Is(err, http.ErrServerClosed) {
		log.Fatalf("relay: %v", err)
	}
}

func (s *server) serveMetrics(w http.ResponseWriter, _ *http.Request) {
	made, joins, dropped := s.hub.stats()
	w.Header().Set("Content-Type", "text/plain; charset=utf-8")
	fmt.Fprintf(w, "relay_rooms %d\n", s.hub.count())
	fmt.Fprintf(w, "relay_rooms_created_total %d\n", made)
	fmt.Fprintf(w, "relay_joins_total %d\n", joins)
	fmt.Fprintf(w, "relay_clients_dropped_total %d\n", dropped)
}

func env(key, def string) string {
	if v, ok := os.LookupEnv(key); ok && v != "" {
		return v
	}
	return def
}

func envInt(key string, def int) int {
	if v, err := strconv.Atoi(env(key, "")); err == nil && v > 0 {
		return v
	}
	return def
}

func envDur(key string, def time.Duration) time.Duration {
	if v, err := time.ParseDuration(env(key, "")); err == nil && v > 0 {
		return v
	}
	return def
}

func splitList(s string) []string {
	out := []string{}
	for _, p := range strings.Split(s, ",") {
		if p = strings.TrimSpace(p); p != "" {
			out = append(out, p)
		}
	}
	return out
}
