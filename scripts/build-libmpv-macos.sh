#!/usr/bin/env bash
# Builds a patched libmpv for macOS: upstream mpv + the --wid (NSView) embedding
# patch that lets mpv render BEHIND the transparent WKWebView instead of opening
# its own window. See docs/macos.md.
#
# Output: src-tauri/lib/libmpv.dylib  (plus libmpv-wrapper.dylib, fetched too)
#
# Requires: Xcode Command Line Tools (swiftc), Homebrew.
set -euo pipefail

MPV_VERSION="0.41.0"
WRAPPER_VERSION="v0.1.1"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
lib_dir="$repo_root/src-tauri/lib"
patch_file="$repo_root/patches/mpv-$MPV_VERSION-macos-wid-embedding.patch"
work="${TMPDIR:-/tmp}/frameplayer-libmpv-$MPV_VERSION"

case "$(uname -m)" in
  arm64)  wrapper_arch="macos-aarch64" ;;
  x86_64) wrapper_arch="macos-x86_64" ;;
  *) echo "unsupported arch: $(uname -m)" >&2; exit 1 ;;
esac

echo "==> Build deps (Homebrew)"
# mpv's own runtime deps double as build deps: brew ships headers and .pc files.
brew install -q meson ninja pkgconf ffmpeg libplacebo libass luajit \
                vulkan-loader molten-vk little-cms2 libarchive uchardet

echo "==> Fetching mpv $MPV_VERSION"
mkdir -p "$work"
cd "$work"
if [ ! -d "mpv-$MPV_VERSION" ]; then
  curl -sSL -o "mpv-$MPV_VERSION.tar.gz" \
    "https://github.com/mpv-player/mpv/archive/refs/tags/v$MPV_VERSION.tar.gz"
  tar xzf "mpv-$MPV_VERSION.tar.gz"

  echo "==> Applying $(basename "$patch_file")"
  ( cd "mpv-$MPV_VERSION" && patch -p1 < "$patch_file" )
fi

echo "==> Configuring (libmpv only, no cplayer)"
cd "mpv-$MPV_VERSION"
# NOTE: this links against Homebrew's ffmpeg, which is a GPL build. For a
# redistributable player use an LGPL ffmpeg and add -Dgpl=false, mirroring the
# Windows side (see docs/macos.md, "Licensing").
if [ ! -d build ]; then
  meson setup build \
    -Dlibmpv=true \
    -Dcplayer=false \
    -Dvulkan=enabled \
    -Dlua=luajit \
    -Dbuildtype=release
fi

echo "==> Building"
ninja -C build

echo "==> Installing into $lib_dir"
mkdir -p "$lib_dir"
cp "build/libmpv.2.dylib" "$lib_dir/libmpv.dylib"
# Homebrew/meson artifacts can land read-only; tauri-build's resource copy then
# fails with a bare EACCES.
chmod 644 "$lib_dir/libmpv.dylib"

if [ ! -f "$lib_dir/libmpv-wrapper.dylib" ]; then
  echo "==> Fetching libmpv-wrapper $WRAPPER_VERSION ($wrapper_arch)"
  curl -sSL -o wrapper.zip \
    "https://github.com/nini22P/libmpv-wrapper/releases/download/$WRAPPER_VERSION/libmpv-wrapper-$wrapper_arch.zip"
  rm -rf wrapper && unzip -o -q wrapper.zip -d wrapper
  cp wrapper/bin/libmpv-wrapper.dylib "$lib_dir/"
  chmod 644 "$lib_dir/libmpv-wrapper.dylib"
fi

echo
echo "==> Making the dylib set self-contained"
# Pulls in the whole transitive closure and rewrites absolute Homebrew paths to
# @rpath, so the bundled .app runs on machines without Homebrew.
"$repo_root/scripts/bundle-macos-libs.sh"

cat <<'EOF'

The wrapper dlopen()s "libmpv.dylib" by bare leaf name, which is why libmpv must
sit next to it (dyld probes that directory via the wrapper's @loader_path rpath).
DYLD_LIBRARY_PATH does not work here: npm execs through SIP-protected /bin/sh,
which strips all DYLD_* variables.

Run with:
  npm run tauri:macos          # dev
  npm run tauri:macos:build    # .app + .dmg
EOF
