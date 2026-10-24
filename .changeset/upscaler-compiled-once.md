---
"frameplayer": patch
---

Video upscaling no longer freezes the picture each time the player starts

On Windows the Video upscaling mode needs ten seconds or more of preparation the first time the picture grows large enough for it to run, and the video stood still meanwhile — typically in the middle of resizing the window or going full screen. The player used to throw that work away when it closed and repeat it in every session; it now keeps it, so the pause happens once on a computer rather than every time.
