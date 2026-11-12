---
"frameplayer": patch
---

Direct stream links open more often, and say why when they do not

The player now introduces itself to servers the way a browser does, so a direct link to a video or an HLS stream (.m3u8) that a site refused before can open without yt-dlp. When a link still fails, the dialog says what the server answered — access refused, nothing at that address, or another error — instead of always suggesting yt-dlp, which only helps with links to a site's page.
