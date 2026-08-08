#!/usr/bin/env bash
# Keeps the N newest releases in an R2 bucket and deletes what is older.
#
#   scripts/r2-retain.sh --kind updates --bucket b --endpoint e --keep 5
#   scripts/r2-retain.sh --kind libs    --bucket b --endpoint e --keep 5 --apply
#
# **Dry run unless `--apply` is given.** This deletes from a bucket that serves
# installed players; a mistake here is not recoverable from a git history.
#
# Objects are grouped and whole groups are kept or dropped, never single files.
# The two buckets group differently and both would break if pruned per file:
#
#   updates — one group per version (`FramePlayer_<version>_…`). The `.exe` and
#             `.dmg` are also attached to the GitHub Release, so pruning here
#             does not put a version out of reach; the `.app.tar.gz` is the
#             updater's payload and lives only here, reachable only through
#             `latest.json`, which always names the newest version.
#   libs    — one group per set (`macos-<arch>-<key>`), the archive and its
#             `.sha256` together. `fetch-macos-libs.sh` refuses an archive whose
#             checksum is missing, so separating the pair would leave a set that
#             is present and unusable.
#
# **Anything that does not match the group pattern is never touched.** That is
# what protects `latest.json`, and it is a whitelist rather than a blacklist on
# purpose: a file nobody anticipated survives instead of being swept.
set -euo pipefail

kind='' bucket='' endpoint='' keep=5 apply=0
while [ $# -gt 0 ]; do
  case "$1" in
    --kind)     kind="$2"; shift 2 ;;
    --bucket)   bucket="$2"; shift 2 ;;
    --endpoint) endpoint="$2"; shift 2 ;;
    --keep)     keep="$2"; shift 2 ;;
    --apply)    apply=1; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
[ -n "$kind" ] && [ -n "$bucket" ] && [ -n "$endpoint" ] || {
  echo "usage: $0 --kind libs|updates --bucket B --endpoint E [--keep N] [--apply]" >&2
  exit 2
}
case "$keep" in ''|*[!0-9]*) echo "--keep must be a number" >&2; exit 2 ;; esac
[ "$keep" -ge 1 ] || { echo "--keep must be at least 1" >&2; exit 2; }

# The group a key belongs to, or nothing when the key is not ours to manage.
group_of() {
  case "$kind" in
    libs)
      case "$1" in
        macos-*.tar.gz)        echo "${1%.tar.gz}" ;;
        macos-*.tar.gz.sha256) echo "${1%.tar.gz.sha256}" ;;
      esac ;;
    updates)
      case "$1" in
        # `FramePlayer_<version>_<rest>`; the version is the second field.
        FramePlayer_*_*) printf '%s\n' "$1" | cut -d_ -f2 ;;
      esac ;;
    *) echo "unknown --kind: $kind" >&2; exit 2 ;;
  esac
}

listing="$(aws s3 ls "s3://$bucket/" --endpoint-url "$endpoint")"

# One line per object: "<last-modified> <group> <key>". The timestamp sorts
# lexicographically because it is ISO-like, so no date parsing is needed.
rows="$(
  printf '%s\n' "$listing" | while read -r date time _size key; do
    [ -n "${key:-}" ] || continue
    g="$(group_of "$key")"
    [ -n "$g" ] || continue
    echo "$date $time $g $key"
  done
)"

if [ -z "$rows" ]; then
  echo "nothing matching --kind $kind in s3://$bucket/"
  exit 0
fi

# Newest timestamp per group, newest group first.
groups="$(printf '%s\n' "$rows" | awk '{print $3, $1" "$2}' \
          | sort -k2 -r | awk '!seen[$1]++ {print $1}')"
total="$(printf '%s\n' "$groups" | wc -l | tr -d ' ')"
doomed="$(printf '%s\n' "$groups" | tail -n +$((keep + 1)))"

echo "s3://$bucket/  kind=$kind  groups=$total  keep=$keep"
printf '%s\n' "$groups" | head -n "$keep" | sed 's/^/  keep  /'

if [ -z "$doomed" ]; then
  echo "  nothing to remove"
  exit 0
fi

freed=0
for g in $doomed; do
  echo "  drop  $g"
  # Exact field match, so `0.3` cannot select the `0.31.0` group.
  printf '%s\n' "$rows" | awk -v g="$g" '$3 == g {print $4}' | while read -r key; do
    size="$(printf '%s\n' "$listing" | awk -v k="$key" '$4 == k {print $3}')"
    echo "          $key (${size:-?} bytes)"
    if [ "$apply" = 1 ]; then
      aws s3 rm "s3://$bucket/$key" --endpoint-url "$endpoint" >/dev/null
    fi
  done
  freed=$((freed + 1))
done

if [ "$apply" = 1 ]; then
  echo "removed $freed group(s)"
else
  echo "dry run — $freed group(s) would be removed; pass --apply to do it"
fi
