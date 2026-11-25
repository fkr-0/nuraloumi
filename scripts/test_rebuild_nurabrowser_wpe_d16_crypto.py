"""Host-only checks for reproducible D16 crypto source and feature gates."""

from pathlib import Path
import unittest


SCRIPT = Path(__file__).with_name("rebuild_nurabrowser_wpe_d16_crypto.sh")


class CryptoBuildContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.text = SCRIPT.read_text()

    def test_pins_complete_source_and_security_patch_set(self):
        for name in ("openssl-3.5.9.tar.gz", "nettle-3.10.2.tar.gz",
                     "libgcrypt-1.10.3.tar.bz2", "gcc14-armv7.patch",
                     "auxv.patch", "CVE-2026-88822.patch",
                     "CVE-2026-88823.patch", "CVE-2026-95691.patch",
                     "CVE-2026-95692.patch", "CVE-2026-107496.patch",
                     "CVE-2026-107493.patch", "CVE-2026-124709.patch"):
            with self.subTest(name=name):
                self.assertIn("check_source " + name + " ", self.text)

    def test_all_three_assembler_disables_and_d16_flags(self):
        for token in ("-mcpu=cortex-a9", "-mfpu=vfpv3-d16",
                      "--disable-fat", "--disable-assembler",
                      "--disable-arm-neon", "--disable-asm",
                      "--disable-neon-support", "--disable-arm-crypto-support",
                      "no-asm"):
            with self.subTest(token=token):
                self.assertIn(token, self.text)

    def test_no_live_device_install_or_prequalified_manifest(self):
        self.assertIn('test ! -e "$out/work"', self.text)
        self.assertIn("normal_deployment=FORBIDDEN", self.text)
        self.assertNotIn("runtime.json", self.text)
        self.assertNotIn("ssh ", self.text)
        self.assertNotIn("sudo ", self.text)

    def test_source_manifest_and_toolchain_are_recorded(self):
        self.assertIn('"$out/SOURCE-MANIFEST.json"', self.text)
        self.assertIn('"$out/BUILD-TOOLCHAIN.txt"', self.text)
        self.assertIn("openssl_alpine_release=3.5.9-r2", self.text)
        self.assertIn("source_manifest_sha256=", self.text)

    def test_crypto_abi_pairs_remain_together(self):
        for soname in ("libnettle.so.8", "libhogweed.so.6",
                       "libgcrypt.so.20", "libcrypto.so.3", "libssl.so.3"):
            self.assertIn("copy_soname stage-", self.text)
            self.assertIn(soname, self.text)

    def test_legacy_module_and_hardfloat_attributes_are_gated(self):
        self.assertIn('test -f "$out/replacements/ossl-modules/legacy.so"',
                      self.text)
        self.assertIn('"$out/replacements/ossl-modules/"*.so', self.text)
        self.assertIn("Tag_ABI_VFP_args: VFP registers", self.text)
        self.assertIn("Tag_FP_arch: VFPv3-D16", self.text)


if __name__ == "__main__":
    unittest.main()
