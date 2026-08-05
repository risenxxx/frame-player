#!/usr/bin/env bash
# Renders the DMG background from its SVG master into a two-representation TIFF
# (1x + 2x). Finder picks the 2x rep on retina; a plain PNG is upscaled and
# visibly soft. tiffutil insists the second image be exactly twice the first.
#
# The output is committed, like the app icons: resvg is a dev-machine tool
# (Homebrew) and CI has no reason to grow a renderer just to rebuild an image
# that only changes when someone edits the SVG.
set -euo pipefail
cd "$(dirname "$0")/.."

SRC=src-tauri/dmg/background.svg
OUT=src-tauri/dmg/background.tiff

command -v resvg >/dev/null || { echo "resvg not found (brew install resvg)" >&2; exit 1; }

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

resvg --width 660 --height 440 "$SRC" "$TMP/bg.png"
resvg --width 1320 --height 880 "$SRC" "$TMP/bg@2x.png"
tiffutil -cathidpicheck "$TMP/bg.png" "$TMP/bg@2x.png" -out "$OUT"

echo "wrote $OUT"
