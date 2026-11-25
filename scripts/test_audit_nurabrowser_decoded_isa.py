"""Targeted regression checks for decoded ISA streaming guard."""
import unittest
from collections import Counter

from scripts.audit_nurabrowser_decoded_isa import (
    SAMPLES_PER_CODE, inspect_instruction, inspect_lines,
)


class DecodedISATests(unittest.TestCase):
    def test_vfpv3_d16_is_permitted(self):
        for line in ("  80f0: vadd.f64 d0, d12, d15",
                     "  80f4: str r2, [sp, #4]",
                     "  80f8: vmov s30, r0",
                     "  80fc: bl 0x1234 @ <vdup_helper>"):
            self.assertEqual(inspect_instruction(line)[1], [])

    def test_upper_register_and_neon_are_rejected(self):
        for line, code in (
            ("  80f0: vadd.f64 d16, d2, d3", "UPPER_VFP_REGISTER"),
            ("  80f4: vmov s63, r0", "UPPER_VFP_REGISTER"),
            ("  80f8: vorr q3, q2, q1", "NEON_Q_REGISTER"),
        ):
            self.assertIn(code, inspect_instruction(line)[1])

    def test_illegal_mnemonics(self):
        for mnemonic in ("sdiv", "udiv", "udivne", "vld1.8", "vdup.32", "vadd.i8",
                         "vorr", "vorrne", "vmax.f32", "vcgt.f32",
                         "vqadd.s16", "vfma.f64", "aese.8", "crc32b"):
            self.assertIn("UNSUPPORTED_INSTRUCTION",
                          inspect_instruction(f"  f00: {mnemonic} d0, d1, d2")[1])

    def test_ignore_symbol_and_comment_names(self):
        lines = ["0000 <my_neon_q16>", "Disassembly of section .text:",
                 "  a00: add r0, r0, r1 @ d19 q2", "  a04: vld1.8 {d0}, [r1]"]
        count, findings, extra = inspect_lines(lines, "lib/foo.so")
        self.assertEqual((count, extra), (2, 0))
        self.assertEqual([x["code"] for x in findings], ["UNSUPPORTED_INSTRUCTION"])

    def test_unknown_decode_is_separately_counted_and_sampled(self):
        counts, samples = Counter(), Counter()
        lines = ["  ff0: <unknown>"] * 100 + ["  ffc: sdiv r0, r1, r2"]
        total, findings, omitted = inspect_lines(
            lines, "bin/bwrap", counts=counts, sampled=samples)
        self.assertEqual(total, 101)
        self.assertEqual(counts["UNKNOWN_DECODE"], 100)
        self.assertEqual(counts["UNSUPPORTED_INSTRUCTION"], 1)
        self.assertEqual(len(findings), SAMPLES_PER_CODE + 1)
        self.assertEqual(omitted, 100 - SAMPLES_PER_CODE)
