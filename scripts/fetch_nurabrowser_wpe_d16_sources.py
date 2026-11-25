#!/usr/bin/env python3
"""Fetch source-pinned Alpine 3.22 OpenSSL r2/Nettle/libgcrypt D16 inputs.

Only writes into a *new* output directory. All upstream tarballs and Alpine
patches are checked against independent SHA-512 pins before being retained.
No build, device operation or release qualification occurs here.
"""

import argparse
import hashlib
import json
from pathlib import Path
import re
from urllib.request import Request, urlopen


APORTS_REVISION = "c1a97dc220098b43d75d03ed5776a948908abfaf"
APORTS_RAW = (
    "https://gitlab.alpinelinux.org/alpine/aports/-/raw/" + APORTS_REVISION
)

# (filename, source URL, vendor-published SHA-512 from the pinned APKBUILD)
SOURCES = (
    ("openssl-3.5.9.tar.gz",
     "https://github.com/openssl/openssl/releases/download/openssl-3.5.9/openssl-3.5.9.tar.gz",
     "c4c8136fd2c98d13bb85a57bfb0def25c906f47f1fe97675cd114594944eab957b5b81a3e0f48826e1a0f093afff5e83766b539d49768f6db68310fd678d7de6"),
    ("nettle-3.10.2.tar.gz",
     "https://ftp.gnu.org/gnu/nettle/nettle-3.10.2.tar.gz",
     "bf37ddd7dca8e78488da2a5286dcf16761d527d620572b42f2ad27bb8ee8c12999d92b0272e06f53766e7155a3f4a1ab7ad9c4b1c3caec47c031878b6b1772fb"),
    ("libgcrypt-1.10.3.tar.bz2",
     "https://www.gnupg.org/ftp/gcrypt/libgcrypt/libgcrypt-1.10.3.tar.bz2",
     "8a8d4c61a6622d8481ceb9edc88ec43f58da32e316f79f8d4775325a48f8936aaa9eb355923b39e2c267b784e9c390600daeb62e0c94f00e30bbadb0d8c0865d"),
    ("gcc14-armv7.patch", APORTS_RAW + "/main/libgcrypt/gcc14-armv7.patch",
     "edc0679c0b324f54fadcb7e6e4f902eb2a846aeb886423c586dbcaa90d6bc9164d85d8508df8b888de7893ae5bec0401c4c67d63b345c7370e1c2aec8ddf4a9f"),
    ("auxv.patch", APORTS_RAW + "/main/openssl/auxv.patch",
     "b2541075148fd5af4552d34158deb1a325f5adced90626dc03fd126f47323cde949b15b2657523c34b26530b322312a43bd42ada7295b4f2dd1d3b5d11892c62"),
    ("CVE-2026-88822.patch", APORTS_RAW + "/main/openssl/CVE-2026-88822.patch",
     "d4d7b8e740384cc4b129bbc53eb92c61abebb009b83a7c434ec5d0820021b95d402245a4d3d577ea28e0935d8de96d339eec6a6411ef00f3b9d982ccf0718721"),
    ("CVE-2026-88823.patch", APORTS_RAW + "/main/openssl/CVE-2026-88823.patch",
     "ca38eeb1aa261d1051100b0e37a9699ccad73de551825bf49ade50af9396b1e749983f802fa3967f67ef778590adb3aba39b1fd8290a17aa753fa351953487a9"),
    ("CVE-2026-95691.patch", APORTS_RAW + "/main/openssl/CVE-2026-95691.patch",
     "2c9c869d0427b08c1480307583c1803971fb6223e5876dd20f61fd64102f095373fb32f5cc7bbbd2bf62aca82c229d740a95c39d5556ec72fd41f371e4a311b6"),
    ("CVE-2026-95692.patch", APORTS_RAW + "/main/openssl/CVE-2026-95692.patch",
     "f23fa46c18fca5e475286f4a5fe57834ccca8603a34f54ab0032af16d296f82eee6a9614f70aec4632fc9024f6af9ded9f2698693f02b3ade889c9d58d2cbe59"),
    ("CVE-2026-107496.patch", APORTS_RAW + "/main/openssl/CVE-2026-107496.patch",
     "0bbd1f64e7bc62c8bbba9c85131a51cbfb6f7715156457d575df1e557d649340b8074195860f83be18fe3ed6852111f10053702d72658012f9c3a4c3fd504cea"),
    ("CVE-2026-107493.patch", APORTS_RAW + "/main/openssl/CVE-2026-107493.patch",
     "5d1e0eda23be8661b3e75771f835ea7359dbbcb6ca2341c863577a256668758d90f03b11c50f50c8d3c9c4f282c12f825474d2b4d8c0444f47fcc5f630d1a3e2"),
    ("CVE-2026-124709.patch", APORTS_RAW + "/main/openssl/CVE-2026-124709.patch",
     "cbbf250b02a6799d47a9ffff81b2e3ed8524f298748bf8619893b3f963aa42ceb7a39e787574d0d39dc974037caf7bf2d1c50930251bcb324ec7094fc07e67ee"),
)
HEX512 = re.compile("[0-9a-f]{128}\\Z")
MAX_SOURCE_BYTES = 80 * 1024 * 1024


def fetch_one(destination, url, expected_hash, opener=urlopen):
    if not HEX512.fullmatch(expected_hash):
        raise ValueError("invalid source SHA-512 pin")
    digest = hashlib.sha512()
    consumed = 0
    request = Request(url, headers={"User-Agent": "NuraLoumi-source-pin-check/1"})
    with opener(request, timeout=90) as stream, destination.open("xb") as output:
        while True:
            block = stream.read(1024 * 1024)
            if not block:
                break
            consumed += len(block)
            if consumed > MAX_SOURCE_BYTES:
                raise ValueError("source exceeds size cap")
            digest.update(block)
            output.write(block)
    if digest.hexdigest() != expected_hash:
        raise ValueError(f"source SHA-512 mismatch: {destination.name}")
    return consumed


def fetch_sources(output, sources=SOURCES, opener=urlopen):
    if output.exists() or output.is_symlink():
        raise ValueError("output directory must be fresh")
    output.mkdir(mode=0o700)
    manifest = []
    for name, url, expected in sources:
        if not name or Path(name).name != name:
            raise ValueError("unsafe source filename")
        destination = output / name
        try:
            size = fetch_one(destination, url, expected, opener)
        except Exception:
            destination.unlink(missing_ok=True)
            raise
        manifest.append({"name": name, "url": url, "sha512": expected, "bytes": size})
    evidence = {
        "schema": "nuraloumi-d16-source-inputs-v1",
        "alpine_aports_revision": APORTS_REVISION,
        "openssl_alpine_release": "3.5.9-r2",
        "qualification": "SOURCE_INPUTS_ONLY",
        "sources": manifest,
    }
    (output / "SOURCE-MANIFEST.json").write_text(
        json.dumps(evidence, indent=2, sort_keys=True) + "\n")
    return evidence


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="new directory for verified sources")
    args = parser.parse_args()
    try:
        evidence = fetch_sources(args.output)
    except Exception as exc:
        parser.exit(1, f"D16_SOURCES=FAIL: {exc}\n")
    print(f"D16_SOURCES=VERIFIED files={len(evidence['sources'])} "
          "release=3.5.9-r2 qualification=SOURCE_INPUTS_ONLY")


if __name__ == "__main__":
    main()
