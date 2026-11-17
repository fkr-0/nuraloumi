#!/usr/bin/env python3
"""Pure JSON geometry/semantic audit. No screenshots, compositor or device access.

Usage: python3 scripts/qol_ux_audit.py tests/fixtures/qol/scenes.json
Input is an explicitly exported or synthetic scene fixture, NOT optical evidence.
"""
from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path


def _rect(rect: object, label: str) -> tuple[float, float, float, float]:
    if not isinstance(rect, list) or len(rect) != 4:
        raise ValueError(f"{label}: expected [x,y,width,height]")
    vals = tuple(float(v) for v in rect)
    if not all(math.isfinite(v) for v in vals) or vals[2] < 0 or vals[3] < 0:
        raise ValueError(f"{label}: invalid rectangle")
    return vals


def _overlap(a: tuple[float, ...], b: tuple[float, ...]) -> bool:
    return min(a[0] + a[2], b[0] + b[2]) > max(a[0], b[0]) and min(
        a[1] + a[3], b[1] + b[3]
    ) > max(a[1], b[1])


def audit(scene: dict) -> list[str]:
    """Return stable defect codes; all dimensions are logical pixels."""
    errors: list[str] = []
    def fail(code: str, condition: bool) -> None:
        if condition:
            errors.append(code)

    vw, vh = map(float, scene["viewport"])
    scale = float(scene.get("scale", 1))
    if min(vw, vh, scale) <= 0 or not all(math.isfinite(x) for x in (vw, vh, scale)):
        raise ValueError("viewport and scale must be positive finite values")
    panel = _rect(scene["panel"], "panel")
    menu = _rect(scene["menu"], "menu")
    fail("menu-gap", abs(menu[1] - (panel[1] + panel[3])) > 1.0)
    fail("menu-offscreen", menu[0] < -0.01 or menu[0] + menu[2] > vw + 0.01
         or menu[1] + menu[3] > vh + 0.01)
    fail("phantom-glyph", bool(scene.get("phantom_glyphs")))
    fail("thick-title", float(scene.get("title_height", 0)) > 48)
    labels = scene.get("labels", [])
    fail("duplicate-label", len(labels) != len(set(labels)))
    fail("tabs", len(scene.get("tabs", [])) > 1 and (
        len([x for x in scene["tabs"] if x.get("selected")]) != 1
        or any(not x.get("label", "").strip() for x in scene["tabs"])))
    fail("outside-dismiss", not scene.get("outside_dismiss", False))
    controls = scene.get("controls", [])
    rows = scene.get("item_rows", [])
    for c in controls:
        rect = _rect(c["hit"], f"control {c['id']}")
        fail(f"tap-target:{c['id']}", rect[2] + 1e-6 < 48 or rect[3] + 1e-6 < 48)
        fail(f"toolbar-overlap:{c['id']}", any(
            _overlap(rect, _rect(r, "item row")) for r in rows))
    for i, a in enumerate(controls):
        for b in controls[i + 1:]:
            fail(f"hit-overlap:{a['id']}:{b['id']}",
                 _overlap(_rect(a["hit"], a["id"]), _rect(b["hit"], b["id"])))
    status = scene.get("status_fields", [])
    fail("status-fields", len(status) != 3)
    if len(status) == 3:
        boxes = [_rect(x, "status field") for x in status]
        fail("status-alignment", max(x[1] for x in boxes) - min(x[1] for x in boxes) > 1)
        fail("status-overlap", any(_overlap(a, b)
             for i, a in enumerate(boxes) for b in boxes[i + 1:]))
    scroll = scene.get("scroll")
    if scroll:
        content, viewport = float(scroll["content"]), float(scroll["viewport"])
        visible = bool(scroll["visible"])
        needed = content > viewport + 1e-6
        fail("scroll-visibility", visible != needed)
        if needed:
            fail("scroll-drag", not scroll.get("draggable", False))
            fail("scroll-track", float(scroll.get("track_hit_width", 0)) < 48)
            fail("scroll-range", float(scroll.get("thumb_length", 0)) <= 0
                 or float(scroll.get("thumb_length", 0)) >= float(scroll.get("track_length", 0)))
    return sorted(set(errors))


def inspect(document: dict) -> dict:
    result = []
    for scene in document["scenes"]:
        actual = audit(scene)
        expected = scene.get("expected_failures", [])
        result.append({"name": scene["name"], "actual": actual, "expected": expected,
                       "matched": actual == sorted(set(expected))})
    return {"scenes": result, "passed": all(x["matched"] for x in result)}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("fixture", type=Path)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    result = inspect(json.loads(args.fixture.read_text(encoding="utf-8")))
    if args.json:
        print(json.dumps(result, indent=2))
    else:
        for case in result["scenes"]:
            print(f"{'PASS' if case['matched'] else 'FAIL'} {case['name']}: {','.join(case['actual']) or 'clean'}")
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
