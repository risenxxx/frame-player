#!/usr/bin/env bash
# Packages the macOS library set in src-tauri/{lib,ffmpeg-macos} and uploads it.
#
#   R2_BUCKET=… R2_ENDPOINT=… scripts/publish-macos-libs.sh [--retain N]
#
# Called from two places — the manual `macOS libs` workflow and, when a release
# finds no published set for its own sources, the release job. It is a script
# rather than two copies of the same YAML because those copies would drift, and
# the shape of that drift is a release quietly publishing something the manual
# workflow would have rejected.
#
# **The license gate runs first, here, not only in the caller.** This is the last
# code between a library set and a public bucket.
#
# `--retain N` prunes older sets and is deliberately *not* the default: the
# release path must never prune. A release built from an old tag would otherwise
# publish its own set and delete the one the main branch needs, which the next
# release would rebuild and delete in turn — twenty minutes each, for ever.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

retain=0
while [ $# -gt 0 ]; do
  case "$1" in
    --retain) retain="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

: "${R2_BUCKET:?R2_BUCKET is not set}"
: "${R2_ENDPOINT:?R2_ENDPOINT is not set}"

name="$(scripts/macos-libs-key.sh)"

echo "==> License gate"
scripts/check-macos-licenses.sh

echo "==> Packaging $name"
tar czf "$name.tar.gz" -C src-tauri lib ffmpeg-macos
shasum -a 256 "$name.tar.gz" | tee "$name.tar.gz.sha256"

echo "==> Uploading"
# The archive first and the checksum second. `fetch-macos-libs.sh` refuses an
# archive with no checksum beside it, so this order means an interrupted upload
# leaves a set that is ignored rather than one that is trusted unverified.
aws s3 cp "$name.tar.gz"        "s3://$R2_BUCKET/$name.tar.gz"        --endpoint-url "$R2_ENDPOINT"
aws s3 cp "$name.tar.gz.sha256" "s3://$R2_BUCKET/$name.tar.gz.sha256" --endpoint-url "$R2_ENDPOINT"
echo "published: $name.tar.gz"

if [ "$retain" -gt 0 ]; then
  echo
  scripts/r2-retain.sh --kind libs --bucket "$R2_BUCKET" --endpoint "$R2_ENDPOINT" \
                       --keep "$retain" --apply
fi
