#!/bin/sh
# Builds version 2 on macOS: Skathi's JS packages (once per pin), the payload, the executable.
#   src-skathi/scripts/build-macos.sh            payload + exe (release)
#   src-skathi/scripts/build-macos.sh exe        the exe only
#   src-skathi/scripts/build-macos.sh payload    the payload only
# Needs version 1's FFmpeg SDK (`src-tauri/ffmpeg-macos`, made by scripts/bundle-macos-libs.sh);
# FFMPEG_DIR overrides where it is. CARGO_TARGET_DIR is honoured.
set -e
here=$(cd "$(dirname "$0")/.." && pwd)
repo=$(cd "$here/.." && pwd)
what=${1:-all}

export FFMPEG_DIR=${FFMPEG_DIR:-$repo/src-tauri/ffmpeg-macos}
export LIBCLANG_PATH=${LIBCLANG_PATH:-/Library/Developer/CommandLineTools/usr/lib}
export MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-13.0}

if [ "$what" != exe ]; then
  node "$here/scripts/fetch-skathi.mjs"
  (cd "$here/app" && bun install --frozen-lockfile && bun run payload)
fi
if [ "$what" != payload ]; then
  [ -d "$FFMPEG_DIR/include" ] || { echo "no FFmpeg SDK at $FFMPEG_DIR (scripts/bundle-macos-libs.sh, or set FFMPEG_DIR)" >&2; exit 1; }
  (cd "$here" && cargo build --release)
fi
