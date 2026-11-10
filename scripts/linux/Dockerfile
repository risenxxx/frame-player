# The Linux build environment `scripts/linux/check.sh` runs the crate's clippy
# and tests in, from a Mac or anywhere else with Docker. Ubuntu 24.04 because it
# is what the .deb's `depends` names (libavcodec60, libmpv2, …) and what the CI
# job's `ubuntu-24.04` runner is. The same packages as that job, plus what a
# headless run of the app needs (Xvfb, Mesa's software GL, xdotool to drive the
# window, ImageMagick to grab and compare frames) — one image, so the check and
# the app run share a target directory.
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential pkg-config curl ca-certificates git rsync file \
      clang libclang-dev \
      libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libxdo-dev libdbus-1-dev \
      libmpv-dev \
      libavcodec-dev libavformat-dev libavutil-dev libswscale-dev \
      libswresample-dev libavfilter-dev libavdevice-dev \
      xvfb xauth dbus-x11 libgl1-mesa-dri libegl1 mesa-utils xdotool imagemagick \
    && rm -rf /var/lib/apt/lists/*

# The toolchain `.github/workflows/rust.yml` pins, passed in by check.sh so the
# two cannot disagree about a lint.
ARG RUST_TOOLCHAIN
RUN test -n "$RUST_TOOLCHAIN" \
    && curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal \
         --default-toolchain "$RUST_TOOLCHAIN" --component clippy
ENV PATH=/root/.cargo/bin:$PATH
