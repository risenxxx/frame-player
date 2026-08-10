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

// The tab icon: the player's own viewfinder mark, from
// `src-tauri/icons/icon-master.svg`, with the comments and the 1024-canvas
// transform dropped so the paths sit in their own 24-grid.
//
// **SVG rather than a PNG, and one file rather than a set.** A favicon is asked
// for at 16, 32 and 64 depending on the browser, the tab bar and the display
// scale, and a vector answers all of them from the same bytes — which is the
// whole of the "what resolution" question.
//
// **One colour rather than a pair for light and dark.** The mark is a single
// mid-tone indigo on nothing at all: no tile, no background, so what sits behind
// it is the tab bar's own colour, and #6366f1 has enough contrast against both.
// A `prefers-color-scheme` block inside the SVG would be machinery for a problem
// this glyph does not have — the master file was drawn this way on purpose and
// says so.
//
// The `viewBox` is tighter than the master's: the ink spans 2.0–22.0 of the
// 24-grid, so starting at 1.2 with a 21.6 box leaves 0.8 of margin on every
// side — about 3.7 %, which is what the master intends and what keeps the glyph
// from shrinking to nothing inside a 16px tab.
//
// What this does not cover: Safari before 17, which ignores SVG favicons and
// falls back to `/favicon.ico`, and gets the browser's default mark. A 32px PNG
// beside this would close that, at the cost of a binary in the repository that
// has to be kept in step with the master by hand. Not worth it for a page
// opened once from a chat window.
const faviconSVG = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="1.2 1.2 21.6 21.6">` +
	`<path fill="none" stroke="#6366f1" stroke-width="2.4" stroke-linecap="round"` +
	` d="M3.2 7.6V5.9c0-1.5 1.2-2.7 2.7-2.7h1.7M16.4 3.2h1.7c1.5 0 2.7 1.2 2.7 2.7v1.7` +
	`M20.8 16.4v1.7c0 1.5-1.2 2.7-2.7 2.7h-1.7M7.6 20.8H5.9c-1.5 0-2.7-1.2-2.7-2.7v-1.7"/>` +
	`<path fill="#6366f1" stroke="#6366f1" stroke-width="1.6" stroke-linejoin="round"` +
	` d="M10 9v6l5.4-3z"/></svg>`

func (s *server) serveFavicon(w http.ResponseWriter, _ *http.Request) {
	w.Header().Set("Content-Type", "image/svg+xml")
	// A brand mark changes with a rebrand, not with a release, and a browser
	// asks for it on every tab.
	w.Header().Set("Cache-Control", "public, max-age=604800")
	_, _ = w.Write([]byte(faviconSVG))
}

// ---- what the page says -----------------------------------------------------

// A page in one language. Two of them rather than a lookup per string: this is
// a dozen phrases on one page, and a table of keys would be more machinery than
// the thing it holds.
type pageText struct {
	Lang string
	/// The product name, with the space that must never break. Data rather than
	/// markup because the template is a raw string literal, where `\u00a0` is
	/// six characters and not a space at all.
	Brand    string
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
	Lang:    "ru",
	Brand:   "Frame\u00a0Player",
	Title:   "Смотрим вместе",
	Sub:     "Один фильм, одна позиция, общие паузы.",
	CodeCap: "Код комнаты",
	// `\u00a0` rather than a literal non-breaking space, which is invisible in
	// the source and reads as an ordinary one to whoever edits it next. A product
	// name split across two lines is the one break worth forbidding outright;
	// everything else is left to `text-wrap: balance`.
	Open:     "Открыть в Frame\u00a0Player",
	Manual:   "Или введите код в плеере — «Смотреть вместе».",
	NoPlayer: "Ещё нет плеера?",
	Get:      "Скачать Frame\u00a0Player",
	Privacy:  "Через сервер идёт только позиция и пауза — не видео.",
}

var textEN = pageText{
	Lang:     "en",
	Brand:    "Frame\u00a0Player",
	Title:    "Watch together",
	Sub:      "One film, one position, the same pauses.",
	CodeCap:  "Room code",
	Open:     "Open in Frame\u00a0Player",
	Manual:   "Or open the player and choose “Watch together”.",
	NoPlayer: "Don’t have it yet?",
	Get:      "Get Frame\u00a0Player",
	Privacy:  "Only the position and pause travel through the server — never the video.",
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
// A direct download rather than a page, when the operator has configured one:
// somebody who has just been sent an invitation wants the player, not a release
// list.
//
// Either way the link opens in a new tab. A direct installer would not have
// taken this one with it — a browser navigating to something it cannot render
// downloads it and stays put — but the default is a *page*, and that does
// navigate away. The code has to still be there when they come back, because
// coming back is the entire point of the page.
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
<title>{{.Code}} · {{.Brand}}</title>
<link rel="icon" href="/favicon.svg" type="image/svg+xml">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Rubik:wght@300..900&display=swap">
<style>
  * { box-sizing: border-box }
  /* **The page centres itself inside an element of its own, and body is left an
     ordinary block.**

     Browser extensions append to document.body — Comet adds an empty
     browser-mcp-container element at the end of it, and others do the same.
     While body was the grid container, that injected node became a *second grid
     item*: the grid grew a second implicit row and place-items centred the
     content in the upper half instead of in the page. Measured by reproducing
     the injection locally, the block moved from 246-556 to 139-449 — a jump of
     107 px upward, which is exactly what was reported.

     A wrapper we own is the fix rather than a workaround: anything appended to
     body now lands after our box in normal flow and cannot reach our layout at
     all. Flex rather than grid inside it for the same reason one level down — a
     stray sibling in a centred row costs nothing vertically, where in a grid it
     becomes a row. Two cheap defences at the two places something can be
     injected, and the second is what keeps a future edit that drops the wrapper
     from silently bringing the bug back. */
  body {
    margin: 0;
    background: #101016;
    color: #e8e8ec;
    font: 400 15px/1.55 'Rubik', -apple-system, 'Segoe UI', system-ui, sans-serif;
    -webkit-font-smoothing: antialiased;
  }
  .page {
    /* svh, not vh — and the plain vh above it is what older browsers get.
       On a phone 100vh is the *large* viewport: the height the page would have
       once the address bar has scrolled away. At load, while the bar is still
       showing, content centred in it sits lower than the middle of what can
       actually be seen, and rises as the bar collapses. That is the one
       mechanism that matches "it starts nearer the centre and then jumps up",
       and svh — the small viewport, with the bar always counted — removes it by
       centring in the area that is visible the whole time.

       Measured on desktop, live and local, at 430x800: the webfont swap moves
       the top edge by 0 px and the block height by 1-2 px, so the font is not
       the cause and font-display was left alone.

       (No backticks anywhere in this file's CSS: the template is a raw string
       literal and one would end it. This is the second time.) */
    min-height: 100vh;
    min-height: 100svh;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
  }
  main { width: 100%; max-width: 340px; text-align: center }
  /* Every run of prose here is two or three lines in a fixed column, which is
     exactly the case text-wrap balance is for: without it each paragraph ended
     with a single word alone on its last line. Progressive enhancement — a
     browser that does not know the property lays the text out as before.
     (No backticks in this file's CSS: the template is a raw string literal and
     one would end it.) */
  h1, p { text-wrap: balance }
  h1 { font-size: 19px; font-weight: 500; margin: 0 0 6px; letter-spacing: .01em }
  p { margin: 0; color: #9a9aa6; font-size: 13.5px }
  .lead { color: #b9b9c3 }

  /* The code and the button are one block, joined edge to edge with the corners
     between them squared off. Apart, the box was a full-column container around
     six characters and its width looked arbitrary — nothing explained why it was
     that wide. Fused with the button the width belongs to the *block*, and the
     box reads as the button's own label rather than as a panel that happens to
     sit above it.

     The bottom border goes with the gap: a hairline resting on a solid indigo
     button reads as a seam rather than as an edge. */
  .codebox {
    margin: 22px 0 0;
    padding: 13px 16px 15px;
    border: 1px solid rgba(255, 255, 255, .09);
    border-bottom: none;
    border-radius: 14px 14px 0 0;
    background: rgba(255, 255, 255, .035);
  }
  .cap {
    font-size: 11px; font-weight: 500; text-transform: uppercase;
    letter-spacing: .11em; color: #7a7a88;
  }
  /* Monospaced and tracked out for the reason the player sets them that way: a
     code is read aloud and typed back, and that is where 0/O and 1/l go wrong.
     The tracking adds a trailing gap the box would otherwise centre against, so
     the same amount is padded back on the left. */
  .code {
    margin-top: 7px;
    font: 600 31px/1 ui-monospace, 'SF Mono', Menlo, Consolas, monospace;
    letter-spacing: .14em; padding-left: .14em; color: #e8e8ec;
  }

  .go {
    display: block; padding: 11px 18px; border: 0;
    /* The outer corners keep the block's radius; the two that meet the box
       above are square, which is what makes the pair one shape. */
    border-radius: 0 0 14px 14px;
    background: #6366f1; color: #fff; font: inherit; font-weight: 500;
    text-decoration: none; transition: background .15s ease;
  }
  .go:hover { background: #818cf8 }
  .sub { margin-top: 13px; font-size: 12.5px }
  /* The first line under the block stands as far from it as the block stands
     from the text above — otherwise the prose crowds a shape that is now twice
     as tall as it used to be. The paragraphs after it keep their own tighter
     rhythm, which is what separates "a note about the block" from the run of
     small print below. */
  .go + .sub { margin-top: 22px }
  .get { color: #c7cbff; text-decoration: none; border-bottom: 1px solid rgba(199, 203, 255, .35) }
  .get:hover { border-bottom-color: #c7cbff }
  .foot { margin-top: 22px; font-size: 11.5px; color: #64646f }
</style>
</head><body><div class="page"><main>
  <h1>{{.Title}}</h1>
  <p class="lead">{{.Sub}}</p>

  <!-- The caption and the code are one block, because on the page they are one
       thing. They used to be two siblings with the caption's gap below it three
       times the gap above — so it read as the tail of the sentence overhead
       rather than as the label of the six characters under it. Grouping them is
       what fixes that rather than a smaller margin: a box says "these belong
       together" in a way spacing has to be re-tuned to keep saying. -->
  <div class="codebox">
    <div class="cap">{{.CodeCap}}</div>
    <div class="code">{{.Code}}</div>
  </div>

  <a class="go" href="frameplayer://join/{{.Code}}">{{.Open}}</a>
  <p class="sub">{{.Manual}}</p>

  {{if .Download}}
    <p class="sub">{{.NoPlayer}} <a class="get" href="{{.Download}}" target="_blank" rel="noopener">{{.Get}}</a></p>
  {{end}}

  <p class="foot">{{.Privacy}}</p>
</main></div></body></html>
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
