#!/usr/bin/env bash
# Prints the name of the macOS native-library set this checkout needs.
#
#   macos-<arch>-<12 hex>      e.g. macos-arm64-36d4ceae3876
#
# The hash covers everything that decides what the libraries *are*, and nothing
# else: the two build scripts and the mpv patch. The pinned FFmpeg, mpv and
# wrapper versions live inside `build-macos-libs.sh`, so hashing that file covers
# a version bump as well — one input, nothing to keep in step by hand.
#
# **The license gate is deliberately not hashed.** A verifier cannot change the
# bytes it verifies, so including it meant that tightening the check republished
# an identical 21 MB set. Leaving it out is also the safer direction rather than
# the lax one: `fetch-macos-libs.sh` runs the *checkout's* gate against whatever
# it downloads, so a strengthened check applies to sets published before it
# existed — which it could not do if strengthening it invalidated them.
#
# **It exists as a file of its own because two places need the same answer**:
# the workflow that publishes a set, and the script that fetches one. Computed
# separately in each, they would agree right up until somebody edited one — and
# the symptom would be a 404 for an artifact that had just been published
# successfully, which reads as a broken bucket rather than as a broken hash.
#
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# Comments and blank lines are stripped from the scripts before hashing: a
# comment cannot change a byte of the output, and this project's scripts are
# more comment than code. Without the strip, rewording a note forced a
# twenty-minute rebuild and republished a byte-identical 21 MB set — which
# happened the first time these files were tidied.
#
# Only whole-line comments, never a trailing `#`: deciding whether a mid-line
# hash starts a comment or sits inside a string needs a shell parser, and being
# wrong would silently drop real code from the hash. Erring toward hashing too
# much costs a rebuild; erring the other way ships the wrong libraries.
#
# The patch is hashed verbatim — it is a diff, and its `#` lines are content.
# `|| true` on the grep: it returns 1 when it filters everything out, and under
# `pipefail` that would abort the script. It cannot happen with these files, but
# it is the exact idiom that has broken this pipeline twice already.
strip_comments() { sed -E 's/^[[:space:]]*#.*$//' "$@" | grep -v '^[[:space:]]*$' || true; }

key="$( { strip_comments scripts/build-macos-libs.sh \
                         scripts/bundle-macos-libs.sh
          cat patches/mpv-*.patch; } | shasum -a 256 | cut -c1-12)"

echo "macos-$(uname -m)-$key"
