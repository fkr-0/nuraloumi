#!/usr/bin/env python3
"""Fail-closed acceptance of actual browser HLS frames plus GstGL evidence.

Inputs are collected by the private SL101 qualifier, not synthesized here.
Neither a page load nor a software texture upload proves GPU decode.
"""
import argparse
import re
import sys
from pathlib import Path
from urllib.parse import parse_qs, urlsplit


def heartbeats(text):
    samples = []
    for line in text.splitlines():
        match = re.search(r"/heartbeat\?([^\s\"']+)", line)
        if not match:
            continue
        raw = parse_qs(urlsplit("http://localhost/heartbeat?" + match.group(1)).query)
        try:
            sample = {name: float(raw[name][0]) for name in
                      ("wall", "time", "frames", "presented", "mediaTime",
                       "total", "width", "height", "ready")}
        except (KeyError, IndexError, ValueError):
            continue
        samples.append(sample)
    return samples


def verify(heartbeat_log, gst_log, *, min_seconds=10):
    if re.search(r"(?:context lost|egl_bad_context|gst_gl_context.*failed|"
                 r"not-negotiated|GST_FLOW_NOT_NEGOTIATED)", gst_log, re.I):
        return False, "GL context loss or caps negotiation failure"
    if re.search(r"(?:[?&]error=)(?!none(?:&|\s|$))", heartbeat_log, re.I):
        return False, "browser reported media playback error"
    samples = heartbeats(heartbeat_log)
    if len(samples) < 2:
        return False, "missing consecutive complete browser heartbeats"
    first, last = samples[0], samples[-1]
    if last["wall"] - first["wall"] < min_seconds:
        return False, "insufficient observation duration"
    if last["width"] <= 0 or last["height"] <= 0 or last["ready"] < 2:
        return False, "video never reached decoded dimensions/current data"
    advanced = (last["presented"] > first["presented"] and
                last["mediaTime"] > first["mediaTime"] and
                last["time"] > first["time"])
    if not advanced:
        return False, "no advancing presented browser frames/media clock"
    if last["total"] >= 0 and first["total"] >= 0 and last["total"] <= first["total"]:
        return False, "browser playback quality reports no new frames"
    # A real GST_DEBUG log must identify caps and the actual buffer memory.
    # GLMemory is mandatory here; DMABuf alone could be mapped by a CPU sink.
    gl_caps = re.search(r"video/x-raw\(memory:GLMemory\)", gst_log)
    gl_buffer = re.search(r"(?:memory:GLMemory|GstGLMemory).*?(?:buffer|render|present)|"
                          r"(?:buffer|render|present).*?(?:memory:GLMemory|GstGLMemory)",
                          gst_log, re.I)
    context = re.search(r"(?:GstGLContext|gst_gl_context).*?(?:EGL|egl|activated)|"
                        r"(?:EGL|egl).*?(?:GstGLContext|gst_gl_context)", gst_log)
    if not (gl_caps and gl_buffer and context):
        return False, "missing EGL-context/GLMemory caps and buffer evidence"
    if re.search(r"LIBGL_ALWAYS_SOFTWARE=1|GALLIUM_DRIVER=softpipe|llvmpipe", gst_log):
        return False, "software GL renderer detected"
    return True, "advancing browser frames with EGL and GLMemory buffer evidence"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("heartbeat_log", type=Path)
    parser.add_argument("gst_debug_log", type=Path)
    parser.add_argument("--min-seconds", type=int, default=10)
    args = parser.parse_args()
    passed, reason = verify(args.heartbeat_log.read_text(errors="replace"),
                            args.gst_debug_log.read_text(errors="replace"),
                            min_seconds=args.min_seconds)
    print(f"{'PASS' if passed else 'FAIL'} {reason}")
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
