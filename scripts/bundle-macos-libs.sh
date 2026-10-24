#!/usr/bin/env bash
# Makes the set of dylibs in src-tauri/lib/ self-contained.
#
# Homebrew's libmpv and FFmpeg reference each other by absolute path
# (/opt/homebrew/opt/...), so the resulting .app would only run on a machine
# with Homebrew and exactly the same formulae installed. This script collects
# the whole transitive closure into one flat directory, rewrites the paths to
# @rpath and gives every library an LC_RPATH of @loader_path — so the set
# resolves relative to its own directory wherever it ends up:
# target/<profile>/lib in dev, Contents/Resources/lib inside the bundle.
#
# Important: this runs BEFORE `tauri build`. Editing Mach-O headers after the
# bundle is built would break its signature and force a separate .dmg rebuild.
#
# Run: scripts/bundle-macos-libs.sh   (after scripts/build-macos-libs.sh)
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
lib_dir="$repo_root/src-tauri/lib"
# A separate directory for linking ffmpeg-sys: symlinks under unversioned names
# only (the linker looks for libavcodec.dylib); the files themselves stay in lib_dir.
ffmpeg_dir="$repo_root/src-tauri/ffmpeg-macos"
# /opt/homebrew on Apple Silicon, /usr/local on Intel.
brew_prefix="$(brew --prefix 2>/dev/null || echo /opt/homebrew)"
# Where the ffmpeg CLI and headers come from. Homebrew's by default, which is a
# **GPL** build; `build-macos-libs.sh` points this at its own LGPL prefix
# instead. Only these two need saying out loud — the dylibs arrive on their own,
# through the closure of whatever libmpv was linked against.
ffmpeg_prefix="${FFMPEG_PREFIX:-$brew_prefix}"
ffmpeg_libs="avutil avcodec avformat avdevice avfilter swscale swresample"

for req in libmpv.dylib libmpv-wrapper.dylib; do
  if [ ! -f "$lib_dir/$req" ]; then
    echo "missing $lib_dir/$req — run scripts/build-macos-libs.sh first" >&2
    exit 1
  fi
done

# Mach-O dependencies, minus the system ones, the already-relative ones
# (@rpath/...) and the library's own install name (otool -L prints that
# alongside the dependencies).
deps_of() {
  local f="$1" own
  own="$(otool -D "$f" | tail -n +2 | head -1 || true)"
  otool -L "$f" | tail -n +2 \
    | sed 's/ (compatibility.*//; s/^[[:space:]]*//' \
    | grep '^/' \
    | grep -vE '^/usr/lib|^/System' \
    | grep -vxF "${own:-/dev/null}" || true
}

# The dependencies `deps_of` leaves out because they are already relative —
# resolved the way dyld would, through the file's own run paths, but only
# those the set does not hold yet. A Homebrew library names its neighbours from
# the same formula this way: libbrotlidec asks for
# `@rpath/libbrotlicommon.1.dylib` and finds it through its own LC_RPATH, so
# when freetype started linking brotli the closure copied libbrotlidec (named
# by absolute path) and never saw libbrotlicommon — a set in which freetype,
# and through it libass and libmpv, could not load.
rel_deps_of() {
  local f="$1" dir own d name rp
  dir="$(dirname "$f")"
  own="$(otool -D "$f" | tail -n +2 | head -1 || true)"
  otool -L "$f" | tail -n +2 \
    | sed 's/ (compatibility.*//; s/^[[:space:]]*//' \
    | grep -E '^@(rpath|loader_path)/' \
    | grep -vxF "${own:-/dev/null}" \
    | while read -r d; do
        name="${d#*/}"
        [ -f "$lib_dir/$name" ] && continue
        case "$d" in
          @loader_path/*) [ -f "$dir/$name" ] && echo "$dir/$name" ;;
          @rpath/*)
            otool -l "$f" | awk '/cmd LC_RPATH/{getline;getline;print $2}' | while read -r rp; do
              case "$rp" in
                @loader_path*) rp="$dir${rp#@loader_path}" ;;
                @*) continue ;;
              esac
              if [ -f "$rp/$name" ]; then echo "$rp/$name"; break; fi
            done
            ;;
        esac
      done || true
}

echo "==> MoltenVK"
# Not part of the closure: the Vulkan loader finds the driver through a JSON
# manifest at runtime rather than through a Mach-O reference. Without it
# vo=gpu-next will not come up on a machine that has no Homebrew. The path in
# the manifest is relative — the loader resolves it against the manifest.
if [ -f "$brew_prefix/lib/libMoltenVK.dylib" ]; then
  cp -f "$brew_prefix/lib/libMoltenVK.dylib" "$lib_dir/libMoltenVK.dylib"
  chmod 644 "$lib_dir/libMoltenVK.dylib"
  cat > "$lib_dir/MoltenVK_icd.json" <<'JSON'
{
    "file_format_version": "1.0.0",
    "ICD": {
        "library_path": "./libMoltenVK.dylib",
        "api_version": "1.4.0",
        "is_portability_driver": true
    }
}
JSON
else
  echo "  ! no libMoltenVK.dylib — brew install molten-vk" >&2
fi

echo "==> ffmpeg CLI"
# The cast prepare rung (cast.rs) spawns the ffmpeg *binary* — for
# `-movflags +faststart`, `-progress` and kill-to-cancel, none of which the
# library route gives for free. Windows gets it from the BtbN SDK; here it is
# Homebrew's, which is the same 0.4 MB shim over the very dylibs this script
# is already bundling, so it costs the closure a few extra entries (libvmaf and
# friends) and nothing else. Same licensing footing as libmpv itself, whichever
# that is: `$FFMPEG_PREFIX` decides, and the two must agree — a GPL CLI beside
# LGPL libraries would make the bundle GPL through the back door.
cli_stage="$(mktemp -d)"
trap 'rm -rf "$cli_stage"' EXIT
if [ -x "$ffmpeg_prefix/bin/ffmpeg" ]; then
  cp -f "$ffmpeg_prefix/bin/ffmpeg" "$cli_stage/ffmpeg"
  chmod 755 "$cli_stage/ffmpeg"
else
  echo "  ! no $ffmpeg_prefix/bin/ffmpeg — brew install ffmpeg (cast prepare/HLS will not work)" >&2
fi

echo "==> Transitive closure"
seen="$(mktemp)"; queue="$(mktemp)"
trap 'rm -f "$seen" "$queue"; rm -rf "$cli_stage"' EXIT
for root in "$lib_dir"/*.dylib; do
  deps_of "$root" >> "$queue"
  rel_deps_of "$root" >> "$queue"
done
# The CLI is a root of the closure too: it pulls in libraries libmpv does not
# (libvmaf came in this way), and a missing one is a binary that dies at exec.
if [ -f "$cli_stage/ffmpeg" ]; then
  deps_of "$cli_stage/ffmpeg" >> "$queue"
  rel_deps_of "$cli_stage/ffmpeg" >> "$queue"
fi

while [ -s "$queue" ]; do
  dep="$(head -1 "$queue")"
  sed -i '' '1d' "$queue"
  [ -n "$dep" ] || continue
  grep -qxF "$dep" "$seen" 2>/dev/null && continue
  if [ ! -f "$dep" ]; then
    echo "  ! dependency not found: $dep" >&2
    continue
  fi
  echo "$dep" >> "$seen"
  deps_of "$dep" >> "$queue"
  rel_deps_of "$dep" >> "$queue"
done

echo "==> Copying ($(wc -l < "$seen" | tr -d ' ') found)"
# Flat directory, name = basename. The same brew file is visible through both
# opt/ and Cellar/, so a repeat copy just overwrites it with identical bytes.
while read -r src; do
  [ -n "$src" ] || continue
  dst="$lib_dir/$(basename "$src")"
  [ -f "$dst" ] && chmod 644 "$dst"
  cp -f "$src" "$dst"
  chmod 644 "$dst"   # brew ships 444, and tauri-build could not overwrite that
done < "$seen"

echo "==> Rewriting paths"
for f in "$lib_dir"/*.dylib; do
  base="$(basename "$f")"
  install_name_tool -id "@rpath/$base" "$f" 2>/dev/null || true
  # A dependency whose namesake sits next to us is ours: point it at @rpath.
  deps_of "$f" | while read -r d; do
    [ -f "$lib_dir/$(basename "$d")" ] || continue
    install_name_tool -change "$d" "@rpath/$(basename "$d")" "$f" 2>/dev/null || true
  done
  # Every run path the linker left goes, and @loader_path — the library's own
  # directory — is the only one put back, so the set stays relocatable AND is
  # the only place its @rpath names resolve. The build leaves its own: meson
  # writes one per dependency directory (libmpv carried ten, Homebrew Cellar
  # and the runner's temp prefix among them), and libjpeg and LuaJIT arrive
  # with theirs. dyld takes the first match in order, so with @loader_path
  # added *after* them, a machine with those formulae installed loaded
  # Homebrew's libplacebo, Vulkan loader, lcms2, libass, uchardet and libjpeg
  # instead of ours (measured with DYLD_PRINT_LIBRARIES). A signed release is
  # saved by library validation, which rejects the foreign copies and lets dyld
  # fall through; an ad-hoc build has no such guard. Relative ones go too:
  # LuaJIT's `@loader_path/../../../../lib` points outside the bundle.
  otool -l "$f" | awk '/cmd LC_RPATH/{getline;getline;print $2}' | while read -r rp; do
    install_name_tool -delete_rpath "$rp" "$f" 2>/dev/null || true
  done
  install_name_tool -add_rpath "@loader_path" "$f"
  # Editing Mach-O breaks the signature, and Apple Silicon will not load unsigned code.
  codesign --force --sign - "$f" 2>/dev/null || true
done

echo "==> ffmpeg linking directory"
# Headers are copied rather than symlinked into Homebrew, so that the pair
# lib/ + ffmpeg-macos/ is self-contained and can go into the CI cache whole,
# without reinstalling brew formulae on every run.
rm -rf "$ffmpeg_dir"
mkdir -p "$ffmpeg_dir/lib" "$ffmpeg_dir/include"
for h in libavutil libavcodec libavformat libavdevice libavfilter libswscale libswresample; do
  if [ -d "$ffmpeg_prefix/include/$h" ]; then
    # -L is required: Homebrew's include holds relative symlinks into Cellar,
    # and without dereferencing them the links themselves get copied — broken
    # the moment they land anywhere else.
    cp -RL "$ffmpeg_prefix/include/$h" "$ffmpeg_dir/include/"
  else
    echo "  ! no $h headers — bindgen will not build" >&2
  fi
done
for l in $ffmpeg_libs; do
  real="$(ls -1 "$lib_dir" | grep -E "^lib$l\.[0-9]+\.dylib$" | head -1 || true)"
  if [ -z "$real" ]; then
    echo "  ! no lib$l — ffmpeg-the-third will not link" >&2
    continue
  fi
  # Relative symlink: ffmpeg-macos/lib/../.. is src-tauri. An absolute one
  # would break when the cache is restored into a different directory.
  ln -s "../../lib/$real" "$ffmpeg_dir/lib/lib$l.dylib"
done

if [ -f "$cli_stage/ffmpeg" ]; then
  echo "==> Installing the ffmpeg CLI"
  mkdir -p "$ffmpeg_dir/bin"
  cp -f "$cli_stage/ffmpeg" "$ffmpeg_dir/bin/ffmpeg"
  chmod 755 "$ffmpeg_dir/bin/ffmpeg"
  deps_of "$ffmpeg_dir/bin/ffmpeg" | while read -r d; do
    [ -f "$lib_dir/$(basename "$d")" ] || continue
    install_name_tool -change "$d" "@rpath/$(basename "$d")" "$ffmpeg_dir/bin/ffmpeg" 2>/dev/null || true
  done
  # One rpath covers both layouts, which is why the CLI is placed where it is:
  # in dev build.rs copies it to target/<profile>/ beside target/<profile>/lib,
  # and in the bundle it is a resource at Contents/Resources/ next to
  # Contents/Resources/lib. `@executable_path` differs between the two, the
  # relative position does not. (The app binary needs two rpaths for the same
  # reason it lives in Contents/MacOS instead — see build.rs.)
  install_name_tool -add_rpath "@executable_path/lib" "$ffmpeg_dir/bin/ffmpeg" 2>/dev/null || true
  codesign --force --sign - "$ffmpeg_dir/bin/ffmpeg" 2>/dev/null || true
  left=$(otool -L "$ffmpeg_dir/bin/ffmpeg" | tail -n +2 | grep -c '^\s*/opt\|^\s*/usr/local' || true)
  if [ "$left" -gt 0 ]; then
    echo "  ! $left dependency(ies) still point at Homebrew — the bundle will not run elsewhere" >&2
  fi
fi

echo "==> Checking the set resolves within itself"
# Every library the set names must be in it, under the name it is asked for:
# with @loader_path as the only run path there is nowhere else for dyld to look.
# A gap does not fail any step before this one — the closure reports what it
# cannot find and goes on, and a name it never followed is not reported at all
# — and on a machine without Homebrew it is a libmpv that will not load.
unresolved=0
for f in "$lib_dir"/*.dylib "$ffmpeg_dir/bin/ffmpeg"; do
  [ -f "$f" ] || continue
  own="$(otool -D "$f" | tail -n +2 | head -1 || true)"
  while read -r d; do
    [ -n "$d" ] || continue
    [ "$d" = "$own" ] && continue
    case "$d" in
      /usr/lib/*|/System/*) continue ;;
      @rpath/*|@loader_path/*) [ -f "$lib_dir/${d#*/}" ] && continue ;;
    esac
    echo "  ! $(basename "$f") needs $d, which the set does not hold" >&2
    unresolved=$((unresolved + 1))
  done < <(otool -L "$f" | tail -n +2 | sed 's/ (compatibility.*//; s/^[[:space:]]*//')
done
if [ "$unresolved" -gt 0 ]; then
  echo "$unresolved unresolved dependency(ies) — the set would not load elsewhere" >&2
  exit 1
fi

count=$(ls -1 "$lib_dir"/*.dylib | wc -l | tr -d ' ')
size=$(du -sh "$lib_dir" | cut -f1)
echo
echo "Done: $lib_dir ($count libraries, $size)"
echo "ffmpeg linking: FFMPEG_DIR=$ffmpeg_dir"
