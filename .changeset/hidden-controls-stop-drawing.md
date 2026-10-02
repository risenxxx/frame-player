---
"frameplayer": patch
---

Uses about 200 MB less memory while a video plays

Once the controls have faded out they are no longer drawn at all. Until now
they were only made transparent, so the seekbar kept being redrawn on every
frame of the video. On macOS that alone held around 200 MB of graphics memory
for as long as the film played. On macOS the player also no longer keeps a
hidden helper window that only Windows needs.
