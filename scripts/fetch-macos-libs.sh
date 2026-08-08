#!/usr/bin/env bash
# Downloads the prebuilt macOS native-library set into src-tauri/{lib,ffmpeg-macos}.
#
# The macOS counterpart of `fetch-libs.ps1`, and the reason it exists: building
# FFmpeg and mpv from source takes twenty minutes and used to sit on the critical
# path of every release, behind an `actions/cache` that GitHub evicts after seven
# days without a hit. Any project releasing less often than weekly paid that
# build every time. The set is now produced once by the `macOS libs` workflow and
# read from here — the same shape Windows has always had.
#
#   scripts/fetch-macos-libs.sh          # fetch if missing or stale
#   FP_LIBS_URL=… scripts/fetch-macos-libs.sh
#
# To build it yourself instead — which is what you want when changing the mpv
# patch or a pinned version — run `scripts/build-macos-libs.sh` and then publish
# through the workflow.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
lib_dir="$repo_root/src-tauri/lib"
ffmpeg_dir="$repo_root/src-tauri/ffmpeg-macos"
# The bucket's public domain. Public information — it is served to anyone who
# builds this project — so it lives here rather than in a secret, and the
# override exists for testing against another bucket.
base_url="${FP_LIBS_URL:-https://libs.frameplayer.app}"
base_url="${base_url%/}"

name="$("$repo_root/scripts/macos-libs-key.sh")"
# What the tree currently holds, if anything. Written after a successful
# extract, so a half-finished download cannot masquerade as a complete set.
stamp="$lib_dir/.libs-key"

if [ -f "$stamp" ] && [ "$(cat "$stamp")" = "$name" ] && [ -f "$lib_dir/libmpv.dylib" ]; then
  echo "macOS libs already at $name"
  exit 0
fi

echo "==> Fetching $name"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

if ! curl -fsSL --retry 3 --retry-delay 2 -o "$work/libs.tar.gz" "$base_url/$name.tar.gz"; then
  cat >&2 <<EOF

Could not fetch $base_url/$name.tar.gz

That name is a hash of the build scripts and the mpv patch in *this* checkout,
so a 404 means one of two things and they need different fixes:

  * this checkout changed how the libraries are built, and no set has been
    published for it yet — run the "macOS libs" workflow (Actions → macOS libs →
    Run workflow), which builds and publishes one; or
  * the set exists but is not where this script is looking — check
    R2_PUBLIC_URL_LIBS and that the bucket has a public domain attached.

Or build it locally: scripts/build-macos-libs.sh
EOF
  exit 1
fi

# The checksum is published beside the set. Verified rather than trusted: this
# is a binary that ends up inside a signed application, fetched over a link
# anybody could stand in front of.
if curl -fsSL --retry 3 -o "$work/libs.sha256" "$base_url/$name.tar.gz.sha256"; then
  want="$(awk '{print $1}' "$work/libs.sha256")"
  got="$(shasum -a 256 "$work/libs.tar.gz" | awk '{print $1}')"
  if [ "$want" != "$got" ]; then
    echo "checksum mismatch for $name.tar.gz" >&2
    echo "  published: $want" >&2
    echo "  fetched:   $got" >&2
    exit 1
  fi
  echo "  checksum ok"
else
  # Refused rather than shrugged at: a missing checksum means the publish was
  # incomplete, and this archive is about to be signed and shipped.
  echo "no $name.tar.gz.sha256 beside the archive — refusing to use it" >&2
  exit 1
fi

echo "==> Extracting"
# Both directories are replaced wholesale. Unpacking over a previous set would
# leave its extra dylibs behind — which for a GPL set replaced by an LGPL one
# means libx264 quietly surviving into the bundle.
rm -rf "$lib_dir" "$ffmpeg_dir"
mkdir -p "$repo_root/src-tauri"
tar xzf "$work/libs.tar.gz" -C "$repo_root/src-tauri"
echo "$name" > "$stamp"

echo
"$repo_root/scripts/check-macos-licenses.sh"
echo
echo "Done: $lib_dir ($(ls -1 "$lib_dir"/*.dylib | wc -l | tr -d ' ') libraries, $(du -sh "$lib_dir" | cut -f1))"
