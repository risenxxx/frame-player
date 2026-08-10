// The TMDB proxy is not a Go library and is never `go get`-ed, so the module
// path is a name rather than a URL — the same reasoning as the relay's, and the
// same rule about never naming a host in this repository.
//
// No dependencies at all. The relay has one (a WebSocket implementation, which
// is not worth writing); this is an HTTP client and an HTTP server, both of
// which the standard library does better than a wrapper would.
module frameplayer/tmdb

go 1.24
