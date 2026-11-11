#!/usr/bin/env bash
# The .deb's `Depends`, read off the binary that was built — what
# `dpkg-shlibdeps` does for a package built the Debian way, which the Tauri
# bundler is not.
#
# A fixed list in tauri.linux.conf.json was right for exactly one release of one
# distribution: the crate links whatever FFmpeg the build machine has, and the
# package names carry its sonames. Written for Ubuntu 24.04 (`libavcodec60`…),
# a .deb built on 26.04 (FFmpeg 8: `libavcodec62`…) could not be installed at
# all. The list was not even the right set: the linker drops a library nothing
# is used from (`--as-needed`), and libswresample and libavfilter were listed
# while the binary needed neither.
#
# So: every `NEEDED` entry of the binary, resolved the way the loader resolves
# it (ldd), mapped to the package that owns the file (dpkg -S) — plus libmpv,
# which the binary never names: the libmpv plugin opens `libmpv.so.2` at run
# time (`LIBMPV_SONAME`). Prints a JSON array. Fails, saying which, when a
# library belongs to no package: a .deb cannot depend on a file.
#
#   scripts/linux/deb-depends.sh src-tauri/target/release/frameplayer
set -euo pipefail

binary="${1:?usage: deb-depends.sh <binary>}"
command -v dpkg >/dev/null || { echo "deb-depends: dpkg not found — a .deb is built on a Debian-family system" >&2; exit 1; }

# The library opened at run time rather than linked (see the header).
runtime_opened=(libmpv.so.2)

# Each command's output is read whole before it is searched: a reader that
# stops early (`awk … exit`, `head`) sends the writer SIGPIPE, which pipefail
# turns into this script's failure.
needed=$(objdump -p "$binary" | awk '/NEEDED/ { print $2 }')
resolved=$(ldd "$binary")
cache=$(ldconfig -p)

paths=()
for soname in $needed; do
  # The dynamic loader itself comes with libc6, which libc.so.6 already names.
  case "$soname" in ld-linux*) continue ;; esac
  path=$(awk -v s="$soname" '$1 == s && $3 ~ /^\// { print $3 }' <<<"$resolved")
  [ -n "$path" ] || { echo "deb-depends: $soname is not found by the loader" >&2; exit 1; }
  paths+=("$path")
done
for soname in "${runtime_opened[@]}"; do
  path=$(awk -v s="$soname" '$1 == s && !found { print $NF; found = 1 }' <<<"$cache")
  [ -n "$path" ] || { echo "deb-depends: $soname is not installed (libmpv's runtime package)" >&2; exit 1; }
  paths+=("$path")
done

packages=()
for path in "${paths[@]}"; do
  # dpkg knows the path it installed, which may be the symlink's or the
  # file's; on a merged-/usr system one of them may sit under /lib instead.
  owner=""
  for candidate in "$path" "$(readlink -f "$path")" "${path#/usr}" "$(readlink -f "$path" | sed 's|^/usr||')"; do
    found=$(dpkg -S "$candidate" 2>/dev/null) || continue
    owner=$(awk -F: 'NR == 1 { print $1 }' <<<"$found")
    [ -n "$owner" ] && break
  done
  [ -n "$owner" ] || { echo "deb-depends: no package owns $path" >&2; exit 1; }
  packages+=("$owner")
done

printf '%s\n' "${packages[@]}" | sort -u | awk 'BEGIN { printf "[" } { printf "%s\"%s\"", (NR > 1 ? "," : ""), $0 } END { print "]" }'
