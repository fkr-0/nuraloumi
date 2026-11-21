"""Fail closed when either native-package CI lane loses provenance or ABI gates."""
from pathlib import Path
import re
import shutil
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]
MUSL = (ROOT / ".github/workflows/sl101-musl-armv7.yml").read_text()
DEB = (ROOT / ".github/workflows/sl101-debian-armhf.yml").read_text()
ALPINE = (ROOT / "packaging/alpine/APKBUILD.in").read_text()
APK_SCRIPT = (ROOT / "scripts/build-ci-alpine-apk.sh").read_text()
DEB_SCRIPT = (ROOT / "scripts/build-ci-debian-deb.sh").read_text()


class NativePackageCiContract(unittest.TestCase):
    def test_native_packages_ship_qol_launchers_and_user_bus_activation(self):
        # Do not let an audited staging tar conceal missing native-package
        # integration: both package constructors must install all launchers.
        for script, label in ((ALPINE, "apk"), (DEB_SCRIPT, "deb")):
            with self.subTest(package=label):
                self.assertTrue(
                    "share/dbus-1/services/org.freedesktop.Notifications.service" in script,
                    "user-bus activation missing from native package",
                )
                self.assertIn("share/applications", script)
                self.assertTrue(
                    ("*.desktop" in script)
                    or all(desktop in script for desktop in (
                        "nuraloumi-files.desktop",
                        "nuraloumi-notifications.desktop",
                    )),
                    "QoL launchers omitted from native package")

    def test_debian_desktop_and_notification_activation_payload_verified(self):
        # Verify the extracted package matches audited staging, not only the tar.
        for required in (
            "nuraloumi-player.desktop",
            "nuraloumi-files.desktop",
            "nuraloumi-notifications.desktop",
        ):
            self.assertIn(required, DEB_SCRIPT)
        for token in (
            '"$stage"/share/applications/*.desktop',
            'test ! -L "$desktop"',
            'test -x "$stage/bin/nuraloumi-notificationd"',
            'Exec=/usr/bin/nuraloumi-notificationd',
            'cmp "$desktop" "$tmp/extracted/usr/share/applications/${desktop##*/}"',
            'cmp "$service" "$tmp/extracted/usr/share/dbus-1/services/org.freedesktop.Notifications.service"',
        ):
            self.assertIn(token, DEB_SCRIPT)

    def test_alpine_prerelease_versions_use_abuild_valid_suffixes(self):
        # Exercise the exact sed conversion used by the CI packaging script,
        # not a separately reimplemented Python version mapper.
        matched = re.search(
            r"^apkver=\$\(printf '%s' \"\$version\" \| sed '([^']+)'\)$",
            APK_SCRIPT, re.MULTILINE,
        )
        self.assertIsNotNone(matched, "APK version conversion must remain testable")
        sed_expression = matched.group(1)
        for source, expected in (
            ("0.0.4-dev.2", "0.0.4_pre2"),
            ("0.0.4-rc.1", "0.0.4_rc1"),
            ("0.0.4-rc.12", "0.0.4_rc12"),
            ("0.0.4", "0.0.4"),
        ):
            with self.subTest(source=source):
                result = subprocess.run(
                    ["sed", sed_expression], input=source + "\n", text=True,
                    check=True, capture_output=True,
                )
                self.assertEqual(result.stdout.strip(), expected)
                self.assertNotIn("-", result.stdout)

        self.assertIn(
            r"grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-(dev|rc)\.[0-9]+)?$'",
            APK_SCRIPT,
        )
        allow = re.search(r"grep -Eq '([^']+)'", APK_SCRIPT)
        self.assertIsNotNone(allow)
        for source in (
            "0.0.4-rc.x", "0.0.4-dev.bad", "0.0.4-beta.1",
            "0.0.4-rc.1-extra", "0.0.4-pre", "", "../0.0.4",
        ):
            with self.subTest(reject=source):
                result = subprocess.run(
                    ["grep", "-Eq", allow.group(1)],
                    input=source + "\n", text=True, capture_output=True,
                )
                self.assertNotEqual(result.returncode, 0)

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

    def test_combined_gpu_package_contains_same_wpe_runtime_and_signed_apk(self):
        gpu_step = MUSL.split("- name: Package and audit GPU-video variant", 1)[1]
        self.assertIn("NURALOUMI_BROWSER_RUNTIME_DIR", gpu_step)
        self.assertIn("NURALOUMI_BROWSER_PRODUCER_REVISION", gpu_step)
        self.assertIn("NURALOUMI_VIDEO_BUNDLE_DIR", gpu_step)
        self.assertIn("NURALOUMI_VIDEO_BUNDLE_SOURCE_VERIFIED", gpu_step)
        self.assertLess(gpu_step.index("cargo xtask package --target"), gpu_step.index("verify_packaged_sl101_video.py"))
        self.assertIn("/output/native-gpu-apk", gpu_step)
        self.assertIn("native-gpu-apk/**", gpu_step)
        self.assertIn("GPU-SHA256SUMS.txt", gpu_step)

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
            "public_key_filename=${key##*/}",
            'install -m0644 "$key" "$out/$public_key_filename"',
            'apk --keys-dir "$exported_keys" verify "$pkg"',
            "PUBLIC_KEY_FILE=$public_key_filename",
            "SIGNING_MODE=$signing_mode",
            "signing_mode=ephemeral-ci-development-only",
            'git_commit=$expected_sha', "git_dirty=false",
        ):
            self.assertIn(token, APK_SCRIPT)
        self.assertNotIn("apk add --allow-untrusted", APK_SCRIPT + MUSL)
        self.assertIn('if [ "$signing_mode" = "ephemeral-ci-development-only" ]; then', APK_SCRIPT)
        self.assertIn('install -m0644 "$out/$public_key_filename" "$exported_keys/$public_key_filename"', APK_SCRIPT)
        self.assertIn('install -m0644 "$key" "/etc/apk/keys/', APK_SCRIPT)
        self.assertIn("The physical devices never receive this key automatically", APK_SCRIPT)
        self.assertIn("NURALOUMI_APK_RELEASE_PUBLIC_SHA256", APK_SCRIPT)
        # Current main workflow is development-only, never mounts production keys.
        self.assertNotIn("NURALOUMI_APK_RELEASE_KEYS_DIR", MUSL)
        self.assertNotIn("+            -v", MUSL)
        self.assertNotIn("+                /work", MUSL)

    def test_deb_has_armhf_control_and_no_maintainer_scripts(self):
        self.assertIn(
            'sh scripts/build-ci-debian-deb.sh \\\n            target/gha-artifact/nuraloumi-ci',
            DEB,
        )
        self.assertNotIn("build-ci-debian-deb.sh +", DEB)
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
        self.assertIn(r"s/-dev\./~dev./;s/-rc\./~rc./", DEB_SCRIPT)

    def test_debian_rc_package_versions_sort_before_stable(self):
        conversion = re.search(
            r"^deb_version=\$\(printf '%s' \"\$version\" \| sed '([^']+)'\)$",
            DEB_SCRIPT, re.MULTILINE,
        )
        self.assertIsNotNone(conversion)
        for source, expected in (
            ("0.0.8-dev.1", "0.0.8~dev.1"),
            ("0.0.8-rc.1", "0.0.8~rc.1"),
            ("0.0.8", "0.0.8"),
        ):
            with self.subTest(source=source):
                result = subprocess.run(
                    ["sed", conversion.group(1)], input=source + "\n",
                    text=True, check=True, capture_output=True,
                )
                self.assertEqual(result.stdout.strip(), expected)
        for earlier, later in (
            ("0.0.8~dev.1", "0.0.8~rc.1"),
            ("0.0.8~rc.1", "0.0.8"),
        ):
            # The development host may be non-Debian; the CI runner has dpkg.
            if shutil.which("dpkg") is None:
                break
            result = subprocess.run(
                ["dpkg", "--compare-versions", earlier, "lt", later],
                check=False, timeout=5,
            )
            self.assertEqual(result.returncode, 0)

    def test_no_flatpak_or_apk_cross_abi_masquerade(self):
        self.assertNotIn("flatpak", MUSL + DEB)
        self.assertNotIn("armv7-unknown-linux-gnueabihf", MUSL)
        self.assertNotIn("armv7-unknown-linux-musleabihf", DEB)
        self.assertIn("retention-days: 14", MUSL)
        self.assertIn("retention-days: 14", DEB)


if __name__ == "__main__":
    unittest.main()
