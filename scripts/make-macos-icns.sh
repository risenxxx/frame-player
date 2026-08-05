#!/usr/bin/env bash
# Rebuilds src-tauri/icons/icon.icns from icon-master-macos.svg.
#
# Touches ONLY the .icns, which is used by the macOS bundle alone. The Windows
# icon (icon.ico) and the PNG set are left untouched — which is why
# `npm run tauri icon` is not usable here: it would regenerate the whole set
# from a single source.
#
# Requires: resvg (brew install resvg) and iconutil (part of macOS).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
icons="$repo_root/src-tauri/icons"
src="$icons/icon-master-macos.svg"
iconset="${TMPDIR:-/tmp}/frameplayer-icon.iconset"

command -v resvg >/dev/null || { echo "resvg not found: brew install resvg" >&2; exit 1; }

rm -rf "$iconset"
mkdir -p "$iconset"

# Names must follow iconutil's format exactly: <base>[@2x].png
render() { resvg "$src" "$iconset/$2" -w "$1" -h "$1"; }

render 16   icon_16x16.png
render 32   icon_16x16@2x.png
render 32   icon_32x32.png
render 64   icon_32x32@2x.png
render 128  icon_128x128.png
render 256  icon_128x128@2x.png
render 256  icon_256x256.png
render 512  icon_256x256@2x.png
render 512  icon_512x512.png
render 1024 icon_512x512@2x.png

iconutil -c icns "$iconset" -o "$icons/icon.icns"
rm -rf "$iconset"

echo "Written: $icons/icon.icns"
sips -g pixelWidth -g pixelHeight "$icons/icon.icns" | tail -2
