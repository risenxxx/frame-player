#!/usr/bin/env bash
# The Linux bundles: the .deb and the AppImage, with the .deb's `Depends` taken
# from this machine (deb-depends.sh says why a fixed list cannot be).
#
# Two steps, because the dependencies are read off the built binary: `tauri
# build --no-bundle`, then `tauri bundle` with the list as extra configuration.
# `tauri.linux.conf.json` refuses to bundle without it (`beforeBundleCommand`),
# so a plain `bun run tauri build` cannot ship a .deb with the wrong names.
#
# Without TAURI_SIGNING_PRIVATE_KEY the updater artifacts are left out, so a
# local build ends in success rather than in the signing error after both
# bundles were written.
#
#   bun run tauri:linux:build                   # both bundles, release
#   bun run tauri:linux:build --bundles deb     # one of them
#   bun run tauri:linux:build --debug
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

profile=release
bundle_args=()
while [ $# -gt 0 ]; do
  case "$1" in
    --debug) profile=debug; bundle_args+=("$1") ;;
    --bundles|-b) bundle_args+=("$1" "$2"); shift ;;
    *) echo "build.sh: unknown argument $1 (takes --debug and --bundles)" >&2; exit 2 ;;
  esac
  shift
done
build_args=()
[ "$profile" = debug ] && build_args+=(--debug)

bun run tauri build --no-bundle "${build_args[@]}"

binary="src-tauri/target/$profile/frameplayer"
depends=$(scripts/linux/deb-depends.sh "$binary")
updater=true
[ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ] || updater=false

config="src-tauri/target/$profile/deb-depends.conf.json"
printf '{"bundle":{"createUpdaterArtifacts":%s,"linux":{"deb":{"depends":%s}}}}\n' "$updater" "$depends" >"$config"
echo "deb Depends: $depends"

FP_DEB_DEPENDS=1 bun run tauri bundle --config "$config" "${bundle_args[@]}"
