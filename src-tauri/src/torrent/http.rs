//! The loopback HTTP server mpv opens: `/t/<infohash>/<index>/<name>`, Range and HEAD.

use std::sync::Arc;

use bytes::Bytes;
use futures_util::StreamExt as _;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, StreamBody};
use hyper::body::Frame;
use hyper::service::service_fn;
use hyper::{header, Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::net::TcpListener;
use tokio_util::io::ReaderStream;

use super::TorrentService;

/// Read size for one body chunk. Matches librqbit's own streaming handler; a
/// piece is typically 1–16 MB, so this is well inside one and the reader blocks
/// on piece arrival rather than on the buffer.
const STREAM_CHUNK: usize = 65536;

type Body = BoxBody<Bytes, std::io::Error>;

impl TorrentService {
    /// The loopback HTTP server, started once and kept for the life of the app.
    ///
    /// Deliberately independent of the session: the session is torn down and
    /// rebuilt whenever the seeding preference changes, and rebinding a listener
    /// each time would leak an accept loop and hand out URLs on a port the old
    /// one still owns.
    pub(super) async fn ensure_server(self: &Arc<Self>) -> Result<u16, String> {
        {
            let inner = self.inner.lock().await;
            if inner.port != 0 {
                return Ok(inner.port);
            }
        }

        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|e| format!("cannot open local stream port: {e}"))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("cannot read local stream port: {e}"))?
            .port();

        let service = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    continue;
                };
                let service = service.clone();
                tauri::async_runtime::spawn(async move {
                    let io = TokioIo::new(stream);
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(
                            io,
                            service_fn(move |req| {
                                let service = service.clone();
                                async move { Ok::<_, std::convert::Infallible>(service.serve(req).await) }
                            }),
                        )
                        .await;
                });
            }
        });

        self.inner.lock().await.port = port;
        Ok(port)
    }

    /// The one route: `/t/<infohash>/<index>/<anything>`.
    async fn serve(self: Arc<Self>, req: Request<hyper::body::Incoming>) -> Response<Body> {
        if req.method() != Method::GET && req.method() != Method::HEAD {
            return simple(StatusCode::METHOD_NOT_ALLOWED);
        }

        let path = req.uri().path().to_string();
        let mut parts = path.trim_start_matches('/').splitn(4, '/');
        let (Some("t"), Some(hash), Some(index)) = (parts.next(), parts.next(), parts.next()) else {
            return simple(StatusCode::NOT_FOUND);
        };
        let Ok(index) = index.parse::<usize>() else {
            return simple(StatusCode::NOT_FOUND);
        };
        let name = parts.next().unwrap_or("").to_string();
        let range = req
            .headers()
            .get(header::RANGE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_string());
        self.serve_file(hash, index, &name, range, req.method() == Method::HEAD)
            .await
    }

    /// Serve one file of one torrent, Range and all. Split out of the route so
    /// the **cast** server can call it: a television cannot fetch from the
    /// loopback address this listener binds, and exposing this one to the LAN
    /// would put every torrent in the session on the network under a guessable
    /// path. So the cast side keeps its single-registered-source-behind-a-token
    /// rule and streams through here.
    pub(crate) async fn serve_file(
        self: &Arc<Self>,
        hash: &str,
        index: usize,
        name: &str,
        range: Option<String>,
        head_only: bool,
    ) -> Response<Body> {
        // The request itself is what selects the file: see the header of mod.rs.
        //
        // The reason is logged rather than swallowed. This used to answer a bare
        // 404 for every failure, and "the torrent is still initializing" then
        // looked exactly like "no such file" — which cost a diagnosis, because
        // the only visible symptom was a player that would not start on some
        // torrents and would on others.
        let handle = match self.select(hash, index).await {
            Ok(h) => h,
            Err(e) => {
                eprintln!("[torrent] select {hash}/{index} failed: {e}");
                // Not 404: the file exists, it is not ready. A distinct code
                // keeps the two apart in a log and tells ffmpeg to give up on
                // this attempt rather than on the URL.
                return simple(StatusCode::SERVICE_UNAVAILABLE);
            }
        };

        let mut stream = match handle.stream(index).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[torrent] stream {hash}/{index} failed: {e:#}");
                return simple(StatusCode::SERVICE_UNAVAILABLE);
            }
        };
        let total = stream.len();

        let mut res = Response::builder()
            // Without this ffmpeg treats the source as unseekable and the
            // seekbar becomes a progress bar.
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_TYPE, mime_for(name))
            // A DLNA renderer reads these before it will seek; harmless to
            // everyone else (see cast/server.rs, where the same pair is set).
            .header("transferMode.dlna.org", "Streaming")
            .header(
                "contentFeatures.dlna.org",
                "DLNA.ORG_OP=01;DLNA.ORG_CI=0;\
                 DLNA.ORG_FLAGS=01700000000000000000000000000000",
            );

        let range = range.as_deref().and_then(|v| parse_range(v, total));

        let (status, start, len) = match range {
            Some(Some((start, end_inclusive))) => {
                res = res.header(
                    header::CONTENT_RANGE,
                    format!("bytes {start}-{end_inclusive}/{total}"),
                );
                (
                    StatusCode::PARTIAL_CONTENT,
                    start,
                    end_inclusive - start + 1,
                )
            }
            // A Range header that was present and unsatisfiable. Saying so is
            // what makes ffmpeg fall back sanely instead of reading garbage.
            Some(None) => {
                return Response::builder()
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header(header::CONTENT_RANGE, format!("bytes */{total}"))
                    .body(empty())
                    .unwrap()
            }
            None => (StatusCode::OK, 0, total),
        };

        let res = res.status(status).header(header::CONTENT_LENGTH, len);

        if head_only {
            return res.body(empty()).unwrap();
        }

        if start > 0 && stream.seek(std::io::SeekFrom::Start(start)).await.is_err() {
            return simple(StatusCode::INTERNAL_SERVER_ERROR);
        }

        let body = StreamBody::new(
            ReaderStream::with_capacity(stream.take(len), STREAM_CHUNK).map(|r| r.map(Frame::data)),
        );
        res.body(BoxBody::new(body)).unwrap()
    }
}

pub(super) fn stream_url(port: u16, info_hash: &str, index: usize, path: &str) -> String {
    // Only the last component goes into the URL, and only to give ffmpeg an
    // extension: the path inside a torrent may contain anything, and the
    // identity is already in the hash and the index.
    let name = path.rsplit('/').next().unwrap_or("video");
    format!(
        "http://127.0.0.1:{port}/t/{info_hash}/{index}/{}",
        urlencode(name)
    )
}

/// Percent-encode everything that is not unreserved. Small and local rather than
/// a dependency: this escapes one file name for one loopback URL. `pub(crate)`
/// because cast/ builds its URLs the same way.
pub(crate) fn urlencode(s: &str) -> String {
    urlencode_bytes(s.as_bytes())
}

pub(super) fn urlencode_bytes(s: &[u8]) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `Some(Some((start, end_inclusive)))` for a usable range, `Some(None)` for one
/// that is present but unsatisfiable, `None` for a header we do not understand
/// (which is served as the whole file, per RFC 9110).
///
/// Only the single-range `bytes=` form: that is what ffmpeg sends (and what a
/// Cast receiver sends — cast/server.rs shares this parser), and a multipart response
/// to a media player would be answering a question nobody asked.
pub(crate) fn parse_range(value: &str, total: u64) -> Option<Option<(u64, u64)>> {
    let spec = value.trim().strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (start, end) = spec.split_once('-')?;
    let (start, end) = (start.trim(), end.trim());

    // A suffix range ("last N bytes") — ffmpeg uses it to read a trailing index.
    if start.is_empty() {
        let n: u64 = end.parse().ok()?;
        if n == 0 || total == 0 {
            return Some(None);
        }
        let n = n.min(total);
        return Some(Some((total - n, total - 1)));
    }

    let start: u64 = start.parse().ok()?;
    if start >= total {
        return Some(None);
    }
    let end = if end.is_empty() {
        total - 1
    } else {
        end.parse::<u64>().ok()?.min(total - 1)
    };
    if end < start {
        return Some(None);
    }
    Some(Some((start, end)))
}

fn mime_for(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "mov" => "video/quicktime",
        "ts" | "m2ts" | "mts" => "video/mp2t",
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "m4a" | "aac" => "audio/mp4",
        _ => "application/octet-stream",
    }
}

fn empty() -> Body {
    BoxBody::new(Empty::<Bytes>::new().map_err(|e| match e {}))
}

fn simple(status: StatusCode) -> Response<Body> {
    Response::builder().status(status).body(empty()).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Range header is the whole seek contract with ffmpeg: get it wrong and
    /// the picture is either garbage or the seekbar stops working, both of which
    /// look like a torrent problem rather than a parsing one.
    #[test]
    fn ranges() {
        // Plain, open-ended, and a single byte.
        assert_eq!(parse_range("bytes=0-99", 1000), Some(Some((0, 99))));
        assert_eq!(parse_range("bytes=500-", 1000), Some(Some((500, 999))));
        assert_eq!(parse_range("bytes=0-0", 1000), Some(Some((0, 0))));
        assert_eq!(parse_range(" bytes=10-20 ", 1000), Some(Some((10, 20))));

        // An end past the file is clamped rather than refused — ffmpeg asks for
        // more than exists when it probes near the tail, and RFC 9110 says the
        // server clamps.
        assert_eq!(parse_range("bytes=900-5000", 1000), Some(Some((900, 999))));

        // Suffix form: the last N bytes. This is how ffmpeg reads an MP4 moov
        // atom that sits at the end of the file, so it is the first request of
        // playback for a large class of torrents — and it is why the whole tail
        // of the file has to be reachable before any of the middle.
        assert_eq!(parse_range("bytes=-100", 1000), Some(Some((900, 999))));
        assert_eq!(parse_range("bytes=-5000", 1000), Some(Some((0, 999))));

        // Present but unsatisfiable → 416, which ffmpeg handles. Answering 200
        // with the whole file instead would have it decode from the wrong offset.
        assert_eq!(parse_range("bytes=1000-", 1000), Some(None));
        assert_eq!(parse_range("bytes=50-20", 1000), Some(None));
        assert_eq!(parse_range("bytes=-0", 1000), Some(None));

        // Not understood → None, served as the whole file.
        assert_eq!(parse_range("items=0-1", 1000), None);
        assert_eq!(parse_range("bytes=0-10,20-30", 1000), None);
        assert_eq!(parse_range("bytes=abc-", 1000), None);
    }

    /// The identity the frontend reads back out of the URL. If this shape
    /// changes, `parseTorrentUrl` in source.ts changes with it.
    #[test]
    fn urls_carry_the_identity() {
        let url = stream_url(51234, "abcdef", 3, "Season 1/Ep 3 [1080p].mkv");
        assert_eq!(
            url,
            "http://127.0.0.1:51234/t/abcdef/3/Ep%203%20%5B1080p%5D.mkv"
        );
        // The extension survives encoding — ffmpeg probes by it.
        assert!(url.ends_with(".mkv"));
    }

    /// mpv opens what `stream_url` builds, and the name is part of it — and a
    /// release named in Cyrillic or with brackets and spaces is the ordinary
    /// case, not the edge. Each byte of the UTF-8 is escaped, a slash inside a
    /// name never becomes a path separator, and the unreserved four stay as
    /// they are (RFC 3986), so the extension ffmpeg probes by survives.
    #[test]
    fn names_are_escaped_byte_by_byte() {
        assert_eq!(urlencode("Брат 2.mkv"), "%D0%91%D1%80%D0%B0%D1%82%202.mkv");
        assert_eq!(urlencode("a/b?c#d&e"), "a%2Fb%3Fc%23d%26e");
        assert_eq!(urlencode("A-z_0.9~"), "A-z_0.9~");
        assert_eq!(urlencode("%20"), "%2520");
    }

    /// What mpv is told a stream is. By the extension and in any case, and an
    /// unknown one is octet-stream rather than a guess — a wrong video type is
    /// worse than none, since the demuxer probes the bytes either way.
    #[test]
    fn mime_by_extension() {
        assert_eq!(mime_for("Film.MKV"), "video/x-matroska");
        assert_eq!(mime_for("a.b.mp4"), "video/mp4");
        assert_eq!(mime_for("clip.m2ts"), "video/mp2t");
        assert_eq!(mime_for("README"), "application/octet-stream");
        assert_eq!(mime_for("noext."), "application/octet-stream");
    }
}
