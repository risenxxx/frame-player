#!/usr/bin/env bash
# Builds the **LGPL** macOS native-library set: FFmpeg from source with the GPL
# parts left out, then the patched libmpv against it, then the flattened dylib
# closure in src-tauri/{lib,ffmpeg-macos}.
#
# This exists because Homebrew's ffmpeg is a GPL-3 build (`--enable-gpl`, and it
# drags in x264 and x265), and mpv linked against it is GPL too. Shipping that
# is legal but makes the whole distributed .app GPL-3 while the project calls
# itself MIT — the label and the contents have to agree. Windows has been LGPL
# all along (`mpv-dev-lgpl`, `ffmpeg-…-lgpl-shared`); this is the macOS half of
# that parity.
#
# **Nothing is lost by dropping the GPL parts**, which is what makes this cheap:
#
#   - x264 / x265 are *encoders*, and the player never encodes video. Every
#     ffmpeg invocation in cast/ passes `-c:v copy`; only audio is transcoded,
#     to AAC or E-AC-3, both native FFmpeg encoders. The sidecar smoke test
#     already uses `mpeg4` for exactly this reason.
#   - librubberband is mpv's optional pitch-correction filter. The default has
#     been the built-in `scaletempo2` since mpv 0.36, and nothing in this
#     project asks for rubberband.
#
# It replaces `build-libmpv-macos.sh`, which built against Homebrew's GPL
# ffmpeg. That script is deleted rather than kept beside this one: two build
# scripts differing only in the license of what they produce is an invitation to
# run the wrong one, and the result would look and work identically.
#
# Requires: Xcode Command Line Tools, Homebrew, nasm (for FFmpeg's asm).
set -euo pipefail

MPV_VERSION="0.41.0"
# Pinned to the patch level, not the series: `8.1` is a real tarball too, and
# taking it would mean two runs of this script producing different libraries
# from identical inputs — which the content-addressed artifact key would then
# report as the same set. Matches the Windows side's 8.1.x.
FFMPEG_VERSION="8.1.2"
WRAPPER_VERSION="v0.1.1"
# Bumping any of these changes the artifact key: the macos-libs workflow hashes
# this file, so a version pin and the name of the published set move together.

# The oldest macOS the libraries load on. Every dylib carries a minimum OS
# version (LC_BUILD_VERSION `minos`) and dyld refuses one newer than the running
# system. The wrapper `dlopen`s libmpv only when mpv is created, so a set that
# is too new does not stop the app from starting: it fails later with the
# plugin's bare "Failed to create mpv instance". That is how a set built on
# macos-26 reached macOS 15 users.
#
# Two things decide the number, and only one of them is set here. FFmpeg and
# mpv follow MACOSX_DEPLOYMENT_TARGET. The Homebrew dependencies (libplacebo,
# libass, luajit, …) are bottles built for the OS the *build machine* runs,
# whatever this variable says. So the floor is really the build machine's
# macOS: the workflow runs on macos-14, and the check below refuses a set built
# anywhere newer. `bundle.macOS.minimumSystemVersion` in tauri.macos.conf.json
# has to match, and `check-macos-minos.sh` holds the two together.
MACOS_MIN="14.0"
export MACOSX_DEPLOYMENT_TARGET="$MACOS_MIN"

host_major="$(sw_vers -productVersion | cut -d. -f1)"
if [ "$host_major" -gt "${MACOS_MIN%%.*}" ]; then
  msg="this machine runs macOS $(sw_vers -productVersion), so Homebrew's bottles need at least that; the set would not load below it (floor: $MACOS_MIN)"
  if [ -n "${CI:-}" ]; then
    # In CI this is the release job's fallback running on its own macos-26
    # runner, and what it built would ship. Better to stop here than twenty
    # minutes from now at the minos check.
    echo "FATAL: $msg. Run the \"macOS libs\" workflow, which builds on macos-14." >&2
    exit 1
  fi
  echo "WARNING: $msg. Fine for a development machine; do not publish it." >&2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
lib_dir="$repo_root/src-tauri/lib"
# Every patch written for this mpv version, in the order the shell sorts them —
# the same glob, and therefore the same order, `macos-libs-key.sh` hashes. A
# list of names here instead would be a second place to forget: a patch added
# to the directory and not to the list changes the name of the published set
# and nothing inside it.
mpv_patches=("$repo_root/patches/mpv-$MPV_VERSION-"*.patch)
if [ ! -f "${mpv_patches[0]}" ]; then
  echo "FATAL: no patches/mpv-$MPV_VERSION-*.patch — a version bump has to carry its patches along" >&2
  exit 1
fi
# The floor is part of the name: a tree configured for another target is kept
# between runs and would not be rebuilt.
work="${TMPDIR:-/tmp}/frameplayer-lgpl-$FFMPEG_VERSION-$MPV_VERSION-macos$MACOS_MIN"
# FFmpeg is installed into a prefix of its own rather than over Homebrew's:
# leaving the machine's own ffmpeg alone means this script is safe to run on a
# development box, and it keeps the two builds distinguishable in `otool -L`.
ff_prefix="$work/ffmpeg-lgpl"

case "$(uname -m)" in
  arm64)  wrapper_arch="macos-aarch64" ;;
  x86_64) wrapper_arch="macos-x86_64" ;;
  *) echo "unsupported arch: $(uname -m)" >&2; exit 1 ;;
esac

echo "==> Build deps (Homebrew)"
# ffmpeg is deliberately NOT in this list: building it is the point. dav1d is,
# because AV1 decoding through the native decoder is far slower and dav1d is
# BSD-2 — no license cost. Everything else here is a dependency of mpv, and none
# of them links FFmpeg back in (verified with otool: libass, libplacebo, luajit,
# uchardet, little-cms2, libarchive and libbluray reference no libav*).
brew install -q meson ninja pkgconf nasm libplacebo libass luajit \
                vulkan-loader molten-vk little-cms2 libarchive uchardet dav1d

mkdir -p "$work"

# ---------------------------------------------------------------------------
echo "==> FFmpeg $FFMPEG_VERSION (LGPL)"
cd "$work"
if [ ! -d "ffmpeg-$FFMPEG_VERSION" ]; then
  curl -sSL -o "ffmpeg-$FFMPEG_VERSION.tar.xz" \
    "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VERSION.tar.xz"
  tar xf "ffmpeg-$FFMPEG_VERSION.tar.xz"
fi
cd "ffmpeg-$FFMPEG_VERSION"
if [ ! -f config.h ]; then
  # `--enable-gpl` and `--enable-version3` are both opt-in and both stay off:
  # FFmpeg is LGPL-2.1+ by default and only becomes GPL because a packager asks
  # it to. `--enable-nonfree` would make the result undistributable entirely.
  #
  # The external libraries are one: dav1d. Everything the player needs to decode
  # is native, and every encoder it uses (aac, eac3, mpeg4, png) is native too,
  # so the usual long list of --enable-lib* buys nothing here and each entry is
  # another license to account for.
  ./configure \
    --prefix="$ff_prefix" \
    --enable-shared --disable-static \
    --disable-gpl --disable-nonfree --disable-version3 \
    --disable-doc --disable-programs --enable-ffmpeg \
    --enable-libdav1d \
    --enable-videotoolbox \
    --disable-debug
fi
make -j"$(sysctl -n hw.ncpu)"
make install

# The check that matters, run against the thing that was actually built rather
# than against the flags that were meant to be passed.
#
# Captured into a variable, never piped into `grep -q`. Under `pipefail` that
# idiom **fails open**: grep exits at the first match, the writer can take
# SIGPIPE, the pipeline reports non-zero, and the `if` skips its own body — so a
# GPL build would sail through the check meant to stop it. Whether the SIGPIPE
# actually lands depends on the output fitting the pipe buffer, which is a
# coin-flip nobody should be resting a licence check on.
ff_version="$("$ff_prefix/bin/ffmpeg" -hide_banner -version)"
case "$ff_version" in
  *--enable-gpl*|*--enable-nonfree*|*--enable-version3*)
    echo "FATAL: the built ffmpeg is not LGPL — see its configuration line" >&2
    printf '%s\n' "$ff_version" | sed -n '2p' >&2
    exit 1 ;;
esac

# ---------------------------------------------------------------------------
echo "==> mpv $MPV_VERSION (patched, -Dgpl=false)"
cd "$work"
# The tree is patched when it is unpacked and kept between runs, so a tree left
# by an earlier run carries the patches of *that* run. Without the stamp a new
# patch was skipped on any machine that had built before, in silence: the build
# succeeds and the library is the old one.
patch_stamp="$(cat "${mpv_patches[@]}" | shasum -a 256 | cut -c1-12)"
if [ -d "mpv-$MPV_VERSION" ] &&
   [ "$(cat "mpv-$MPV_VERSION/.patch-stamp" 2>/dev/null || true)" != "$patch_stamp" ]; then
  echo "==> The patch set changed, unpacking a clean tree"
  rm -rf "mpv-$MPV_VERSION"
fi
if [ ! -d "mpv-$MPV_VERSION" ]; then
  curl -sSL -o "mpv-$MPV_VERSION.tar.gz" \
    "https://github.com/mpv-player/mpv/archive/refs/tags/v$MPV_VERSION.tar.gz"
  tar xzf "mpv-$MPV_VERSION.tar.gz"
  for patch_file in "${mpv_patches[@]}"; do
    echo "==> Applying $(basename "$patch_file")"
    ( cd "mpv-$MPV_VERSION" && patch -p1 < "$patch_file" )
  done
  # Last, so that a tree whose patching was interrupted is not taken for a
  # finished one.
  echo "$patch_stamp" > "mpv-$MPV_VERSION/.patch-stamp"
fi
cd "mpv-$MPV_VERSION"
if [ ! -d build ]; then
  # PKG_CONFIG_PATH first, so meson finds our FFmpeg rather than Homebrew's —
  # this is the whole point and it is silent when it goes wrong: a build against
  # the wrong ffmpeg links and runs, and is GPL.
  PKG_CONFIG_PATH="$ff_prefix/lib/pkgconfig:${PKG_CONFIG_PATH:-}" \
  meson setup build \
    -Dlibmpv=true \
    -Dcplayer=false \
    -Dgpl=false \
    -Drubberband=disabled \
    -Dvulkan=enabled \
    -Dlua=luajit \
    -Dbuildtype=release
fi
ninja -C build

echo "==> Installing into $lib_dir"
mkdir -p "$lib_dir"
cp "build/libmpv.2.dylib" "$lib_dir/libmpv.dylib"
# Homebrew/meson artifacts can land read-only; tauri-build's resource copy then
# fails with a bare EACCES.
chmod 644 "$lib_dir/libmpv.dylib"

if [ ! -f "$lib_dir/libmpv-wrapper.dylib" ]; then
  echo "==> Fetching libmpv-wrapper $WRAPPER_VERSION ($wrapper_arch)"
  curl -sSL -o "$work/wrapper.zip" \
    "https://github.com/nini22P/libmpv-wrapper/releases/download/$WRAPPER_VERSION/libmpv-wrapper-$wrapper_arch.zip"
  rm -rf "$work/wrapper" && unzip -o -q "$work/wrapper.zip" -d "$work/wrapper"
  cp "$work/wrapper/bin/libmpv-wrapper.dylib" "$lib_dir/"
  chmod 644 "$lib_dir/libmpv-wrapper.dylib"
fi

# ---------------------------------------------------------------------------
echo
echo "==> Making the dylib set self-contained"
FFMPEG_PREFIX="$ff_prefix" "$repo_root/scripts/bundle-macos-libs.sh"

echo
echo "==> License check"
"$repo_root/scripts/check-macos-licenses.sh"

echo
echo "==> Minimum macOS"
if ! "$repo_root/scripts/check-macos-minos.sh"; then
  # Already refused up front in CI; on a newer development machine the set is
  # still good for that machine, so it stays installed.
  [ -n "${CI:-}" ] && exit 1
  echo "WARNING: this set is for this machine only" >&2
fi
