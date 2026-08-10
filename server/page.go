// The two pages the relay serves to a browser, and the fonts they are set in.
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
// The typeface comes from Google Fonts. The player embeds its own copy and this
// page could have too — 50 KB of woff2 in the binary — and that was the first
// version; it is deliberately not what shipped. The privacy argument that shapes
// the *relay* (hold nothing, learn nothing, forget in five minutes) is about the
// room and what is being watched in it. This is a public invitation page with a
// code on it, opened once, and treating a webfont request as the same class of
// exposure was privacy theatre with a maintenance cost attached: a second copy
// of a font to keep in step with the player's.
//
// There is still **no script and nothing else external** — no analytics, no
// tracking pixel, no framework. The page is HTML, inline CSS and one stylesheet
// link.
package main

import (
	"html/template"
	"net/http"
	"strings"

	"frameplayer/relay/internal/wire"
)

// ---- what the page says -----------------------------------------------------

// A page in one language. Two of them rather than a lookup per string: this is
// a dozen phrases on one page, and a table of keys would be more machinery than
// the thing it holds.
type pageText struct {
	Lang     string
	Title    string
	Sub      string
	CodeCap  string
	Open     string
	Manual   string
	NoPlayer string
	Get      string
	Privacy  string
}

var textRU = pageText{
	Lang:     "ru",
	Title:    "Смотрим вместе",
	Sub:      "Один фильм, одна позиция, одни паузы.",
	CodeCap:  "Код комнаты",
	Open:     "Открыть в Frame Player",
	Manual:   "Или откройте плеер, нажмите «Смотреть вместе» и введите код.",
	NoPlayer: "Нет плеера?",
	Get:      "Скачать Frame Player",
	Privacy:  "Через сервер идут только позиция и пауза. Видео не передаётся.",
}

var textEN = pageText{
	Lang:     "en",
	Title:    "Watch together",
	Sub:      "One film, one position, the same pauses.",
	CodeCap:  "Room code",
	Open:     "Open in Frame Player",
	Manual:   "Or open the player, choose “Watch together” and type the code.",
	NoPlayer: "Don’t have it?",
	Get:      "Get Frame Player",
	Privacy:  "Only the position and pause travel through the server. No video does.",
}

// The language to answer in, from `Accept-Language`.
//
// Deliberately crude: this page has two languages and one decision, so a full
// quality-value negotiation would be a parser for a choice with two outcomes.
// The first tag that names either wins, which is what a browser's ordering
// already means — and anything else gets English, because a link is forwarded
// far more often than it is generated.
func pageTextFor(header string) pageText {
	for _, part := range strings.Split(header, ",") {
		tag, _, _ := strings.Cut(strings.TrimSpace(part), ";")
		tag = strings.ToLower(strings.TrimSpace(tag))
		switch {
		case tag == "ru" || strings.HasPrefix(tag, "ru-"):
			return textRU
		case tag == "en" || strings.HasPrefix(tag, "en-"):
			return textEN
		}
	}
	return textEN
}

// Which installer to offer, from the User-Agent.
//
// A download link rather than a page, when the operator has configured one:
// somebody who has just been sent an invitation wants the player, not a release
// list. It does not take the tab with it — a browser navigating to something it
// cannot render downloads it and stays where it was — so the code stays on
// screen behind the download, which is the whole point of putting it there.
func (s *server) downloadFor(ua string) string {
	ua = strings.ToLower(ua)
	switch {
	case strings.Contains(ua, "windows") && s.cfg.DownloadWin != "":
		return s.cfg.DownloadWin
	case (strings.Contains(ua, "mac os") || strings.Contains(ua, "macos")) && s.cfg.DownloadMac != "":
		return s.cfg.DownloadMac
	}
	return s.cfg.DownloadPage
}

type joinPageData struct {
	pageText
	Code     string
	Download string
}

var joinPage = template.Must(template.New("join").Parse(`<!doctype html>
<html lang="{{.Lang}}"><head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex">
<meta name="color-scheme" content="dark">
<title>{{.Code}} · Frame Player</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Rubik:wght@300..900&display=swap">
<style>
  * { box-sizing: border-box }
  body {
    margin: 0; min-height: 100vh; display: grid; place-items: center;
    background: #101016; color: #e8e8ec; padding: 24px;
    font: 400 15px/1.55 'Rubik', -apple-system, 'Segoe UI', system-ui, sans-serif;
    -webkit-font-smoothing: antialiased;
  }
  main { width: 100%; max-width: 360px; text-align: center }
  h1 { font-size: 19px; font-weight: 500; margin: 0 0 4px; letter-spacing: .01em }
  p { margin: 0; color: #9a9aa6; font-size: 13.5px }
  /* The six characters are the thing on this page. Monospaced and tracked out
     for the same reason the player sets them that way: a code is read aloud and
     typed back, and that is where 0/O and 1/l go wrong. */
  .code {
    font: 600 34px/1 ui-monospace, 'SF Mono', Menlo, Consolas, monospace;
    letter-spacing: .18em; padding-left: .18em;
    margin: 26px 0 8px; color: #e8e8ec;
  }
  .cap { font-size: 12px; color: #7a7a88; text-transform: uppercase; letter-spacing: .08em }
  .go {
    display: block; margin: 24px 0 0; padding: 12px 18px; border: 0; border-radius: 10px;
    background: #6366f1; color: #fff; font: inherit; font-weight: 500;
    text-decoration: none; transition: background .15s ease;
  }
  .go:hover { background: #818cf8 }
  .sub { margin-top: 14px; font-size: 12.5px }
  .rule { height: 1px; margin: 22px 0 14px; background: rgba(255,255,255,.1) }
  .get { color: #b9b9c3; text-decoration: none; border-bottom: 1px solid rgba(255,255,255,.2) }
  .get:hover { color: #e8e8ec }
  .foot { margin-top: 18px; font-size: 11.5px; color: #6f6f7a }
</style>
</head><body><main>
  <h1>{{.Title}}</h1>
  <p>{{.Sub}}</p>

  <div class="cap">{{.CodeCap}}</div>
  <div class="code">{{.Code}}</div>

  <a class="go" href="frameplayer://join/{{.Code}}">{{.Open}}</a>
  <p class="sub">{{.Manual}}</p>

  {{if .Download}}
    <div class="rule"></div>
    <p class="sub">{{.NoPlayer}} <a class="get" href="{{.Download}}" rel="noopener">{{.Get}}</a></p>
  {{end}}

  <p class="foot">{{.Privacy}}</p>
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
	// The page varies by language, so a shared cache must not hand a Russian
	// copy to an English reader.
	w.Header().Set("Vary", "Accept-Language, User-Agent")
	_ = joinPage.Execute(w, joinPageData{
		pageText: pageTextFor(r.Header.Get("Accept-Language")),
		Code:     code,
		Download: s.downloadFor(r.Header.Get("User-Agent")),
	})
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
