#!/bin/sh
# Read-only source transfer from Nura into a NEW host-private staging directory.
# Binary provenance is SHA-pinned; this is NOT a Mesa/LLVM source rebuild.
set -eu

if [ "$#" -ne 1 ]; then
    echo "usage: $0 FRESH_HOST_DIRECTORY" >&2
    exit 64
fi
destination=$1
case "$destination" in
    /*) ;;
    *) destination="$PWD/$destination" ;;
esac
if [ -e "$destination" ] || [ -L "$destination" ]; then
    echo "refusing existing graphics staging destination: $destination" >&2
    exit 2
fi
mkdir -p "$destination"

# SSH host alias 'sl101' is intentionally user-managed (192.168.23.106).
# No remote commands, installation, process changes, or system writes.
private=/home/user/wpe2542-canary-20261124/mesa-d16-gnu-tls-candidate-20261124
scp -B -p \
    "sl101:$private/libEGL.so.1.0.0" \
    "sl101:$private/libGLESv2.so.2.0.0" \
    "sl101:$private/libgbm.so.1.0.0" \
    "sl101:$private/libgallium-26.2.4.so" \
    "$destination/"
scp -B -p \
    sl101:/usr/lib/libGLESv1_CM.so.1.1.0 \
    sl101:/usr/lib/dri/libdril_dri.so \
    sl101:/usr/lib/libwayland-client.so.0.26.0 \
    "$destination/"
# This object is ~139 MiB; compression reduces connection traffic.
scp -B -C -p sl101:/usr/lib/libLLVM.so.23.1 "$destination/"

repository=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repository"
python3 -c 'import sys
from pathlib import Path
from scripts.assemble_nurabrowser_alpine_d16_root import verified_graphics
verified = verified_graphics(Path(sys.argv[1]))
print("NURA_MESA26_D16_STAGING=PASS", len(verified), "qualification=UNQUALIFIED")
' "$destination"
