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
# Run: scripts/bundle-macos-libs.sh   (after scripts/build-libmpv-macos.sh)
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
lib_dir="$repo_root/src-tauri/lib"
# A separate directory for linking ffmpeg-sys: symlinks under unversioned names
# only (the linker looks for libavcodec.dylib); the files themselves stay in lib_dir.
ffmpeg_dir="$repo_root/src-tauri/ffmpeg-macos"
# /opt/homebrew on Apple Silicon, /usr/local on Intel.
brew_prefix="$(brew --prefix 2>/dev/null || echo /opt/homebrew)"
ffmpeg_libs="avutil avcodec avformat avdevice avfilter swscale swresample"

for req in libmpv.dylib libmpv-wrapper.dylib; do
  if [ ! -f "$lib_dir/$req" ]; then
    echo "missing $lib_dir/$req — run scripts/build-libmpv-macos.sh first" >&2
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

echo "==> Transitive closure"
seen="$(mktemp)"; queue="$(mktemp)"
trap 'rm -f "$seen" "$queue"' EXIT
for root in "$lib_dir"/*.dylib; do deps_of "$root" >> "$queue"; done

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
  # @loader_path is the library's own directory, so the set stays relocatable.
  install_name_tool -add_rpath "@loader_path" "$f" 2>/dev/null || true
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
  if [ -d "$brew_prefix/include/$h" ]; then
    # -L is required: Homebrew's include holds relative symlinks into Cellar,
    # and without dereferencing them the links themselves get copied — broken
    # the moment they land anywhere else.
    cp -RL "$brew_prefix/include/$h" "$ffmpeg_dir/include/"
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

count=$(ls -1 "$lib_dir"/*.dylib | wc -l | tr -d ' ')
size=$(du -sh "$lib_dir" | cut -f1)
echo
echo "Done: $lib_dir ($count libraries, $size)"
echo "ffmpeg linking: FFMPEG_DIR=$ffmpeg_dir"
