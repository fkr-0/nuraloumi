#!/bin/sh
# Launch only the source-pinned D16 crypto builder with persistent local state.
# No device operation, network, image pull or production installer.
set -eu
umask 077

usage() {
  echo "usage: sh scripts/launch_nurabrowser_d16_persistent.sh --preflight|--start" >&2
  exit 64
}
[ "$#" -eq 1 ] || usage
case "$1" in --preflight|--start) ;; *) usage ;; esac

repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
scripts="$repo/scripts"
state="$repo/local-state/build/nurabrowser-d16"
sources="$state/sources"
overlay="$state/overlay"
image=nuraloumi-crypto-d16:alpine322-r2
container=nura-d16-r2-persistent

[ -d "$sources" ] || { echo "persistent pinned source cache missing" >&2; exit 2; }
[ -f "$sources/SOURCE-MANIFEST.json" ] || { echo "source manifest missing" >&2; exit 2; }
[ ! -e "$overlay/work" ] || { echo "existing build work detected; resume or inspect, never overwrite" >&2; exit 2; }
[ ! -e "$overlay/CANDIDATE-ONLY.txt" ] || { echo "already completed candidate detected" >&2; exit 2; }
command -v docker >/dev/null 2>&1
docker image inspect "$image" >/dev/null
if docker container inspect "$container" >/dev/null 2>&1; then
  echo "named builder already exists; inspect or resume it, never duplicate" >&2
  exit 2
fi

# Fail closed when an overnight emulated build would consume the last host space.
available_kib=$(df -Pk "$repo" | awk 'END {print $4}')
case "$available_kib" in ''|*[!0-9]*) echo "cannot measure free disk" >&2; exit 2 ;; esac
[ "$available_kib" -ge 8388608 ] || { echo "less than 8 GiB free on persistent filesystem" >&2; exit 2; }

if [ "$1" = --preflight ]; then
  echo "D16_PERSISTENT_BUILD=READY source_cache=present image=local disk=adequate"
  exit 0
fi

mkdir -p "$overlay"
docker run --detach --pull never --platform linux/arm/v7 \
  --name "$container" --network none --read-only --cap-drop ALL \
  --security-opt no-new-privileges --memory 16g --cpus 20 \
  --pids-limit 512 --env NURA_BUILD_JOBS=20 --user "$(id -u):$(id -g)" \
  --tmpfs /tmp:rw,size=256m \
  --mount "type=bind,src=$scripts,dst=/work/scripts,readonly" \
  --mount "type=bind,src=$sources,dst=/inputs,readonly" \
  --mount "type=bind,src=$overlay,dst=/out" \
  "$image" sh /work/scripts/rebuild_nurabrowser_wpe_d16_crypto.sh /inputs /out
echo "D16_PERSISTENT_BUILD=STARTED container=$container state=local-state/build/nurabrowser-d16"
