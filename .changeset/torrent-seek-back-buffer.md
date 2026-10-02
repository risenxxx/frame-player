---
"frameplayer": patch
---

Torrents use up to 500 MB less memory during long playback

A torrent no longer keeps hundreds of megabytes of already-watched video in
memory for seeking back, since those parts are on disk anyway. On a 4K episode
that saved about 500 MB after a few minutes of playback. Streams from links
keep the larger buffer, where seeking back would otherwise download again.
