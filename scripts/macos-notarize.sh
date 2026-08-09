#!/usr/bin/env bash
# Notarises one artefact and staples the ticket to it.
#
#   scripts/macos-notarize.sh "src-tauri/target/release/bundle/macos/Frame Player.app"
#   scripts/macos-notarize.sh "src-tauri/target/release/bundle/dmg/Frame Player_0.33.0_aarch64.dmg"
#
# Deliberately ours rather than Tauri's. The bundler will notarise the .app by
# itself when the credentials are in the environment, but it cannot touch the
# disk image — that is built by `build-dmg.sh` in a step of its own, for reasons
# in its own header — so half the job would be Tauri's and half ours, in an
# order decided by the bundler's internals. One script, called twice, is the
# version whose order can be read off the workflow.
#
# It is also what makes the tail re-runnable without rebuilding: everything here
# operates on artefacts that already exist, so a notarisation that fails at
# 23:59 can be retried against the same .app.
#
# Credentials, either set (the API key is preferred — it does not expire with an
# Apple ID password and is not tied to one person's account):
#   APPLE_API_KEY       key id          APPLE_ID        the account e-mail
#   APPLE_API_ISSUER    issuer uuid     APPLE_PASSWORD  an app-specific password
#   APPLE_API_KEY_P8    the .p8 body    APPLE_TEAM_ID   the ten-character team
#     (or APPLE_API_KEY_PATH, a file)
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target="${1:-}"

if [ -z "$target" ] || [ ! -e "$target" ]; then
  echo "usage: $0 <path to .app or .dmg>" >&2
  exit 2
fi

identity="${APPLE_SIGNING_IDENTITY:-}"
if [ -z "$identity" ] || [ "$identity" = "-" ]; then
  # Notarisation is a statement about an identity. Without one there is nothing
  # to say, and an ad-hoc build is a legitimate thing to produce locally.
  echo "no Developer ID configured — skipping notarisation of $(basename "$target")"
  exit 0
fi

tmp="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
cleanup_files=()
cleanup() { [ ${#cleanup_files[@]} -eq 0 ] || rm -rf "${cleanup_files[@]}"; }
trap cleanup EXIT

# --- credentials -------------------------------------------------------------
auth=()
if [ -n "${APPLE_API_KEY:-}" ] && [ -n "${APPLE_API_ISSUER:-}" ]; then
  key_path="${APPLE_API_KEY_PATH:-}"
  if [ -z "$key_path" ] && [ -n "${APPLE_API_KEY_P8:-}" ]; then
    key_path="$tmp/AuthKey_${APPLE_API_KEY}.p8"
    # Written with no group or other access from the start rather than chmod'ed
    # afterwards: between the two there is a window, and this is a private key.
    (umask 077; printf '%s\n' "$APPLE_API_KEY_P8" > "$key_path")
    cleanup_files+=("$key_path")
  fi
  [ -n "$key_path" ] || { echo "APPLE_API_KEY is set but neither APPLE_API_KEY_P8 nor APPLE_API_KEY_PATH is" >&2; exit 1; }
  auth=(--key "$key_path" --key-id "$APPLE_API_KEY" --issuer "$APPLE_API_ISSUER")
elif [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then
  auth=(--apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID")
else
  echo "no notarisation credentials: set APPLE_API_KEY + APPLE_API_ISSUER + APPLE_API_KEY_P8," >&2
  echo "or APPLE_ID + APPLE_PASSWORD + APPLE_TEAM_ID" >&2
  exit 1
fi

# --- what gets submitted -----------------------------------------------------
# A disk image is submitted as it is; a bundle has to be archived first, and
# `ditto` rather than `zip` because it is the one that preserves the bundle
# structure and the extended attributes a signature lives in.
case "$target" in
  *.dmg)
    # The image itself is signed too, and only here: `build-dmg.sh` runs
    # dmgbuild and nothing else. Unsigned, notarisation refuses it before it
    # starts — and an image whose seal is broken cannot carry a ticket anyway.
    echo "==> Signing $(basename "$target")"
    codesign --force --timestamp --sign "$identity" "$target"
    upload="$target"
    ;;
  *.app)
    upload="$tmp/$(basename "$target").zip"
    cleanup_files+=("$upload")
    echo "==> Archiving $(basename "$target")"
    ditto -c -k --keepParent --sequesterRsrc "$target" "$upload"
    ;;
  *)
    echo "don't know how to notarise $target" >&2
    exit 2
    ;;
esac

# --- submit ------------------------------------------------------------------
echo "==> Notarising $(basename "$target") — this takes minutes, not seconds"
set +e
out="$(xcrun notarytool submit "$upload" "${auth[@]}" --wait --output-format json 2>&1)"
rc=$?
set -e
echo "$out"

# `jq` rather than a JSON parser of our own, and every read tolerant of the
# output not being JSON at all: notarytool prints a plain-text error for a bad
# credential, and a parse failure there would replace Apple's message with ours.
id="$(printf '%s' "$out" | jq -r '.id // empty' 2>/dev/null || true)"
status="$(printf '%s' "$out" | jq -r '.status // empty' 2>/dev/null || true)"

if [ "$status" != "Accepted" ] || [ $rc -ne 0 ]; then
  echo "notarisation did not succeed (status=${status:-unknown})" >&2
  # The submission log is the only place that names the offending file, and it
  # is the difference between "rejected" and "this dylib is signed ad-hoc".
  # Without it the next step is guesswork against a ten-minute round trip.
  if [ -n "$id" ]; then
    echo "--- notarytool log $id ---" >&2
    xcrun notarytool log "$id" "${auth[@]}" >&2 || true
  fi
  exit 1
fi

# --- staple ------------------------------------------------------------------
# The ticket is fetched from Apple and written into the artefact, so Gatekeeper
# can clear it on a machine that is offline or behind a captive portal. Without
# this the notarisation is real but has to be looked up over the network at
# first launch — which is exactly when a new user is least forgiving.
echo "==> Stapling"
xcrun stapler staple "$target"
xcrun stapler validate "$target"

# --- the updater archive -----------------------------------------------------
# Only for the .app, and not an optional extra. Tauri builds `.app.tar.gz`
# during the build, i.e. before the ticket existed, so the moment stapling
# succeeds that archive is a copy of the app *without* the ticket. Shipping it
# would notarise the disk image that first-time users download and leave every
# automatic update carrying an unstapled bundle — the failure that only shows up
# for existing users, weeks later.
case "$target" in
  *.app)
    dir="$(cd "$(dirname "$target")" && pwd)"
    name="$(basename "$target")"
    tgz="$dir/$name.tar.gz"
    echo "==> Repacking $name.tar.gz around the stapled bundle"
    # COPYFILE_DISABLE stops bsdtar from storing extended attributes as
    # AppleDouble `._` members, which is how Tauri's own archive is laid out
    # (measured: 50 entries, not one of them a `._` file).
    ( cd "$dir" && COPYFILE_DISABLE=1 tar czf "$tgz" "$name" )

    signer="$repo_root/node_modules/.bin/tauri"
    if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ] && [ -x "$signer" ]; then
      # The archive changed, so the signature beside it is stale. This is the
      # updater's own minisign key and has nothing to do with Apple's.
      "$signer" signer sign "$tgz" >/dev/null
      echo "  re-signed $name.tar.gz"
    else
      # Refuse to leave a matching-looking pair that does not match. The staging
      # step reads this .sig; without it the release fails here, loudly, instead
      # of shipping an update every client rejects.
      rm -f "$tgz.sig"
      echo "  ! no TAURI_SIGNING_PRIVATE_KEY — removed the stale $name.tar.gz.sig" >&2
    fi

    # The archive is what the updater installs, so the ticket has to have
    # survived the round trip through tar.
    check="$tmp/staple-check"
    rm -rf "$check"; mkdir -p "$check"
    cleanup_files+=("$check")
    tar xzf "$tgz" -C "$check"
    xcrun stapler validate "$check/$name"
    echo "  ticket survives the archive"
    ;;
esac

echo "Done: $(basename "$target") is notarised and stapled"
