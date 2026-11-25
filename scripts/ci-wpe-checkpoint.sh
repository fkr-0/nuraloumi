#!/usr/bin/env bash
# Host-side, bounded WPE build checkpoint. Never qualifies a runtime.
# Usage: ci-wpe-checkpoint.sh create OUTPUT_ROOT ARCHIVE
#        ci-wpe-checkpoint.sh restore ARCHIVE OUTPUT_ROOT ARCHIVE_SHA256 SOURCE_SHA
set -euo pipefail

die() { printf 'WPE checkpoint: %s\n' "$*" >&2; exit 2; }

case "${1:-}" in
  create)
    [[ $# -eq 3 ]] || die 'create requires OUTPUT_ROOT ARCHIVE'
    root=$2
    archive=$3
    [[ -s "$root/BUILD-PROVENANCE.txt" ]] || die 'missing builder provenance'
    [[ -s "$root/build/webkit/build.ninja" ]] || die 'missing configured Ninja graph'
    [[ -f "$root/src/wpewebkit-2.54.2/CMakeLists.txt" ]] || die 'missing source tree'
    [[ ! -e "$archive" ]] || die 'refusing to replace an archive'
    command -v zstd >/dev/null || die 'zstd is required on the x86 runner'
    mkdir -p "$(dirname "$archive")"
    tar -C "$root" -cf - src build BUILD-PROVENANCE.txt |
      zstd -q -1 -T2 -o "$archive"
    [[ -s "$archive" ]] || die 'empty checkpoint'
    (cd "$(dirname "$archive")" && sha256sum "$(basename "$archive")" > "$(basename "$archive").sha256")
    printf 'WPE_CHECKPOINT=BUILT qualification=UNQUALIFIED\n'
    ;;
  restore)
    [[ $# -eq 5 ]] || die 'restore requires ARCHIVE OUTPUT_ROOT ARCHIVE_SHA256 SOURCE_SHA'
    archive=$2
    root=$3
    digest=$4
    source_sha=$5
    [[ "$digest" =~ ^[0-9a-f]{64}$ ]] || die 'invalid checkpoint digest'
    [[ "$source_sha" =~ ^[0-9a-f]{40}$ ]] || die 'invalid source SHA'
    [[ -s "$archive" ]] || die 'missing checkpoint archive'
    printf '%s  %s\n' "$digest" "$archive" | sha256sum --check --status ||
      die 'archive SHA256 mismatch'
    [[ ! -e "$root/src" && ! -e "$root/build" && ! -e "$root/BUILD-PROVENANCE.txt" ]] ||
      die 'refusing to overwrite build state'
    # Accept only our three archive roots, without absolute/traversal paths.
    # Keep the tar-list exit status; process substitution would mask a bad tar.
    members=$(mktemp)
    trap 'rm -f "$members"' EXIT
    zstd -qd -c "$archive" | tar -tf - > "$members"
    while IFS= read -r name; do
      case "$name" in
        src|src/|src/*|build|build/|build/*|BUILD-PROVENANCE.txt) ;;
        *) die "unexpected checkpoint member: $name" ;;
      esac
      case "/$name/" in
        */../*|*/./*) die "unsafe checkpoint member: $name" ;;
      esac
    done < "$members"
    mkdir -p "$root"
    zstd -qd -c "$archive" | tar --no-same-owner --no-same-permissions -xf - -C "$root"
    grep -Fx "source_sha=$source_sha" "$root/BUILD-PROVENANCE.txt" >/dev/null ||
      die 'checkpoint belongs to another source revision'
    [[ -s "$root/build/webkit/build.ninja" ]] || die 'missing restored Ninja graph'
    printf 'WPE_CHECKPOINT=RESTORED qualification=UNQUALIFIED\n'
    ;;
  *) die 'expected create or restore' ;;
esac
