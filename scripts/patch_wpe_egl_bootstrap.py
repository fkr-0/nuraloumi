#!/usr/bin/env python3
"""Guarded WPE 2.54.2 EGL bootstrap patch for the ARMv7 media producer.

Run only against the SHA-pinned, extracted original source. A failed
surfaceless eglInitialize must not abort before another platform can be tried.
"""

import argparse
from pathlib import Path


SOURCE = Path("Source/WebCore/platform/graphics/egl/PlatformDisplaySurfaceless.cpp")
OLD = '''    if (!glDisplay) {
        WTFLogAlways("Could not create surfaceless EGL display: %s. Aborting...", GLContext::lastErrorString());
        CRASH();
    }
'''
NEW = '''    if (!glDisplay) {
        WTFLogAlways("Surfaceless EGL display initialization failed: %s; returning to platform selection", GLContext::lastErrorString());
        return nullptr;
    }
'''


def apply(root: Path) -> bool:
    path = root / SOURCE
    content = path.read_text()
    if NEW in content:
        if OLD in content:
            raise ValueError("mixed EGL bootstrap patch state")
        return False
    if content.count(OLD) != 1:
        raise ValueError("unexpected WPE 2.54.2 surfaceless EGL source")
    path.write_text(content.replace(OLD, NEW, 1))
    return True


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("source_root", type=Path)
    args = parser.parse_args()
    print("WPE_EGL_PATCH=" + ("APPLIED" if apply(args.source_root) else "ALREADY_APPLIED"))
