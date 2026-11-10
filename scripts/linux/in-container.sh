#!/usr/bin/env bash
# Runs inside the image check.sh builds, with the checkout read-only at /src.
# Copies what the crate needs to compile into /work, makes the system-library
# links (fetch-linux-libs.sh), and runs the checks — or opens a shell.
#
# What the crate reaches outside src-tauri, and so what is copied besides it:
#   - shared/: thumb_service.rs `include_str!`s ../../shared/path-under.txt
#   - LICENSE, THIRD-PARTY-NOTICES.md: bundle resources, which tauri-build checks
#   - .cargo/config.toml: its `[env]` is what a Linux build has to live with
#   - build/: frontendDist, which tauri's codegen wants to exist (empty is fine
#     for a check; `app` and the run modes embed the real one, so `bun run
#     build` on the host comes first)
set -euo pipefail

mode="${1:-check}"
mkdir -p /work /work/scripts
rsync -a --delete \
  --exclude /target --exclude /ffmpeg --exclude /ffmpeg-macos --exclude /tools \
  --exclude '/lib/*.dylib' --exclude '/lib/*.dll' --exclude '/lib/*.so' \
  /src/src-tauri/ /work/src-tauri/
rsync -a --delete /src/shared/ /work/shared/
rsync -a /src/.cargo/ /work/.cargo/
rsync -a /src/scripts/fetch-linux-libs.sh /src/scripts/linux/run-app.sh /work/scripts/
cp /src/LICENSE /src/THIRD-PARTY-NOTICES.md /work/
mkdir -p /work/build
if [ -f /src/build/index.html ]; then
  rsync -a --delete /src/build/ /work/build/
fi

bash /work/scripts/fetch-linux-libs.sh

cd /work/src-tauri
export CARGO_TARGET_DIR=/cache/target
# As in CI: nothing here runs under a debugger, and debug info is most of the
# target directory.
export CARGO_PROFILE_DEV_DEBUG=0

# The player itself, debug, with the frontend embedded the way `tauri build`
# embeds it (without `custom-protocol` a debug binary loads the dev server's
# URL instead).
build_app() {
  if [ ! -f /work/build/index.html ]; then
    echo "no frontend in build/ — run \`bun run build\` on the host first" >&2
    exit 1
  fi
  echo "==> build the app"
  cargo build -q --locked --features tauri/custom-protocol
}

case "$mode" in
  shell)
    exec bash
    ;;
  check)
    echo "==> $(rustc --version), $(uname -m)"
    echo "==> clippy"
    cargo clippy -q --all-targets --locked -- -D warnings
    echo "==> test"
    # The whole log is kept in the target volume; what is printed is the
    # verdict lines, and the verdict itself is cargo's exit status.
    status=0
    cargo test -q --locked >/cache/target/test.log 2>&1 || status=$?
    grep -E '^(test result|failures:|---- )|panicked|^error' /cache/target/test.log || true
    if [ "$status" -ne 0 ]; then
      echo "==> tests failed (full log: /cache/target/test.log in the fp-linux-target volume)"
      exit "$status"
    fi
    echo "==> Linux check passed"
    ;;
  app)
    build_app
    ;;
  run-x11 | run-wayland)
    build_app
    exec bash /work/scripts/run-app.sh "${mode#run-}"
    ;;
  *)
    echo "unknown mode: $mode (check | shell | app | run-x11 | run-wayland)" >&2
    exit 2
    ;;
esac
