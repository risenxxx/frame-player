---
"frameplayer": patch
---

Second subtitles use less memory and keep up with a resize

The second subtitle is now drawn together with the picture instead of over it,
so it no longer makes the player hold on to around 200 MB of extra graphics
memory at times, and it stays in place while the window is being resized. It
looks and sits as before; when the controls appear it now moves out of their way
at once rather than sliding.
