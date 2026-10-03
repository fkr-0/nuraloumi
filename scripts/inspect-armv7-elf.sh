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

find_objdump() {
    if command -v llvm-objdump >/dev/null 2>&1; then
        command -v llvm-objdump
        return 0
    fi
    if command -v rustc >/dev/null 2>&1; then
        rust_sysroot=$(rustc --print sysroot 2>/dev/null || true)
        rust_host=$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')
        rust_llvm_objdump="$rust_sysroot/lib/rustlib/$rust_host/bin/llvm-objdump"
        if [ -x "$rust_llvm_objdump" ]; then
            printf '%s\n' "$rust_llvm_objdump"
            return 0
        fi
    fi
    if command -v objdump >/dev/null 2>&1; then
        command -v objdump
        return 0
    fi
    return 1
}

OBJDUMP=$(find_objdump || true)
UPPER_VFP_RE='(^|[[:space:],{])d(1[6-9]|2[0-9]|3[01])([^[:alnum:]_]|$)'

# Fail closed if the audit expression ever stops recognizing the exact class
# of Tegra20-breaking operand that exposed the musl round() false negative.
if ! printf '%s\n' '  1950c0: vldr d16, [pc, #136]' | grep -Eq "$UPPER_VFP_RE"; then
    echo "ELF_AUDIT=FAIL reason=upper-vfp-regex-self-test-d16" >&2
    exit 1
fi
if printf '%s\n' '  000000: vldr d15, [pc, #4]' | grep -Eq "$UPPER_VFP_RE"; then
    echo "ELF_AUDIT=FAIL reason=upper-vfp-regex-self-test-d15" >&2
    exit 1
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
    if ! printf '%s\n' "$attrs" | grep -Eq 'Tag_FP_arch:[[:space:]]+VFPv3-D16'; then
        echo "ELF_FILE=FAIL path=$elf reason=not-VFPv3-D16"
        status=1
        continue
    fi

    if [ -z "$OBJDUMP" ]; then
        echo "ELF_FILE=PENDING path=$elf arm32=yes neon-attribute=no reason=objdump-not-installed"
        mark_pending
        continue
    fi
    if ! disasm=$("$OBJDUMP" -d "$elf" 2>/dev/null); then
        echo "ELF_FILE=PENDING path=$elf arm32=yes neon-attribute=no reason=objdump-disassembly-failed tool=$OBJDUMP"
        mark_pending
        continue
    fi
    if printf '%s\n' "$disasm" | grep -Eiq '[[:space:]](vld[1-4]|vst[1-4]|vdup|vext|vtbl|vtbx|vzip|vuzp|vtrn|vrev|vswp|vq[a-z0-9.]+)[[:space:]]|[[:space:],{]q[0-9]+'; then
        echo "ELF_FILE=FAIL path=$elf reason=possible-NEON-opcode-or-q-register"
        status=1
        continue
    fi
    if printf '%s\n' "$disasm" | grep -Eiq "$UPPER_VFP_RE"; then
        echo "ELF_FILE=FAIL path=$elf reason=upper-VFP-register-d16-d31-detected"
        status=1
        continue
    fi
    if printf '%s\n' "$disasm" | grep -Eiq '[[:space:]](sdiv|udiv)[[:space:]]'; then
        echo "ELF_FILE=FAIL path=$elf reason=hardware-divide-opcode-detected"
        status=1
        continue
    fi
    echo "ELF_FILE=PASS path=$elf arm32=yes fp=VFPv3-D16 neon-attribute=no neon-disassembly-heuristic=no upper-dregs=no hwdiv=no"
done

exit "$status"
