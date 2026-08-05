#!/usr/bin/env bash
# Builds the macOS disk image from the .app Tauri has already bundled and
# signed. Kept out of `tauri build` on purpose — see scripts/dmg-settings.py
# for why Tauri's own dmg target cannot produce a laid-out image on CI.
#
# Set DMGBUILD to point at a virtualenv copy; CI does, since the runner's
# python is externally managed and refuses a plain `pip install`.
set -euo pipefail
cd "$(dirname "$0")/.."

APP="src-tauri/target/release/bundle/macos/Frame Player.app"
VERSION=$(jq -r .version src-tauri/tauri.conf.json)
OUT="src-tauri/target/release/bundle/dmg/Frame Player_${VERSION}_aarch64.dmg"

[ -d "$APP" ] || { echo "no app bundle at $APP — run the Tauri build first" >&2; exit 1; }
[ -f src-tauri/dmg/background.tiff ] || {
  echo "no background.tiff — run scripts/make-dmg-background.sh" >&2; exit 1; }

mkdir -p "$(dirname "$OUT")"
# dmgbuild refuses to overwrite, and a stale image from an earlier run would
# otherwise be the one the release workflow picks up by glob.
rm -f "$OUT"

"${DMGBUILD:-dmgbuild}" -s scripts/dmg-settings.py -D app="$APP" "Frame Player" "$OUT"

echo "wrote $OUT"
