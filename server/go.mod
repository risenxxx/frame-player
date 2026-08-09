// The relay is not a Go library and is never `go get`-ed, so the module path is
// a name rather than a URL. Naming a host here would also be the one thing the
// repository's own rule forbids (see the note at the end of CLAUDE.md).
module frameplayer/relay

go 1.24

require github.com/coder/websocket v1.8.15
