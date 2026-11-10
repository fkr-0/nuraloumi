"""Fail closed when either native-package CI lane loses provenance or ABI gates."""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]
MUSL = (ROOT / ".github/workflows/sl101-musl-armv7.yml").read_text()
DEB = (ROOT / ".github/workflows/sl101-debian-armhf.yml").read_text()
ALPINE = (ROOT / "packaging/alpine/APKBUILD.in").read_text()
APK_SCRIPT = (ROOT / "scripts/build-ci-alpine-apk.sh").read_text()
DEB_SCRIPT = (ROOT / "scripts/build-ci-debian-deb.sh").read_text()


class NativePackageCiContract(unittest.TestCase):
    def test_native_packaging_requires_prior_binary_audits(self):
        for ci, native in (
            (MUSL, "sh /work/scripts/build-ci-alpine-apk.sh"),
            (DEB, "sh scripts/build-ci-debian-deb.sh"),
        ):
            self.assertLess(ci.index("cargo xtask package-check"), ci.index("ELF_AUDIT=PASS"))
            self.assertLess(ci.index("ELF_AUDIT=PASS"), ci.index(native))
            self.assertLess(ci.index(native), ci.index("actions/upload-artifact@v4"))
            for gate in ("git_dirty=false", "MANIFEST.txt", "ELF-AUDIT.txt", "github.sha"):
                self.assertIn(gate, ci)
            for forbidden in ("ssh ", "scp ", "192.168.", "reboot", "systemctl "):
                self.assertNotIn(forbidden, ci)

    def test_apk_is_native_armv7_and_signed_but_never_auto_trusted(self):
        self.assertIn("docker run --rm --platform linux/arm/v7", MUSL)
        self.assertIn("alpine:3.22", MUSL)
        self.assertIn("unmapped Alpine runtime dependency", MUSL)
        for token in (
            'arch="armv7"', 'options="!check !strip !tracedeps"',
            'depends="musl cairo libgcc libxkbcommon wayland-libs-client"',
            'source="nuraloumi-stage.tar.gz"', "install -m755",
        ):
            self.assertIn(token, ALPINE)
        for token in (
            "abuild-keygen -a -n", "abuild -F checksum", "abuild -F -d",
            "CI-EPHEMERAL-KEY.rsa.pub", "apk --keys-dir /root/.abuild verify",
            "SIGNING_MODE=$signing_mode",
            "signing_mode=ephemeral-ci-development-only",
            'git_commit=$expected_sha', "git_dirty=false",
        ):
            self.assertIn(token, APK_SCRIPT)
        self.assertNotIn("apk add --allow-untrusted", APK_SCRIPT + MUSL)
        self.assertIn('install -m0644 "$key" "/etc/apk/keys/', APK_SCRIPT)
        self.assertIn("The physical devices never receive this key automatically", APK_SCRIPT)
        self.assertIn("NURALOUMI_APK_RELEASE_PUBLIC_SHA256", APK_SCRIPT)
        # Current main workflow is development-only, never mounts production keys.
        self.assertNotIn("NURALOUMI_APK_RELEASE_KEYS_DIR", MUSL)
        self.assertNotIn("+            -v", MUSL)
        self.assertNotIn("+                /work", MUSL)

    def test_deb_has_armhf_control_and_no_maintainer_scripts(self):
        for token in (
            "Architecture: armhf", "dpkg-deb --root-owner-group --build",
            "cmp \"$executable\"", "CI-MANIFEST.txt",
            "libwayland-client0", "libxkbcommon0", "libgcc-s1", "git_commit=$expected_sha",
            "git_dirty=false",
        ):
            self.assertIn(token, DEB_SCRIPT)
        self.assertNotIn("DEBIAN/postinst", DEB_SCRIPT)
        self.assertNotIn("DEBIAN/prerm", DEB_SCRIPT)
        self.assertIn("armv7-unknown-linux-gnueabihf", DEB_SCRIPT)
        self.assertIn("unmapped Debian dependency", DEB)
        self.assertIn("armv7-unknown-linux-musleabihf", APK_SCRIPT)
        self.assertRegex(DEB_SCRIPT, r"sed 's/-dev\\\./~dev\./'")

    def test_no_flatpak_or_apk_cross_abi_masquerade(self):
        self.assertNotIn("flatpak", MUSL + DEB)
        self.assertNotIn("armv7-unknown-linux-gnueabihf", MUSL)
        self.assertNotIn("armv7-unknown-linux-musleabihf", DEB)
        self.assertIn("retention-days: 14", MUSL)
        self.assertIn("retention-days: 14", DEB)


if __name__ == "__main__":
    unittest.main()
