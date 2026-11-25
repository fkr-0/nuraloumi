#!/usr/bin/env python3
"""Streaming ARMv7 VFPv3-D16 disassembly preflight; never generates runtime.json.

ELF decoding is insufficient to prove JIT, dlopen, sandbox, or device pixels.
"""
import argparse
from collections import Counter
import json
from pathlib import Path
import re
import subprocess

INSTRUCTION = re.compile(r"^\s*([0-9a-f]+):\s+([a-z][a-z0-9_.]*)\s*(.*?)\s*$", re.I)
UPPER_VFP = re.compile(r"\b(?:d(?:1[6-9]|2[0-9]|3[01])|s(?:3[2-9]|[4-5][0-9]|6[0-3]))\b", re.I)
NEON_Q = re.compile(r"\bq(?:[0-9]|1[0-5])\b", re.I)
FORBIDDEN = re.compile(
    r"^(?:sdiv|udiv|vld[1-4]|vst[1-4]|vdup|vext|vtbl|vtbx|"
    r"vzip|vuzp|vtrn|vrev(?:16|32|64)?|vswp|vq[a-z0-9_]*|"
    r"vand|vbic|vbsl|veor|vorr|vorn|vmvn|vbit|vbif|"
    r"vcnt|vcls|vclz|vcge|vcgt|vcle|vclt|vceq|vtst|"
    r"vmax[a-z0-9_]*|vmin[a-z0-9_]*|vmovl|vmovn|vmull|"
    r"vpad[a-z0-9_]*|vsh[a-z0-9_]*|vsli|vsri|"
    r"vsel[a-z0-9_]*|vfma|vfms|vfnma|vfnms|"
    r"aese|aesd|aesmc|aesimc|sha1[a-z0-9_]*|sha256[a-z0-9_]*|"
    r"crc32[a-z0-9_]*)(?:eq|ne|cs|cc|mi|pl|vs|vc|hi|ls|ge|lt|gt|le)?(?:\.|$)", re.I)
SIMD_SUFFIX = re.compile(r"\.(?:i|p)(?:8|16|32|64)$|\.f16$", re.I)
LIMIT = 256
SAMPLES_PER_CODE = 20


def inspect_instruction(line):
    match = INSTRUCTION.match(line)
    if not match:
        return None
    mnemonic = match.group(2).lower()
    operands = match.group(3).split("@", 1)[0].split(";", 1)[0]
    codes = []
    if UPPER_VFP.search(operands):
        codes.append("UPPER_VFP_REGISTER")
    if NEON_Q.search(operands):
        codes.append("NEON_Q_REGISTER")
    if FORBIDDEN.match(mnemonic) or SIMD_SUFFIX.search(mnemonic):
        codes.append("UNSUPPORTED_INSTRUCTION")
    return mnemonic, codes


def inspect_lines(lines, path, limit=LIMIT, counts=None, sampled=None):
    findings, count, extra = [], 0, 0
    counts = counts if counts is not None else Counter()
    sampled = sampled if sampled is not None else Counter()
    for line in lines:
        matched = inspect_instruction(line)
        if matched is None:
            if re.match(r"^\s*[0-9a-f]+:\s+<unknown>", line, re.I):
                matched = ("<unknown>", ["UNKNOWN_DECODE"])
            else:
                continue
        count += 1
        for code in matched[1]:
            counts[code] += 1
            if len(findings) < limit and sampled[code] < SAMPLES_PER_CODE:
                findings.append({"path": path, "code": code,
                                 "instruction": line.strip()[:200]})
                sampled[code] += 1
            else:
                extra += 1
    return count, findings, extra


def audit(root):
    if root.is_symlink() or not root.is_dir():
        raise ValueError("candidate must be a regular directory")
    elf_files = []
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            raise ValueError(f"candidate symlink forbidden: {path}")
        if path.is_file():
            with path.open("rb") as f:
                if f.read(4) == b"\x7fELF":
                    elf_files.append(path)
    findings, instructions, overflow = [], 0, 0
    counts, sampled = Counter(), Counter()
    for file in elf_files:
        rel = file.relative_to(root).as_posix()
        # Stream huge libWPEWebKit output, bound even a stuck decoder.
        with subprocess.Popen(
            ["timeout", "240", "llvm-objdump", "-d", "--no-show-raw-insn", str(file)],
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
        ) as process:
            count, issues, extra = inspect_lines(process.stdout, rel,
                                                 max(0, LIMIT - len(findings)),
                                                 counts=counts, sampled=sampled)
            instructions += count
            findings.extend(issues)
            overflow += extra
            result = process.wait()
            if result:
                findings.append({"path": rel, "code": "DECODER_FAILED", "exit_code": result})
    return {
        "schema": "nuraloumi-armv7-d16-decoded-isa-preflight-v1",
        "qualification": "UNQUALIFIED", "elf_count": len(elf_files),
        "decoded_instructions": instructions, "findings": findings,
        "additional_findings": overflow,
        "findings_by_code": dict(sorted(counts.items())),
        "unknown_decodes": counts["UNKNOWN_DECODE"],
        "incompatible_decoded_uses": sum(n for code, n in counts.items()
                                         if code != "UNKNOWN_DECODE"),
        "limitations": ["dynamic_JIT", "dlopen_completeness",
                        "decoder_mapping_symbols", "sandbox_and_pixels"],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.output.resolve().is_relative_to(args.root.resolve()):
            raise ValueError("report cannot be written inside candidate")
        report = audit(args.root)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    except (ValueError, OSError, subprocess.SubprocessError) as exc:
        parser.exit(2, f"WPE_DECODED_ISA=ERROR: {exc}\n")
    errors = report["unknown_decodes"] + report["incompatible_decoded_uses"] + sum(
        f["code"] == "DECODER_FAILED" for f in report["findings"])
    print(f"WPE_DECODED_ISA={'BLOCKED' if errors else 'PREFLIGHT_PASS'} "
          f"ELF={report['elf_count']} instructions={report['decoded_instructions']} "
          f"ISA_findings={report['incompatible_decoded_uses']} "
          f"unknown_decodes={report['unknown_decodes']} "
          f"decoder_errors={sum(f['code'] == 'DECODER_FAILED' for f in report['findings'])} "
          "runtime_qualification=UNQUALIFIED")
    if errors:
        for finding in report["findings"][:25]:
            print(f"  {finding['code']}: {finding['path']} {finding.get('instruction', '')}")
        parser.exit(1)


if __name__ == "__main__":
    main()
