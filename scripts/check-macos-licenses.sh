#!/usr/bin/env bash
# Refuses a macOS library set that is not LGPL-or-weaker.
#
# The failure this guards against is silent in every other way: a bundle linked
# against Homebrew's GPL ffmpeg builds, runs, passes every test and ships — and
# the only thing wrong with it is a license, which no binary reports and no
# reviewer sees. It went unnoticed here for the whole life of the macOS build.
#
# Three independent checks, because each catches a different way in:
#
#   1. The library list. x264, x265 and rubberband are GPL and have no business
#      in the closure; they arrive by being linked, not by being asked for.
#   2. FFmpeg's own configuration line, read out of the binary that was built
#      rather than out of the flags someone meant to pass.
#   3. What libmpv actually links, which is the one that catches a build that
#      found Homebrew's ffmpeg through PKG_CONFIG_PATH instead of ours.
#
# Run: scripts/check-macos-licenses.sh   (after the libraries are in place)
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
lib_dir="$repo_root/src-tauri/lib"
cli="$repo_root/src-tauri/ffmpeg-macos/bin/ffmpeg"
fail=0

say() { printf '  %s %s\n' "$1" "$2"; }

# --- 1. no GPL libraries in the closure -------------------------------------
# Named individually rather than by pattern: the list is short, every entry has
# a reason, and a pattern would quietly start matching something innocent.
for gpl in libx264 libx265 librubberband; do
  hit="$(ls -1 "$lib_dir" 2>/dev/null | grep -E "^$gpl\." || true)"
  if [ -n "$hit" ]; then
    say '✗' "$gpl present ($hit) — GPL-2.0-or-later"
    fail=1
  else
    say '✓' "no $gpl"
  fi
done

# --- 2. FFmpeg was configured without the GPL opt-ins ------------------------
if [ ! -x "$cli" ]; then
  say '✗' "no $cli — cannot read the ffmpeg configuration"
  fail=1
else
  # The CLI's rpath is `@executable_path/lib`, which resolves only once build.rs
  # has put it beside the libraries — in the repo it sits one directory away, so
  # running it here needs the fallback path. Same reason `cargo test` on macOS
  # needs it (see CLAUDE.md). Without this the binary aborts on a missing dylib
  # and the check reads as a license failure rather than a layout one.
  cfg="$(DYLD_FALLBACK_LIBRARY_PATH="$lib_dir" "$cli" -hide_banner -version 2>/dev/null \
         | sed -n 's/^ *configuration: //p' || true)"
  if [ -z "$cfg" ]; then
    say '✗' "$cli would not run — cannot read its configuration"
    fail=1
  fi
  for opt in --enable-gpl --enable-nonfree --enable-version3; do
    if printf '%s' "$cfg" | grep -q -- "$opt"; then
      say '✗' "ffmpeg built with $opt"
      fail=1
    else
      say '✓' "ffmpeg without $opt"
    fi
  done
fi

# --- 3. libmpv links our FFmpeg and nothing GPL ------------------------------
# The subtle failure: meson finds Homebrew's ffmpeg through pkg-config, the
# build succeeds, and the result is GPL with every other check still green.
if [ -f "$lib_dir/libmpv.dylib" ]; then
  bad="$(otool -L "$lib_dir/libmpv.dylib" | tail -n +2 \
        | grep -oE '(x264|x265|rubberband)[^ ]*' | sort -u | tr '\n' ' ')"
  if [ -n "$bad" ]; then
    say '✗' "libmpv links: $bad"
    fail=1
  else
    say '✓' "libmpv links nothing GPL"
  fi
  # Anything still pointing at Homebrew would also mean the bundle is not
  # self-contained, which is a separate bug with the same cause.
  stray="$(otool -L "$lib_dir"/*.dylib 2>/dev/null | grep -cE '^\s+(/opt/homebrew|/usr/local)/' || true)"
  if [ "$stray" -gt 0 ]; then
    say '✗' "$stray absolute Homebrew path(s) left in the closure"
    fail=1
  else
    say '✓' "no absolute Homebrew paths"
  fi
else
  say '✗' "no $lib_dir/libmpv.dylib"
  fail=1
fi

echo
if [ "$fail" -ne 0 ]; then
  echo "licenses: FAILED — this set must not be published" >&2
  exit 1
fi
echo "licenses: LGPL-or-weaker, safe to publish."
