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
#     ffmpeg invocation in cast.rs passes `-c:v copy`; only audio is transcoded,
#     to AAC or E-AC-3, both native FFmpeg encoders. The sidecar smoke test
#     already uses `mpeg4` for exactly this reason.
#   - librubberband is mpv's optional pitch-correction filter. The default has
#     been the built-in `scaletempo2` since mpv 0.36, and nothing in this
#     project asks for rubberband.
#
# Output is byte-identical in shape to `build-libmpv-macos.sh`, so the rest of
# the build does not care which produced it.
#
# Requires: Xcode Command Line Tools, Homebrew, nasm (for FFmpeg's asm).
set -euo pipefail

MPV_VERSION="0.41.0"
# Pinned to the patch level, not the series: `8.1` is a real tarball too, and
# taking it would mean two runs of this script producing different libraries
# from identical inputs — which the content-addressed artefact key would then
# report as the same set. Matches the Windows side's 8.1.x.
FFMPEG_VERSION="8.1.2"
WRAPPER_VERSION="v0.1.1"
# Bumping any of these changes the artefact key: the macos-libs workflow hashes
# this file, so a version pin and the name of the published set move together.

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
lib_dir="$repo_root/src-tauri/lib"
patch_file="$repo_root/patches/mpv-$MPV_VERSION-macos-wid-embedding.patch"
work="${TMPDIR:-/tmp}/frameplayer-lgpl-$FFMPEG_VERSION-$MPV_VERSION"
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
if "$ff_prefix/bin/ffmpeg" -hide_banner -version | grep -qE -- '--enable-(gpl|nonfree|version3)'; then
  echo "FATAL: the built ffmpeg is not LGPL — see its configuration line" >&2
  "$ff_prefix/bin/ffmpeg" -hide_banner -version | sed -n '2p' >&2
  exit 1
fi

# ---------------------------------------------------------------------------
echo "==> mpv $MPV_VERSION (patched, -Dgpl=false)"
cd "$work"
if [ ! -d "mpv-$MPV_VERSION" ]; then
  curl -sSL -o "mpv-$MPV_VERSION.tar.gz" \
    "https://github.com/mpv-player/mpv/archive/refs/tags/v$MPV_VERSION.tar.gz"
  tar xzf "mpv-$MPV_VERSION.tar.gz"
  echo "==> Applying $(basename "$patch_file")"
  ( cd "mpv-$MPV_VERSION" && patch -p1 < "$patch_file" )
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
