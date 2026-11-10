#!/usr/bin/env bash
# The Linux half of the Rust checks, from any machine with Docker (OrbStack on a
# Mac): `cargo clippy --all-targets --locked -- -D warnings` and `cargo test
# --locked` inside Ubuntu 24.04, with the toolchain `.github/workflows/rust.yml`
# pins. The same two commands as the CI job, so a red CI is reproducible here.
#
#   bun run linux:check            # clippy + tests
#   scripts/linux/check.sh shell   # a shell in the same container, tree synced
#
# The checkout is mounted read-only and copied into the container (see
# in-container.sh): the build writes into src-tauri (the ffmpeg/tools links,
# build.rs's copies), and a Linux build must not leave those in a Mac's tree.
# The cargo registry and the target directory are named volumes, so a second
# run rebuilds only what changed. `docker volume rm fp-linux-target
# fp-linux-cargo` starts from nothing.
#
# The image is built for the host's architecture — arm64 on Apple silicon, which
# CI does not cover, so the two together check both. FP_LINUX_PLATFORM=linux/amd64
# forces x86-64 (emulated, and much slower).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
toolchain="$(sed -n "s/^ *RUST_TOOLCHAIN: *'\([^']*\)'.*/\1/p" "$repo_root/.github/workflows/rust.yml" | head -1)"
if [ -z "$toolchain" ]; then
  echo "linux:check: no RUST_TOOLCHAIN in .github/workflows/rust.yml" >&2
  exit 1
fi

platform_args=()
if [ -n "${FP_LINUX_PLATFORM:-}" ]; then
  platform_args=(--platform "$FP_LINUX_PLATFORM")
fi
image="frameplayer-linux:$toolchain"

echo "==> image $image"
docker build -q ${platform_args[@]+"${platform_args[@]}"} \
  --build-arg RUST_TOOLCHAIN="$toolchain" \
  -t "$image" "$repo_root/scripts/linux" >/dev/null

tty_args=()
[ -t 0 ] && [ -t 1 ] && tty_args=(-it)

docker run --rm ${tty_args[@]+"${tty_args[@]}"} ${platform_args[@]+"${platform_args[@]}"} \
  -v "$repo_root:/src:ro" \
  -v fp-linux-cargo:/root/.cargo/registry \
  -v fp-linux-target:/cache/target \
  -e CARGO_TERM_COLOR="${CARGO_TERM_COLOR:-always}" \
  "$image" bash /src/scripts/linux/in-container.sh "${1:-check}"
