---
"frameplayer": patch
---

The player starts on macOS 14 and 15 again

On macOS versions older than 26 the player opened but could not start playback and showed "Could not start mpv". The libraries it plays video with were built for macOS 26 only. They are now built for macOS 14 and later, and macOS 13 or older says plainly that the app needs a newer system.
