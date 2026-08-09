#!/usr/bin/env bash
# The macOS bundle build — `npm run tauri:macos:build`.
#
# A wrapper rather than a line in package.json because one decision has to be
# made before the build starts: **the hardened runtime is only turned on when
# there is a real certificate to pair it with.**
#
# The two are not independent. The hardened runtime enables library validation,
# which requires every dylib the process loads to carry the same Team ID as the
# process itself — and the ~35 libraries in Resources are loaded by the app
# binary at launch. With a Developer ID that is satisfied for free, because
# `macos-sign.sh` signs them with the same identity. With the ad-hoc fallback it
# cannot be satisfied at all: ad-hoc code has no Team ID, so dyld refuses the
# first library and the app dies before `main`, with
#
#   Library not loaded: @rpath/libavutil.60.dylib
#   … not valid for use in process: mapping process and mapped file
#     (non-platform) have different Team IDs
#
# Measured on this project, macOS 26.5. Worth knowing that it does not look like
# a signing problem from the outside: the bundle verifies, `codesign --verify
# --deep --strict` passes silently, and the failure is a crash dialog offering to
# report to Apple. Note also that a probe which `dlopen`s the same libraries
# survives this, so testing the libraries in isolation proves nothing about the
# app — the check that matters is launching the bundle.
#
# So: no identity means no hardened runtime, which is exactly the build the
# project shipped for its first thirty releases and which anyone can produce
# from a clean checkout with no Apple account. The entitlements file stays in
# the config either way; without the hardened runtime it is inert.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

identity="${APPLE_SIGNING_IDENTITY:-}"
args=()

if [ -z "$identity" ] || [ "$identity" = "-" ]; then
  # `-` is codesign's ad-hoc identity. Set explicitly rather than left to the
  # config so there is exactly one place the identity comes from — an empty
  # value would leave the bundle carrying only the linker's signature, which
  # Gatekeeper refuses down the "file is damaged" path with no way out in the
  # interface.
  identity="-"
  echo "==> No Developer ID — ad-hoc signature, hardened runtime off"
  # **Both** keys, and clearing `entitlements` is the load-bearing half.
  # `hardenedRuntime: false` on its own does not do it: measured on Tauri CLI
  # 2.11.4, a config carrying an entitlements file is signed `--options runtime`
  # regardless of the flag, so the bundle came out `flags=0x10002(adhoc,runtime)`
  # and died at launch on library validation. Verified it is the bundler and not
  # codesign — `codesign --entitlements … --sign -` with no `--options runtime`
  # gives a plain `0x2(adhoc)`.
  args+=(--config '{"bundle":{"macOS":{"hardenedRuntime":false,"entitlements":null}}}')
else
  # Nothing to override — tauri.macos.conf.json is written for this case, so a
  # release build is the plain one and the workaround is the exception.
  echo "==> Signing as: $identity (hardened runtime on)"
fi

export APPLE_SIGNING_IDENTITY="$identity"
export FFMPEG_DIR="$repo_root/src-tauri/ffmpeg-macos"
export LIBCLANG_PATH=/Library/Developer/CommandLineTools/usr/lib

# Split rather than one `"${args[@]}"`: macOS ships bash **3.2**, where `set -u`
# treats expanding an *empty* array as an unbound variable and kills the script
# ("args[@]: unbound variable"). Fixed in bash 4.4, which macOS will never have —
# it is on the old licence. The failure is worth knowing because of its shape:
# the build simply does not run, and every check afterwards then reads the
# artefacts of the previous build and reports on those.
if [ ${#args[@]} -gt 0 ]; then
  exec npx tauri build "${args[@]}" "$@"
else
  exec npx tauri build "$@"
fi
