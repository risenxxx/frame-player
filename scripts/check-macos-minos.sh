#!/usr/bin/env bash
# Checks that every native library in src-tauri/lib, and the ffmpeg CLI, loads
# on the oldest macOS the app says it supports.
#
# The floor is `bundle.macOS.minimumSystemVersion` in tauri.macos.conf.json:
# macOS will not open the app below it. Each Mach-O file records its own floor
# (LC_BUILD_VERSION `minos`), and dyld refuses to load a file whose floor is
# above the running system. When that happens nothing fails at startup. The
# wrapper `dlopen`s libmpv only when mpv is created, and the viewer gets the
# plugin's bare "Failed to create mpv instance". That reached macOS 15 users
# from a set built on macos-26, where every Homebrew bottle and our own FFmpeg
# and mpv were marked 26.0.
#
# Run by build-macos-libs.sh, by fetch-macos-libs.sh against every set it
# downloads, and by the macOS libs workflow. Lowering the floor in the config
# without rebuilding the set therefore fails at fetch time, not on a viewer's
# machine.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
conf="$repo_root/src-tauri/tauri.macos.conf.json"

floor="$(node -e 'process.stdout.write(String(JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).bundle?.macOS?.minimumSystemVersion ?? ""))' "$conf")"
if [ -z "$floor" ]; then
  echo "FATAL: no bundle.macOS.minimumSystemVersion in $conf" >&2
  exit 1
fi

# "14.0" → 14000, "10.15.7" → 101507: two-digit minor and patch, so plain
# integer comparison orders versions correctly.
as_int() { awk -F. '{ printf "%d%02d%02d\n", $1, $2, $3 }' <<<"$1"; }
floor_int="$(as_int "$floor")"

# The `minos` of the first LC_BUILD_VERSION, or of LC_VERSION_MIN_MACOSX
# (`version`) in files built by older toolchains.
minos_of() {
  otool -l "$1" | awk '
    /cmd LC_BUILD_VERSION/      { want = "minos" }
    /cmd LC_VERSION_MIN_MACOSX/ { want = "version" }
    want != "" && $1 == want    { print $2; exit }'
}

files=("$repo_root"/src-tauri/lib/*.dylib)
[ -f "$repo_root/src-tauri/ffmpeg-macos/bin/ffmpeg" ] && files+=("$repo_root/src-tauri/ffmpeg-macos/bin/ffmpeg")
if [ ! -f "${files[0]}" ]; then
  echo "FATAL: no libraries in src-tauri/lib" >&2
  exit 1
fi

bad=0
newest="0"
for f in "${files[@]}"; do
  v="$(minos_of "$f")"
  if [ -z "$v" ]; then
    echo "no minimum macOS version recorded in $(basename "$f")" >&2
    bad=1
    continue
  fi
  [ "$(as_int "$v")" -gt "$(as_int "$newest")" ] && newest="$v"
  if [ "$(as_int "$v")" -gt "$floor_int" ]; then
    echo "  $(basename "$f") needs macOS $v" >&2
    bad=1
  fi
done

if [ "$bad" -ne 0 ]; then
  echo "FATAL: the set does not load on macOS $floor (minimumSystemVersion)." >&2
  echo "Build it on macOS $floor — the \"macOS libs\" workflow does — or raise the floor." >&2
  exit 1
fi
echo "every library loads on macOS $floor (newest floor in the set: $newest, ${#files[@]} files)"
