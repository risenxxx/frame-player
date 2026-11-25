#!/usr/bin/env bash
# Version 2's macOS library set: `scripts/build-macos-libs.sh --v2` runs this.
#
# Version 2 (src-skathi/) draws through mpv's render API with gpu-next on
# Vulkan, through MoltenVK, so its libmpv is not version 1's: it is mpv PR
# #18845 with src-skathi/patches/mpv-vulkan-render-api.diff on top, against a
# libplacebo newer than any release. Until this script it existed only as a
# developer build linked against Homebrew.
#
# **Every library in the set is built here from a pinned source, or is an
# official release artifact, and every download is checked against a SHA-256
# (a git commit, for the one source fetched with git).** Nothing is linked from
# Homebrew, which version 1's set still does for mpv's dependencies. Two things
# follow from that, besides reproducibility:
#
#   - pkg-config, CMake and meson are fenced into our own prefix
#     (PKG_CONFIG_LIBDIR, CMAKE_IGNORE_PREFIX_PATH, --wrap-mode=nodownload), so
#     an optional dependency the build machine happens to have cannot slip in.
#     A machine with Homebrew and one without build the same set.
#   - MACOSX_DEPLOYMENT_TARGET is the floor of every file, because no file is a
#     bottle built for the build machine's own macOS. The check at the end
#     still reads it out of each file rather than trusting the variable.
#
# The licences are version 1's: FFmpeg without its GPL opt-ins, mpv with
# -Dgpl=false; what this adds (Vulkan headers, loader and MoltenVK, glslang)
# is Apache-2.0 / BSD / MIT, except glslang's Bison-generated parser, which is
# GPL-3.0-or-later WITH Bison-exception-2.2 (no obligation on what links it;
# src-skathi/THIRD-PARTY-NOTICES.md says why). The checks at the end refuse a
# set that is not.
#
# Output (both ignored by git):
#   src-skathi/lib/            libmpv.dylib and its closure, libvulkan.1.dylib,
#                              libMoltenVK.dylib + MoltenVK_icd.json. Every
#                              library is @rpath/<name> with one run path,
#                              @loader_path, so the set loads from wherever it
#                              is put (MPV_LIB_DIR, Contents/Frameworks).
#   src-skathi/ffmpeg-macos/   the FFmpeg SDK the thumbnail decoder links
#                              (FFMPEG_DIR for src-skathi/scripts/build-macos.sh)
#                              and the ffmpeg CLI.
#
# Build tools are the machine's (meson, ninja, cmake, pkg-config, python3, git,
# Xcode's clang); in CI they come from Homebrew, and that is all Homebrew is
# used for. FP_V2_WORK moves the build tree (default: under $TMPDIR, kept
# between runs; each component is skipped once its stamp is there).
set -euo pipefail

# ---------------------------------------------------------------------------
# Pins. Changing any line here changes what the set is.
MPV_COMMIT="aade20e8cd7cbe43e39f4ffc4e00da8e044119c1"   # PR #18845 head, v0.42.0-110
MPV_SHA256="a2b4f4646904915b1fb2e1a6d9409a2349cfd69717956a099a514cfc56dd8003"
# A commit, not a release: the overlay target needs `pl_render_image(NULL, …)`
# behaviour and the shaders of 7.398, which no tag carries yet. Fetched with git
# because its build needs four submodules; the commit hash is the checksum.
LIBPLACEBO_COMMIT="f2e04cb1840ef1f1586b71050bff40b04cbd8d91" # v7.387.0-132, API 398
FFMPEG_VERSION="8.1.2"   # version 1's, same flags
FFMPEG_SHA256="464beb5e7bf0c311e68b45ae2f04e9cc2af88851abb4082231742a74d97b524c"
VULKAN_SDK="1.4.357.0"   # Vulkan-Headers, Vulkan-Loader and glslang move together
VULKAN_HEADERS_SHA256="e87dce08116151f6b6d7de6b6faf41498e87e6cf848ff16fa3bd5402190ad4a3"
VULKAN_LOADER_SHA256="54f2537df22313768da0317dda2abdaaab7711b4081c48c869a79db343d0ae70"
GLSLANG_SHA256="81038794e20494556edbcc0fc70fa984d71d1b440f9c49adf2cbaaa60a519757"
# KhronosGroup's own release build. Building MoltenVK from source means building
# SPIRV-Cross, SPIRV-Tools and glslang a second time through Xcode, for the
# same bytes upstream publishes.
MOLTENVK_VERSION="1.4.2"
MOLTENVK_SHA256="f95765a6229cb7b915990a2890ce12ebe36a730b021545d3d52ae69ce4c4024e"
LCMS2_VERSION="2.18"
LCMS2_SHA256="ee67be3566f459362c1ee094fde2c159d33fa0390aa4ed5f5af676f9e5004347"
DAV1D_VERSION="1.5.3"
DAV1D_SHA256="732010aa5ef461fa93355ed2c6c5fedb48ddc4b74e697eaabe8907eaeb943011"
FREETYPE_VERSION="2.14.3"
FREETYPE_SHA256="36bc4f1cc413335368ee656c42afca65c5a3987e8768cc28cf11ba775e785a5f"
FRIBIDI_VERSION="1.0.16"
FRIBIDI_SHA256="1b1cde5b235d40479e91be2f0e88a309e3214c8ab470ec8a2744d82a5a9ea05c"
HARFBUZZ_VERSION="14.4.0"
HARFBUZZ_SHA256="2357ed966c6ced7bfa720b0640c0231065af01158fbea215093ffa15aed44371"
UNIBREAK_VERSION="7.0"
UNIBREAK_SHA256="8c9a6e121736cd0d5c890ae3ae96f3f4010a19aa040f1dbded833a62a87717d3"
LIBASS_VERSION="0.17.5"
LIBASS_SHA256="2dca25c0e0c837ddf00b52011b3f82cac1e4ddd3ad018227806b0c2288864acc"
UCHARDET_VERSION="0.0.8"
UCHARDET_SHA256="e97a60cfc00a1c147a674b097bb1422abd9fa78a2d9ce3f3fdcc2e78a34ac5f0"
# LuaJIT has no releases, only a rolling v2.1 branch; this is the commit
# Homebrew's 2.1.1785192264 is, as a GitHub archive (its .relver is filled in by
# export-subst, so the version needs no .git).
LUAJIT_COMMIT="96f7b297326e9fd454b7186c6858b2773f94a0dc"
LUAJIT_SHA256="ce9d03b33e4c28492e8dacacf69c6975a0502cf9225957a6b42dc28f92cbbd34"

# The oldest macOS the set loads on. Version 1's floor; here it is honoured by
# every file, since nothing is a Homebrew bottle.
MACOS_MIN="15.0"
export MACOSX_DEPLOYMENT_TARGET="$MACOS_MIN"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out_lib="$repo_root/src-skathi/lib"
out_ff="$repo_root/src-skathi/ffmpeg-macos"
mpv_patches=("$repo_root"/src-skathi/patches/mpv-vulkan-render-api.diff)

case "$(uname -m)" in
  arm64)  arch=arm64 ;;
  x86_64) arch=x86_64 ;;
  *) echo "unsupported arch: $(uname -m)" >&2; exit 1 ;;
esac

# macOS's TMPDIR ends in a slash, and some build systems record a path with the
# doubled one, others without; the closure check below compares both collapsed.
tmp="${TMPDIR:-/tmp}"; tmp="${tmp%/}"
work="${FP_V2_WORK:-$tmp/frameplayer-v2-libs-macos$MACOS_MIN}"
dl="$work/dl"; src="$work/src"; prefix="$work/prefix"; logs="$work/logs"
mkdir -p "$dl" "$src" "$prefix/lib/pkgconfig" "$logs"
jobs="$(sysctl -n hw.ncpu)"

# ---------------------------------------------------------------------------
echo "==> Build tools"
if [ -n "${CI:-}" ]; then
  # Tools only — nothing installed here is linked.
  brew install -q meson ninja pkgconf cmake
fi
for t in meson ninja cmake pkg-config python3 git curl shasum; do
  command -v "$t" >/dev/null || { echo "FATAL: $t not found (build tool)" >&2; exit 1; }
done

# The fence. PKG_CONFIG_LIBDIR replaces pkg-config's default search path rather
# than adding to it, so Homebrew's .pc files are invisible.
unset PKG_CONFIG_PATH
export PKG_CONFIG_LIBDIR="$prefix/lib/pkgconfig:$prefix/share/pkgconfig"
export CMAKE_PREFIX_PATH="$prefix"
# Absolute install names into the prefix while building; the collect step at
# the end rewrites every one of them to @rpath.
cmake_common=(
  -G Ninja
  -DCMAKE_BUILD_TYPE=Release
  -DCMAKE_INSTALL_PREFIX="$prefix"
  -DCMAKE_INSTALL_LIBDIR=lib
  -DCMAKE_INSTALL_NAME_DIR="$prefix/lib"
  -DCMAKE_OSX_DEPLOYMENT_TARGET="$MACOS_MIN"
  -DCMAKE_OSX_ARCHITECTURES="$arch"
  -DCMAKE_IGNORE_PREFIX_PATH="/opt/homebrew;/usr/local"
  # Named, because the ignored prefixes hide Homebrew's ninja from find_program.
  -DCMAKE_MAKE_PROGRAM="$(command -v ninja)"
  -DCMAKE_C_COMPILER="$(xcrun -f clang)"
  -DCMAKE_CXX_COMPILER="$(xcrun -f clang++)"
)
meson_common=(
  --prefix="$prefix" --libdir=lib --buildtype=release
  --wrap-mode=nodownload
  -Dcmake_prefix_path="$prefix"
)

# fetch NAME URL SHA256 — downloads once, refuses a file that does not match.
fetch() {
  local name="$1" url="$2" sum="$3" f="$dl/$1"
  if [ ! -f "$f" ]; then
    curl -fsSL --retry 3 -o "$f.part" "$url"
    mv "$f.part" "$f"
  fi
  local got; got="$(shasum -a 256 "$f" | cut -c1-64)"
  if [ "$got" != "$sum" ]; then
    echo "FATAL: $name: sha256 $got, expected $sum ($url)" >&2
    rm -f "$f"
    exit 1
  fi
}

# unpack ARCHIVE DIR — a fresh tree every time a component is (re)built.
unpack() {
  rm -rf "${src:?}/$2" "$src/.unpack"
  mkdir -p "$src/.unpack"
  tar xf "$dl/$1" -C "$src/.unpack"
  mv "$src/.unpack/"* "$src/$2"
  rmdir "$src/.unpack"
}

# step NAME STAMP — true when NAME still has to be built; the stamp carries the
# pin, so a version bump rebuilds just that component.
step() {
  if [ -f "$prefix/.done-$1" ] && [ "$(cat "$prefix/.done-$1")" = "$2" ]; then
    echo "==> $1 ($2): built"
    return 1
  fi
  echo "==> $1 ($2)"
}
done_step() { echo "$2" > "$prefix/.done-$1"; }

# Quiet builds: the log goes to $logs, its tail to stderr on failure.
run() {
  local log="$logs/$1.log"; shift
  if ! "$@" >> "$log" 2>&1; then
    echo "FATAL: $* — last lines of $log:" >&2
    tail -40 "$log" >&2
    exit 1
  fi
}

# ---------------------------------------------------------------------------
if step vulkan-headers "$VULKAN_SDK"; then
  fetch vulkan-headers.tar.gz \
    "https://github.com/KhronosGroup/Vulkan-Headers/archive/refs/tags/vulkan-sdk-$VULKAN_SDK.tar.gz" \
    "$VULKAN_HEADERS_SHA256"
  unpack vulkan-headers.tar.gz vulkan-headers
  run vulkan-headers cmake -S "$src/vulkan-headers" -B "$src/vulkan-headers/build" "${cmake_common[@]}"
  run vulkan-headers cmake --install "$src/vulkan-headers/build"
  done_step vulkan-headers "$VULKAN_SDK"
fi

if step vulkan-loader "$VULKAN_SDK"; then
  fetch vulkan-loader.tar.gz \
    "https://github.com/KhronosGroup/Vulkan-Loader/archive/refs/tags/vulkan-sdk-$VULKAN_SDK.tar.gz" \
    "$VULKAN_LOADER_SHA256"
  unpack vulkan-loader.tar.gz vulkan-loader
  run vulkan-loader cmake -S "$src/vulkan-loader" -B "$src/vulkan-loader/build" "${cmake_common[@]}" \
    -DVULKAN_HEADERS_INSTALL_DIR="$prefix" -DUPDATE_DEPS=OFF -DBUILD_TESTS=OFF
  run vulkan-loader cmake --build "$src/vulkan-loader/build" -j "$jobs"
  run vulkan-loader cmake --install "$src/vulkan-loader/build"
  done_step vulkan-loader "$VULKAN_SDK"
fi

if step moltenvk "$MOLTENVK_VERSION"; then
  fetch moltenvk.tar \
    "https://github.com/KhronosGroup/MoltenVK/releases/download/v$MOLTENVK_VERSION/MoltenVK-macos.tar" \
    "$MOLTENVK_SHA256"
  rm -rf "$src/moltenvk" && mkdir -p "$src/moltenvk"
  tar xf "$dl/moltenvk.tar" -C "$src/moltenvk" MoltenVK/LICENSE MoltenVK/MoltenVK/dynamic/dylib/macOS
  # Upstream's dylib is universal; the set is one architecture, like the rest.
  lipo -thin "$arch" "$src/moltenvk/MoltenVK/MoltenVK/dynamic/dylib/macOS/libMoltenVK.dylib" \
    -output "$prefix/lib/libMoltenVK.dylib"
  done_step moltenvk "$MOLTENVK_VERSION"
fi

if step glslang "$VULKAN_SDK"; then
  fetch glslang.tar.gz \
    "https://github.com/KhronosGroup/glslang/archive/refs/tags/vulkan-sdk-$VULKAN_SDK.tar.gz" \
    "$GLSLANG_SHA256"
  unpack glslang.tar.gz glslang
  # Without SPIRV-Tools (ENABLE_OPT=OFF): libplacebo needs only the GLSL-to-
  # SPIR-V front end. In place of version 1's shaderc, which is the same glslang
  # wrapped. Shared, which is the configuration that was played back with.
  run glslang cmake -S "$src/glslang" -B "$src/glslang/build" "${cmake_common[@]}" \
    -DBUILD_SHARED_LIBS=ON \
    -DENABLE_OPT=OFF -DGLSLANG_TESTS=OFF -DENABLE_GLSLANG_BINARIES=OFF \
    -DENABLE_HLSL=OFF -DGLSLANG_ENABLE_INSTALL=ON
  run glslang cmake --build "$src/glslang/build" -j "$jobs"
  run glslang cmake --install "$src/glslang/build"
  done_step glslang "$VULKAN_SDK"
fi

if step lcms2 "$LCMS2_VERSION"; then
  fetch lcms2.tar.gz \
    "https://github.com/mm2/Little-CMS/releases/download/lcms$LCMS2_VERSION/lcms2-$LCMS2_VERSION.tar.gz" \
    "$LCMS2_SHA256"
  unpack lcms2.tar.gz lcms2
  run lcms2 meson setup "$src/lcms2/build" "$src/lcms2" "${meson_common[@]}" \
    -Djpeg=disabled -Dtiff=disabled -Dutils=false -Dtests=disabled \
    -Dfastfloat=false -Dthreaded=false   # the two plugins that are GPL-3.0
  run lcms2 ninja -C "$src/lcms2/build" install
  done_step lcms2 "$LCMS2_VERSION"
fi

if step libplacebo "$LIBPLACEBO_COMMIT"; then
  rm -rf "$src/libplacebo"
  run libplacebo git clone -q https://code.videolan.org/videolan/libplacebo.git "$src/libplacebo"
  run libplacebo git -C "$src/libplacebo" checkout -q "$LIBPLACEBO_COMMIT"
  [ "$(git -C "$src/libplacebo" rev-parse HEAD)" = "$LIBPLACEBO_COMMIT" ] ||
    { echo "FATAL: libplacebo is not at $LIBPLACEBO_COMMIT" >&2; exit 1; }
  # Pinned by the superproject's gitlinks; the demos' Nuklear is not needed.
  run libplacebo git -C "$src/libplacebo" submodule update --init \
    3rdparty/glad 3rdparty/jinja 3rdparty/markupsafe 3rdparty/fast_float 3rdparty/Vulkan-Headers
  # Its build file looks for libglslang in the compiler's own directories only
  # (every other glslang library gets `dirs: vulkan_lib_dirs`); give it the
  # same. Build system only, no code: the line is checked to have changed.
  sed -i '' "s|cxx.find_library('glslang', required: required, static: static)|cxx.find_library('glslang', required: required, static: static, dirs: vulkan_lib_dirs)|" \
    "$src/libplacebo/src/glsl/meson.build"
  grep -q "find_library('glslang', required: required, static: static, dirs: vulkan_lib_dirs)" \
    "$src/libplacebo/src/glsl/meson.build" || { echo "FATAL: libplacebo's glslang lookup moved" >&2; exit 1; }
  # glslang is found by find_library, not pkg-config: `vulkan-sdk` names the
  # directory for some of it, the compiler's search path (LIBRARY_PATH) the rest.
  run libplacebo env LIBRARY_PATH="$prefix/lib" CPATH="$prefix/include" meson setup "$src/libplacebo/build" "$src/libplacebo" "${meson_common[@]}" \
    -Dvulkan-sdk="$prefix" -Dvulkan=enabled -Dvk-proc-addr=enabled -Dglslang=enabled -Dshaderc=disabled \
    -Dlcms=enabled -Dopengl=enabled -Dd3d11=disabled \
    -Dlibdovi=disabled -Dxxhash=disabled -Dunwind=disabled \
    -Ddemos=false -Dtests=false -Dbench=false -Dfuzz=false
  run libplacebo env LIBRARY_PATH="$prefix/lib" CPATH="$prefix/include" ninja -C "$src/libplacebo/build" install
  done_step libplacebo "$LIBPLACEBO_COMMIT"
fi

if step dav1d "$DAV1D_VERSION"; then
  fetch dav1d.tar.xz \
    "https://downloads.videolan.org/pub/videolan/dav1d/$DAV1D_VERSION/dav1d-$DAV1D_VERSION.tar.xz" \
    "$DAV1D_SHA256"
  unpack dav1d.tar.xz dav1d
  run dav1d meson setup "$src/dav1d/build" "$src/dav1d" "${meson_common[@]}" \
    -Denable_tools=false -Denable_tests=false -Denable_examples=false
  run dav1d ninja -C "$src/dav1d/build" install
  done_step dav1d "$DAV1D_VERSION"
fi

if step ffmpeg "$FFMPEG_VERSION"; then
  fetch ffmpeg.tar.xz "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VERSION.tar.xz" "$FFMPEG_SHA256"
  unpack ffmpeg.tar.xz ffmpeg
  # Version 1's configure line, word for word (scripts/build-macos-libs.sh):
  # the same decoders, encoders and licence. Only the dav1d it finds differs —
  # ours, through the fence.
  ( cd "$src/ffmpeg" && run ffmpeg ./configure \
      --prefix="$prefix" \
      --enable-shared --disable-static \
      --disable-gpl --disable-nonfree --disable-version3 \
      --disable-doc --disable-programs --enable-ffmpeg \
      --enable-libdav1d \
      --enable-videotoolbox \
      --disable-debug )
  run ffmpeg make -C "$src/ffmpeg" -j"$jobs"
  run ffmpeg make -C "$src/ffmpeg" install
  done_step ffmpeg "$FFMPEG_VERSION"
fi
# Read out of the built binary, not the flags (see build-macos-libs.sh for why
# this is a `case` and not a pipe into grep).
ff_version="$(DYLD_LIBRARY_PATH="$prefix/lib" "$prefix/bin/ffmpeg" -hide_banner -version)"
case "$ff_version" in
  *--enable-gpl*|*--enable-nonfree*|*--enable-version3*)
    echo "FATAL: the built ffmpeg is not LGPL" >&2; exit 1 ;;
esac

if step freetype "$FREETYPE_VERSION"; then
  fetch freetype.tar.xz \
    "https://download.savannah.gnu.org/releases/freetype/freetype-$FREETYPE_VERSION.tar.xz" \
    "$FREETYPE_SHA256"
  unpack freetype.tar.xz freetype
  # zlib is the system's; brotli, bzip2 and png are only for font formats
  # libass never meets (WOFF2, compressed PCF, colour bitmaps).
  run freetype meson setup "$src/freetype/build" "$src/freetype" "${meson_common[@]}" \
    -Dbrotli=disabled -Dbzip2=disabled -Dharfbuzz=disabled -Dpng=disabled -Dzlib=system \
    -Dtests=disabled
  run freetype ninja -C "$src/freetype/build" install
  done_step freetype "$FREETYPE_VERSION"
fi

if step fribidi "$FRIBIDI_VERSION"; then
  fetch fribidi.tar.xz \
    "https://github.com/fribidi/fribidi/releases/download/v$FRIBIDI_VERSION/fribidi-$FRIBIDI_VERSION.tar.xz" \
    "$FRIBIDI_SHA256"
  unpack fribidi.tar.xz fribidi
  run fribidi meson setup "$src/fribidi/build" "$src/fribidi" "${meson_common[@]}" \
    -Ddocs=false -Dbin=false -Dtests=false
  run fribidi ninja -C "$src/fribidi/build" install
  done_step fribidi "$FRIBIDI_VERSION"
fi

if step harfbuzz "$HARFBUZZ_VERSION"; then
  fetch harfbuzz.tar.xz \
    "https://github.com/harfbuzz/harfbuzz/releases/download/$HARFBUZZ_VERSION/harfbuzz-$HARFBUZZ_VERSION.tar.xz" \
    "$HARFBUZZ_SHA256"
  unpack harfbuzz.tar.xz harfbuzz
  # No GLib, ICU or Graphite — version 1's Homebrew HarfBuzz pulled in GLib,
  # libintl, PCRE2 and Graphite2 for features libass does not call.
  run harfbuzz meson setup "$src/harfbuzz/build" "$src/harfbuzz" "${meson_common[@]}" \
    -Dfreetype=enabled -Dglib=disabled -Dgobject=disabled -Dicu=disabled \
    -Dgraphite2=disabled -Dcairo=disabled -Dchafa=disabled -Dcoretext=disabled \
    -Dpng=disabled -Dzlib=disabled -Draster=disabled -Dvector=disabled -Dgpu=disabled -Dsubset=disabled \
    -Dtests=disabled -Ddocs=disabled -Dutilities=disabled -Dintrospection=disabled
  run harfbuzz ninja -C "$src/harfbuzz/build" install
  done_step harfbuzz "$HARFBUZZ_VERSION"
fi

if step libunibreak "$UNIBREAK_VERSION"; then
  fetch libunibreak.tar.gz \
    "https://github.com/adah1972/libunibreak/releases/download/libunibreak_${UNIBREAK_VERSION//./_}/libunibreak-$UNIBREAK_VERSION.tar.gz" \
    "$UNIBREAK_SHA256"
  unpack libunibreak.tar.gz libunibreak
  ( cd "$src/libunibreak" && run libunibreak ./configure --prefix="$prefix" --disable-static )
  run libunibreak make -C "$src/libunibreak" -j"$jobs" install
  done_step libunibreak "$UNIBREAK_VERSION"
fi

if step libass "$LIBASS_VERSION"; then
  fetch libass.tar.xz \
    "https://github.com/libass/libass/releases/download/$LIBASS_VERSION/libass-$LIBASS_VERSION.tar.xz" \
    "$LIBASS_SHA256"
  unpack libass.tar.xz libass
  # CoreText finds the fonts, as with Homebrew's; no fontconfig.
  ( cd "$src/libass" && run libass ./configure --prefix="$prefix" --disable-static \
      --disable-fontconfig --enable-coretext --enable-libunibreak )
  run libass make -C "$src/libass" -j"$jobs" install
  done_step libass "$LIBASS_VERSION"
fi

if step uchardet "$UCHARDET_VERSION"; then
  fetch uchardet.tar.xz \
    "https://www.freedesktop.org/software/uchardet/releases/uchardet-$UCHARDET_VERSION.tar.xz" \
    "$UCHARDET_SHA256"
  unpack uchardet.tar.xz uchardet
  run uchardet cmake -S "$src/uchardet" -B "$src/uchardet/build" "${cmake_common[@]}" \
    -DBUILD_BINARY=OFF -DBUILD_STATIC=OFF -DCMAKE_POLICY_VERSION_MINIMUM=3.5
  run uchardet cmake --build "$src/uchardet/build" -j "$jobs"
  run uchardet cmake --install "$src/uchardet/build"
  done_step uchardet "$UCHARDET_VERSION"
fi

if step luajit "$LUAJIT_COMMIT"; then
  fetch luajit.tar.gz "https://github.com/LuaJIT/LuaJIT/archive/$LUAJIT_COMMIT.tar.gz" "$LUAJIT_SHA256"
  unpack luajit.tar.gz luajit
  run luajit make -C "$src/luajit" -j"$jobs" PREFIX="$prefix" MACOSX_DEPLOYMENT_TARGET="$MACOS_MIN"
  run luajit make -C "$src/luajit" install PREFIX="$prefix"
  done_step luajit "$LUAJIT_COMMIT"
fi

# ---------------------------------------------------------------------------
# mpv: the tree is unpacked and patched in one go, and the stamp carries the
# patch, so a changed patch rebuilds it (the same trap as version 1's tree).
patch_stamp="$MPV_COMMIT-$(cat "${mpv_patches[@]}" | shasum -a 256 | cut -c1-12)"
if step mpv "$patch_stamp"; then
  fetch mpv.tar.gz "https://github.com/mpv-player/mpv/archive/$MPV_COMMIT.tar.gz" "$MPV_SHA256"
  unpack mpv.tar.gz mpv
  for patch_file in "${mpv_patches[@]}"; do
    echo "    applying $(basename "$patch_file")"
    run mpv patch -d "$src/mpv" -p1 -i "$patch_file"
  done
  # Version 1's options, plus the fence. -Dgpl=false is the licence.
  run mpv meson setup "$src/mpv/build" "$src/mpv" "${meson_common[@]}" \
    -Dlibmpv=true -Dcplayer=false -Dgpl=false -Drubberband=disabled \
    -Dvulkan=enabled -Dlua=luajit
  run mpv ninja -C "$src/mpv/build"
  done_step mpv "$patch_stamp"
fi
grep -qx '#define HAVE_GPL 0' "$src/mpv/build/config.h" ||
  { echo "FATAL: mpv was configured with GPL features" >&2; exit 1; }

# ---------------------------------------------------------------------------
echo "==> Collecting into $out_lib"
rm -rf "$out_lib" "$out_ff"
mkdir -p "$out_lib" "$out_ff/bin" "$out_ff/lib" "$out_ff/include"
cp "$src/mpv/build/libmpv.2.dylib" "$out_lib/libmpv.dylib"
cp "$prefix/lib/libMoltenVK.dylib" "$out_lib/"
cp "$prefix/bin/ffmpeg" "$out_ff/bin/ffmpeg"

# Non-system dependencies of a Mach-O file, as written in it.
deps_of() {
  otool -L "$1" | tail -n +2 | sed 's/ (compatibility.*//; s/^[[:space:]]*//' |
    grep -vE '^(/usr/lib/|/System/)' || true
}
# The closure: every dependency is a file of the prefix, by absolute path or
# by @rpath name. Anything else is a leak, and fails here.
queue=("$out_lib/libmpv.dylib" "$out_ff/bin/ffmpeg")
while [ "${#queue[@]}" -gt 0 ]; do
  f="${queue[0]}"; queue=("${queue[@]:1}")
  own="$(otool -D "$f" | tail -n +2 | head -1 || true)"
  while read -r d; do
    [ -n "$d" ] && [ "$d" != "$own" ] || continue
    name="$(basename "$d")"
    [ -f "$out_lib/$name" ] && continue
    case "$(printf %s "$d" | tr -s /)" in
      "$prefix"/lib/*|@rpath/*) ;;
      *) echo "FATAL: $(basename "$f") links $d, which is not ours" >&2; exit 1 ;;
    esac
    [ -f "$prefix/lib/$name" ] || { echo "FATAL: $name not in $prefix/lib" >&2; exit 1; }
    cp -L "$prefix/lib/$name" "$out_lib/$name"
    queue+=("$out_lib/$name")
  done < <(deps_of "$f")
done

# @rpath/<name> everywhere, @loader_path the only run path (version 1's rule
# and its reason: bundle-macos-libs.sh), ad-hoc signed since the edit breaks
# the signature. The CLI looks in ./lib beside itself, as version 1's does.
rewrite() {
  local f="$1" d rp
  while read -r d; do
    [ -n "$d" ] || continue
    [ "$d" = "@rpath/$(basename "$d")" ] && continue
    install_name_tool -change "$d" "@rpath/$(basename "$d")" "$f" 2>/dev/null
  done < <(deps_of "$f")
  while read -r rp; do
    [ -n "$rp" ] && install_name_tool -delete_rpath "$rp" "$f" 2>/dev/null
  done < <(otool -l "$f" | awk '/cmd LC_RPATH/{getline;getline;print $2}')
  install_name_tool -add_rpath "$2" "$f"
  codesign --force --sign - "$f" 2>/dev/null
}
for f in "$out_lib"/*.dylib; do
  chmod 644 "$f"
  install_name_tool -id "@rpath/$(basename "$f")" "$f"
  rewrite "$f" "@loader_path"
done
chmod 755 "$out_ff/bin/ffmpeg"
rewrite "$out_ff/bin/ffmpeg" "@executable_path/lib"

# The loader finds MoltenVK through this manifest, relative to the manifest
# itself (VK_DRIVER_FILES in development; a bundle puts a copy under
# Contents/Resources/vulkan/icd.d with the path adjusted).
cat > "$out_lib/MoltenVK_icd.json" <<JSON
{
    "file_format_version": "1.0.0",
    "ICD": {
        "library_path": "./libMoltenVK.dylib",
        "api_version": "${VULKAN_SDK%.*}",
        "is_portability_driver": true
    }
}
JSON

# The FFmpeg SDK, laid out like version 1's (bundle-macos-libs.sh).
for h in libavutil libavcodec libavformat libavdevice libavfilter libswscale libswresample; do
  cp -R "$prefix/include/$h" "$out_ff/include/"
done
for l in avutil avcodec avformat avdevice avfilter swscale swresample; do
  real="$(cd "$out_lib" && ls -1 | grep -E "^lib$l\.[0-9]+\.dylib$" | head -1)"
  ln -s "../../lib/$real" "$out_ff/lib/lib$l.dylib"
done

# ---------------------------------------------------------------------------
echo "==> Checks"
bad=0
floor_int="$(awk -F. '{ printf "%d%02d%02d\n", $1, $2, $3 }' <<<"$MACOS_MIN")"
for f in "$out_lib"/*.dylib "$out_ff/bin/ffmpeg"; do
  name="$(basename "$f")"
  # Only system paths and our own @rpath names, each present in the set.
  while read -r d; do
    [ -n "$d" ] || continue
    case "$d" in
      @rpath/*) [ -f "$out_lib/${d#@rpath/}" ] || { echo "  ✗ $name needs $d, not in the set" >&2; bad=1; } ;;
      *) echo "  ✗ $name links $d" >&2; bad=1 ;;
    esac
  done < <(deps_of "$f")
  rpaths="$(otool -l "$f" | awk '/cmd LC_RPATH/{getline;getline;print $2}' | tr '\n' ' ')"
  case "$name" in ffmpeg) want="@executable_path/lib " ;; *) want="@loader_path " ;; esac
  [ "$rpaths" = "$want" ] || { echo "  ✗ $name run paths: $rpaths" >&2; bad=1; }
  minos="$(otool -l "$f" | awk '/cmd LC_BUILD_VERSION/{w="minos"} /cmd LC_VERSION_MIN_MACOSX/{w="version"} w!="" && $1==w {print $2; exit}')"
  if [ -z "$minos" ] || [ "$(awk -F. '{ printf "%d%02d%02d\n", $1, $2, $3 }' <<<"$minos")" -gt "$floor_int" ]; then
    echo "  ✗ $name: minimum macOS ${minos:-unknown}, above $MACOS_MIN" >&2; bad=1
  fi
done
[ "$bad" -eq 0 ] || { echo "FATAL: the set is not self-contained" >&2; exit 1; }
echo "  ✓ every library is @rpath within the set, one run path, minimum macOS ≤ $MACOS_MIN"

MACOS_LIB_DIR="$out_lib" MACOS_FFMPEG_CLI="$out_ff/bin/ffmpeg" \
  "$repo_root/scripts/check-macos-licenses.sh"

# libmpv loads with its whole closure from the set alone.
python3 -I -c 'import ctypes, sys; ctypes.CDLL(sys.argv[1]); print("  ✓ libmpv loads")' "$out_lib/libmpv.dylib"

echo
echo "Done: $out_lib ($(ls -1 "$out_lib"/*.dylib | wc -l | tr -d ' ') libraries, $(du -sh "$out_lib" | cut -f1))"
echo "  MPV_LIB_DIR=$out_lib MP_VK_LIB=$out_lib/libvulkan.1.dylib VK_DRIVER_FILES=$out_lib/MoltenVK_icd.json"
echo "  FFMPEG_DIR=$out_ff"
