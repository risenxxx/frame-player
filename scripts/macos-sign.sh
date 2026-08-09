#!/usr/bin/env bash
# Signs the native libraries with the Developer ID, and — in CI — puts the
# certificate somewhere `codesign` can find it.
#
# Runs BEFORE `tauri build`, and that ordering is the whole design. Notarisation
# inspects every Mach-O file in the bundle, and the ~35 dylibs plus the ffmpeg
# CLI are not Tauri's to sign: they arrive as `bundle.resources`, i.e. they are
# copied in, carrying whatever signature they already had — which is the ad-hoc
# one `bundle-macos-libs.sh` applies after rewriting their load commands. An
# ad-hoc Mach-O inside the bundle fails notarisation on identity, and no
# entitlement makes that go away.
#
# Signing them here means Tauri copies files that are already right and seals
# the bundle over them, last. The alternative — re-signing inside the built
# .app — would break the outer seal and force the disk image and the updater
# archive to be rebuilt from an app whose signature had just been replaced. Same
# reason `bundle-macos-libs.sh` insists on running before the build.
#
#   scripts/macos-sign.sh          # no identity configured -> does nothing
#   APPLE_SIGNING_IDENTITY=… scripts/macos-sign.sh
#
# In CI, APPLE_CERTIFICATE (base64 .p12) and APPLE_CERTIFICATE_PASSWORD are also
# set, and the keychain built here stays in the search list for the rest of the
# job — which is what lets the `tauri build` step afterwards find the identity
# without knowing any of this happened.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
lib_dir="$repo_root/src-tauri/lib"
ffmpeg_cli="$repo_root/src-tauri/ffmpeg-macos/bin/ffmpeg"

identity="${APPLE_SIGNING_IDENTITY:-}"

# `-` is the ad-hoc identity, which is what a local build without a certificate
# uses and what `bundle-macos-libs.sh` has already applied. Nothing to do, and
# saying so beats a silent no-op when this is called from a workflow.
if [ -z "$identity" ] || [ "$identity" = "-" ]; then
  echo "no Developer ID configured — leaving the ad-hoc signatures alone"
  # Said out loud where a release is being watched, because everything
  # downstream succeeds quietly: the notarisation steps skip themselves, the
  # artefacts are produced, and the only symptom is users meeting Gatekeeper
  # again — days later, in the release notes' words rather than the build's.
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    echo "⚠️ **APPLE_SIGNING_IDENTITY is not set** — this build is ad-hoc signed and not notarised." \
      >> "$GITHUB_STEP_SUMMARY"
  fi
  exit 0
fi

# --- the certificate ---------------------------------------------------------
# Only on a machine that was handed one. A developer with the certificate in
# their login keychain sets APPLE_SIGNING_IDENTITY and nothing else.
if [ -n "${APPLE_CERTIFICATE:-}" ]; then
  : "${APPLE_CERTIFICATE_PASSWORD:?APPLE_CERTIFICATE is set but APPLE_CERTIFICATE_PASSWORD is not}"

  tmp="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
  keychain="$tmp/frameplayer-signing.keychain-db"
  # A password for a keychain that lives for the length of one job on a throwaway
  # machine. It protects nothing; it exists because `create-keychain` demands one.
  keychain_pw="$(uuidgen)"

  echo "==> Importing the signing certificate"
  security create-keychain -p "$keychain_pw" "$keychain"
  # Without this the keychain re-locks itself after five minutes of idling and
  # the build step signs against a locked keychain, which fails as "User
  # interaction is not allowed" — a message about a UI on a machine with no UI.
  security set-keychain-settings -lut 21600 "$keychain"
  security unlock-keychain -p "$keychain_pw" "$keychain"

  cert="$tmp/certificate.p12"
  # `security import` wants a file. Written to the runner's temp dir, which is
  # discarded with the machine, and removed on the way out regardless.
  trap 'rm -f "$cert"' EXIT
  echo "$APPLE_CERTIFICATE" | base64 --decode > "$cert"

  security import "$cert" -k "$keychain" -P "$APPLE_CERTIFICATE_PASSWORD" \
    -T /usr/bin/codesign -T /usr/bin/security
  # `-T` alone is not enough on modern macOS: the key's ACL still prompts for
  # confirmation the first time codesign reaches for it, and there is nobody to
  # confirm. This grants it up front.
  security set-key-partition-list -S apple-tool:,apple:,codesign: \
    -s -k "$keychain_pw" "$keychain" >/dev/null

  # Prepended rather than replacing the list: `codesign` searches the default
  # keychains, and dropping the login keychain from it has broken unrelated
  # tooling on runners before.
  security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')
  security default-keychain -s "$keychain"
fi

# A Developer ID certificate is signed by an intermediate authority, and a .p12
# exported from Keychain Access usually does not carry it. Without it the chain
# reaches no root, the certificate is untrusted, and `find-identity -v` — which
# lists *valid* identities only — reports nothing at all, while the certificate
# is plainly there. The G2 intermediate is recent enough that a machine may
# simply not have it (measured: a developer's own Mac did not, and the
# certificate showed as "not trusted" in Keychain Access with no other symptom).
#
# So it is fetched on demand rather than assumed, and only when something is
# actually wrong — a runner with Xcode has it already.
if ! security find-identity -v -p codesigning | grep -qF "$identity"; then
  echo "==> Identity not valid yet — installing the Developer ID intermediate"
  intermediate="${tmp:-${TMPDIR:-/tmp}}/DeveloperIDG2CA.cer"
  if curl -fsSL --retry 3 -o "$intermediate" \
       https://www.apple.com/certificateauthority/DeveloperIDG2CA.cer; then
    security add-certificates -k "${keychain:-$HOME/Library/Keychains/login.keychain-db}" \
      "$intermediate" 2>/dev/null || security add-certificates "$intermediate" || true
  else
    echo "  ! could not fetch the intermediate from apple.com" >&2
  fi
fi

# Fail here rather than 35 codesign calls later, with the list of what *is*
# available — a mistyped identity and an unimported certificate look identical
# from the error `codesign` gives.
if ! security find-identity -v -p codesigning | grep -qF "$identity"; then
  echo "no such signing identity: $identity" >&2
  echo "available:" >&2
  security find-identity -v -p codesigning >&2
  exit 1
fi

# --- the libraries -----------------------------------------------------------
# `--timestamp` contacts Apple's timestamp server, so this step needs the
# network; without it the signature expires with the certificate instead of
# outliving it, and notarisation refuses it outright.
#
# `--options runtime` on a library is not what turns the hardened runtime on —
# that is a property of the process, taken from the main executable — but
# notarisation checks for it on every Mach-O, and a library without it is a
# rejection with a path and no explanation.
#
# No entitlements here: the JIT entitlement belongs to the process that runs the
# code, which is the app binary (and Tauri applies it from the config). The
# ffmpeg CLI is its own process and needs nothing.
sign() {
  codesign --force --timestamp --options runtime --sign "$identity" "$1"
}

count=0
echo "==> Signing native libraries as: $identity"
for f in "$lib_dir"/*.dylib; do
  [ -f "$f" ] || continue
  sign "$f"
  count=$((count + 1))
done

if [ -x "$ffmpeg_cli" ]; then
  sign "$ffmpeg_cli"
  count=$((count + 1))
else
  # Not fatal — a set built without the CLI still plays video, and the cast
  # prepare rung is the only thing that misses it — but it is never intentional.
  echo "  ! no $ffmpeg_cli to sign" >&2
fi

# Proof rather than assumption: `codesign --verify` on one of them catches a
# signature that was written but is not valid, which the signing call itself
# reports as success.
sample="$(ls -1 "$lib_dir"/*.dylib 2>/dev/null | head -1 || true)"
if [ -n "$sample" ]; then
  codesign --verify --strict "$sample"
  team="$(codesign -dvv "$sample" 2>&1 | sed -n 's/^TeamIdentifier=//p')"
  echo "  verified $(basename "$sample") — team $team"
fi

echo "Done: $count Mach-O files signed"
