#!/bin/sh
set -eu

if [ "$#" -eq 0 ]; then
    echo "ELF_AUDIT=PENDING reason=no-binaries-supplied"
    exit 2
fi

if ! command -v readelf >/dev/null 2>&1; then
    echo "ELF_AUDIT=PENDING reason=readelf-not-installed"
    exit 2
fi

status=0
mark_pending() {
    if [ "$status" -eq 0 ]; then
        status=2
    fi
}

for elf in "$@"; do
    if [ ! -f "$elf" ]; then
        echo "ELF_FILE=PENDING path=$elf reason=missing"
        mark_pending
        continue
    fi

    if ! header=$(readelf -h "$elf" 2>/dev/null); then
        echo "ELF_FILE=FAIL path=$elf reason=readelf-header-failed"
        status=1
        continue
    fi
    if ! printf '%s\n' "$header" | grep -Eq 'Class:[[:space:]]+ELF32'; then
        echo "ELF_FILE=FAIL path=$elf reason=not-ELF32"
        status=1
        continue
    fi
    if ! printf '%s\n' "$header" | grep -Eq 'Machine:[[:space:]]+ARM'; then
        echo "ELF_FILE=FAIL path=$elf reason=not-ARM"
        status=1
        continue
    fi

    if ! attrs=$(readelf -A "$elf" 2>/dev/null); then
        echo "ELF_FILE=PENDING path=$elf reason=readelf-attributes-failed"
        mark_pending
        continue
    fi
    if printf '%s\n' "$attrs" | grep -Eiq 'Advanced_SIMD|NEON'; then
        echo "ELF_FILE=FAIL path=$elf reason=NEON-attribute-detected"
        status=1
        continue
    fi

    if ! command -v objdump >/dev/null 2>&1; then
        echo "ELF_FILE=PENDING path=$elf arm32=yes neon-attribute=no reason=objdump-not-installed"
        mark_pending
        continue
    fi
    if ! disasm=$(objdump -d "$elf" 2>/dev/null); then
        echo "ELF_FILE=PENDING path=$elf arm32=yes neon-attribute=no reason=objdump-disassembly-failed"
        mark_pending
        continue
    fi
    if printf '%s\n' "$disasm" | grep -Eiq '[[:space:]](vld[1-4]|vst[1-4]|vdup|vext|vtbl|vtbx|vzip|vuzp|vtrn|vrev|vswp|vq[a-z0-9.]+)[[:space:]]|[[:space:],{]q[0-9]+'; then
        echo "ELF_FILE=FAIL path=$elf reason=possible-NEON-opcode-or-q-register"
        status=1
        continue
    fi
    echo "ELF_FILE=PASS path=$elf arm32=yes neon-attribute=no neon-disassembly-heuristic=no"
done

exit "$status"
