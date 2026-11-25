#!/usr/bin/env bash
# The macOS beta's drag-to-Applications disk image, built around the .app Velopack packed.
#
#   src-skathi/scripts/dmg-macos.sh FEED OUT.dmg "<Developer ID Application>" <notarytool profile>
#
# FEED is `vpk pack`'s output folder. Its *-Portable.zip holds the very bundle the feed's packages
# describe — Velopack's `UpdateMac` and `sq.version` (id FramePlayerBeta, channel osx-arm64-beta)
# inside, signed with the hardened runtime, notarized and stapled by vpk — so an app dragged out of
# this image updates from the same feed as one the .pkg installed.
#
# The layout is version 1's: scripts/dmg-settings.py (dmgbuild writes the .DS_Store itself, no
# Finder scripting) with src-tauri/dmg/background.tiff and the app icon. dmgbuild comes from PyPI
# (`DMGBUILD` points at a virtualenv copy, as release.yml does for version 1). The image is signed
# with the Developer ID Application identity, notarized and stapled; the app keeps its own ticket.
set -euo pipefail
cd "$(dirname "$0")/../.."

FEED=$1 OUT=$2 IDENTITY=$3 PROFILE=$4
ZIP=$(ls "$FEED"/*-Portable.zip 2>/dev/null | head -1)
[ -f "$ZIP" ] || { echo "no *-Portable.zip in $FEED — run vpk pack first" >&2; exit 1; }

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
ditto -x -k "$ZIP" "$TMP"
APP=$(ls -d "$TMP"/*.app | head -1)
NAME=$(basename "$APP" .app)

# The bundle must be what vpk shipped: signed, notarized, with Velopack's files.
codesign --verify --deep --strict "$APP"
xcrun stapler validate "$APP" >/dev/null
[ -f "$APP/Contents/MacOS/UpdateMac" ] && [ -f "$APP/Contents/MacOS/sq.version" ] ||
  { echo "$APP has no UpdateMac/sq.version: not a Velopack bundle" >&2; exit 1; }

rm -f "$OUT"
"${DMGBUILD:-dmgbuild}" -s scripts/dmg-settings.py -D app="$APP" "$NAME" "$OUT"

codesign --force --timestamp --sign "$IDENTITY" "$OUT"
xcrun notarytool submit "$OUT" --keychain-profile "$PROFILE" --wait
xcrun stapler staple "$OUT"
xcrun stapler validate "$OUT"
spctl -a -vv -t open --context context:primary-signature "$OUT"
echo "wrote $OUT"
