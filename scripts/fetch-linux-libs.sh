#!/usr/bin/env bash
# The Linux counterpart of `fetch-libs.ps1` and `fetch-macos-libs.sh` — and it
# downloads nothing. On Linux libmpv, FFmpeg, WebKitGTK and GTK are the
# distribution's packages: the player links the system's copies and the .deb
# depends on them (`tauri.linux.conf.json`). What this does instead:
#
#   1. Checks every library the crate links through pkg-config, and when one is
#      missing prints the one command that installs all of them (apt for
#      Debian/Ubuntu, dnf for Fedora) and exits 1.
#   2. Lays out src-tauri/ffmpeg/{include,lib} and src-tauri/tools/libclang.so as
#      links into the system's own directories.
#
# The second step exists because of `.cargo/config.toml`. Its `[env]` points
# FFMPEG_DIR and LIBCLANG_PATH at src-tauri/ffmpeg and src-tauri/tools for the
# Windows SDKs, and Cargo has no per-target `[env]`, so the same two variables
# reach a Linux build. ffmpeg-sys-the-third takes a set FFMPEG_DIR as the whole
# answer (it never falls through to pkg-config), and clang-sys searches only
# LIBCLANG_PATH when it is set. So instead of fighting the configuration, the
# two directories it names are made to exist here, pointing at what pkg-config
# and llvm-config report — and plain `cargo` (and rust-analyzer) then works with
# no environment of its own. Both directories are in .gitignore already.
#
#   scripts/fetch-linux-libs.sh          # check, then (re)make the links
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tauri_dir="$repo_root/src-tauri"

# pkg-config module → Debian/Ubuntu package → Fedora package.
#   FFmpeg: the libraries ffmpeg-the-third enables by default (codec, format,
#   util, swscale, swresample, filter, device). On Fedora the full FFmpeg is in
#   RPM Fusion; `ffmpeg-free-devel` from Fedora itself also builds.
#   dbus-1: the keyring's Secret Service backend (Cargo.toml).
modules=(
  "mpv              libmpv-dev               mpv-devel"
  "libavcodec       libavcodec-dev           ffmpeg-free-devel"
  "libavformat      libavformat-dev          ffmpeg-free-devel"
  "libavutil        libavutil-dev            ffmpeg-free-devel"
  "libswscale       libswscale-dev           ffmpeg-free-devel"
  "libswresample    libswresample-dev        ffmpeg-free-devel"
  "libavfilter      libavfilter-dev          ffmpeg-free-devel"
  "libavdevice      libavdevice-dev          ffmpeg-free-devel"
  "webkit2gtk-4.1   libwebkit2gtk-4.1-dev    webkit2gtk4.1-devel"
  "gtk+-3.0         libgtk-3-dev             gtk3-devel"
  "dbus-1           libdbus-1-dev            dbus-devel"
)

if ! command -v pkg-config >/dev/null 2>&1; then
  echo "pkg-config is not installed."
  echo "  Debian/Ubuntu: sudo apt install pkg-config"
  echo "  Fedora:        sudo dnf install pkgconf-pkg-config"
  exit 1
fi

missing_apt=()
missing_dnf=()
for row in "${modules[@]}"; do
  read -r module apt_pkg dnf_pkg <<<"$row"
  if version="$(pkg-config --modversion "$module" 2>/dev/null)"; then
    printf '  %-16s %s\n' "$module" "$version"
  else
    printf '  %-16s MISSING\n' "$module"
    missing_apt+=("$apt_pkg")
    missing_dnf+=("$dnf_pkg")
  fi
done

# bindgen (ffmpeg-sys) needs libclang; clang itself is the C compiler several
# build scripts want anyway.
libclang=""
if command -v llvm-config >/dev/null 2>&1; then
  libclang="$(find "$(llvm-config --libdir)" -maxdepth 1 \( -name 'libclang.so*' -o -name 'libclang-[0-9]*.so*' \) 2>/dev/null | sort | head -1 || true)"
fi
if [ -z "$libclang" ]; then
  # Not every directory exists on every distribution, and find says so in its
  # exit status — which pipefail would otherwise turn into this script's.
  libclang="$(find /usr/lib /usr/lib64 -maxdepth 4 \( -name 'libclang.so*' -o -name 'libclang-[0-9]*.so*' \) 2>/dev/null | sort -V | tail -1 || true)"
fi
if [ -n "$libclang" ]; then
  printf '  %-16s %s\n' "libclang" "$libclang"
else
  printf '  %-16s MISSING\n' "libclang"
  missing_apt+=("libclang-dev" "clang")
  missing_dnf+=("clang-devel")
fi

if [ ${#missing_apt[@]} -gt 0 ]; then
  # De-duplicated, in order (the FFmpeg rows share a Fedora package).
  dedupe() { printf '%s\n' "$@" | awk '!seen[$0]++' | tr '\n' ' '; }
  echo
  echo "Missing system packages. Install them with:"
  echo "  Debian/Ubuntu: sudo apt install build-essential $(dedupe "${missing_apt[@]}")"
  echo "  Fedora:        sudo dnf install $(dedupe "${missing_dnf[@]}")"
  echo "(and Tauri's own prerequisites: https://v2.tauri.app/start/prerequisites/#linux)"
  exit 1
fi

# FFmpeg as one prefix. Debian's multiarch layout puts the headers under
# /usr/include/<triplet> and Fedora's under /usr/include/ffmpeg; pkg-config
# knows either way, and FFMPEG_DIR only ever reads `include/` and `lib/` of it.
ffmpeg_dir="$tauri_dir/ffmpeg"
mkdir -p "$ffmpeg_dir"
ln -sfn "$(pkg-config --variable=includedir libavcodec)" "$ffmpeg_dir/include"
ln -sfn "$(pkg-config --variable=libdir libavcodec)" "$ffmpeg_dir/lib"

tools_dir="$tauri_dir/tools"
mkdir -p "$tools_dir"
ln -sfn "$libclang" "$tools_dir/libclang.so"

echo
echo "Linked src-tauri/ffmpeg/{include,lib} and src-tauri/tools/libclang.so to the system's."
