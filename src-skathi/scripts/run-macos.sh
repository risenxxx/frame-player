#!/bin/sh
# Starts a development build in the background, muted, without taking focus, with its own data
# directory (src-skathi/runs/NAME/data), never version 1's or the installed beta's.
#   MPV_LIB_DIR=/path/to/libmpv/dir src-skathi/scripts/run-macos.sh NAME [file …]
# The log is src-skathi/runs/NAME/log; the PID is printed.
set -e
here=$(cd "$(dirname "$0")/.." && pwd)
name=$1; shift
dir="$here/runs/$name"
mkdir -p "$dir/data"
exe="${CARGO_TARGET_DIR:-$here/target}/release/frameplayer"
FPUI_DATA_DIR="$dir/data" FPUI_MUTE=${FPUI_MUTE:-1} SK_WINDOW_FOCUS=${SK_WINDOW_FOCUS:-never} \
  "$exe" "$@" > "$dir/log" 2>&1 &
echo $!
