// The two pages the relay serves to a browser.
//
// `/j/<code>` is what a viewer's friend actually clicks. It exists because a
// bare `frameplayer://join/ABC123` cannot be sent through most chat
// applications — they linkify `https://` and leave an unknown scheme as text —
// and because somebody without the player has to land somewhere that says what
// this is rather than on a browser error.
//
// Deliberately not a redirect. A page that fires the custom scheme on load
// leaves a browser tab showing "cannot be opened" for anyone who does not have
// the player, and browsers increasingly refuse an automatic navigation to an
// unknown scheme anyway. A button that says what it will do is both honest and
// what actually works.
//
// Self-contained: no fonts, no scripts, no images, nothing fetched. The relay
// holds no state about a person and a page that phoned somewhere would be the
// first thing to break that.
package main

import (
	"html/template"
	"net/http"
	"strings"

	"frameplayer/relay/internal/wire"
)

// Bilingual on one page rather than negotiated by `Accept-Language`: the two
// languages the player ships in, both visible, so a link forwarded on to a
// third person reads for them too.
var joinPage = template.Must(template.New("join").Parse(`<!doctype html>
<html lang="en"><head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex">
<title>{{.Code}} — Frame Player</title>
<style>
  :root { color-scheme: dark }
  * { box-sizing: border-box }
  body { margin: 0; min-height: 100vh; display: grid; place-items: center;
         background: #101016; color: #e8e8ef; padding: 24px;
         font: 15px/1.5 -apple-system, "Segoe UI", system-ui, sans-serif }
  main { width: 100%; max-width: 380px; text-align: center }
  h1 { font-size: 17px; font-weight: 600; margin: 0 0 4px }
  p { margin: 0; color: #a0a0b0 }
  .code { font: 600 34px/1 ui-monospace, "SF Mono", Menlo, Consolas, monospace;
          letter-spacing: .18em; margin: 22px 0 6px; padding-left: .18em }
  .go { display: block; margin: 22px 0 10px; padding: 11px 18px; border: 0; border-radius: 10px;
        background: #6366f1; color: #fff; font: inherit; font-weight: 600;
        text-decoration: none; cursor: pointer }
  .go:hover { background: #818cf8 }
  .sub { font-size: 13px; margin-top: 14px }
  a.plain { color: #a0a0b0 }
</style>
</head><body><main>
  <h1>Watch together · Совместный просмотр</h1>
  <p>Room code · Код комнаты</p>
  <div class="code">{{.Code}}</div>
  <a class="go" href="frameplayer://join/{{.Code}}">Open in Frame Player</a>
  <p class="sub">Или откройте плеер, нажмите «Смотреть вместе» и введите код.<br>
     Don’t have it? Install Frame Player, then enter the code above.</p>
</main></body></html>
`))

func (s *server) serveJoinPage(w http.ResponseWriter, r *http.Request) {
	code := wire.NormalizeCode(r.PathValue("code"))
	if code == "" {
		http.Error(w, "not a room code", http.StatusNotFound)
		return
	}
	// Whether the room *exists* is deliberately not checked or reported. It
	// would turn this page into an oracle for walking the code space, and it
	// would be wrong as often as it was right: a link is normally sent before
	// anyone has opened the player.
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	w.Header().Set("Cache-Control", "no-store")
	w.Header().Set("Referrer-Policy", "no-referrer")
	_ = joinPage.Execute(w, struct{ Code string }{code})
}

func (s *server) serveIndex(w http.ResponseWriter, _ *http.Request) {
	w.Header().Set("Content-Type", "text/plain; charset=utf-8")
	var b strings.Builder
	b.WriteString("Frame Player watch-together relay.\n\n")
	b.WriteString("Rooms are held in memory, nothing is written to disk, and no media\n")
	b.WriteString("passes through here — only the shared timeline.\n\n")
	if s.cfg.PublicURL != "" {
		b.WriteString("Point the player at: " + s.cfg.PublicURL + "\n")
	}
	_, _ = w.Write([]byte(b.String()))
}
