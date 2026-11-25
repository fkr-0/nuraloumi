#!/usr/bin/env python3
"""Offline glyph coverage gate for NuraLoumi's exact glyph table and font file."""
import argparse
import hashlib
import re
from pathlib import Path

ROW = re.compile(r'^\s*\("([^"]+)",\s*\'([^\']+)\',\s*\'([^\']+)\'\),', re.MULTILINE)


def table_rows(source: Path):
    rows = ROW.findall(source.read_text(encoding="utf-8"))
    if len(rows) < 50 or len({row[0] for row in rows}) != len(rows):
        raise ValueError("unexpected or duplicate glyph table entries")
    return rows


def codepoints(font: Path):
    from fontTools.ttLib import TTFont
    face = TTFont(font, lazy=True)
    try:
        return set().union(*(set(t.cmap) for t in face["cmap"].tables if t.isUnicode()))
    finally:
        face.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", default="crates/nuraloumi-shell/src/glyphs.rs", type=Path)
    parser.add_argument("--font", type=Path, required=True)
    parser.add_argument("--sha256", help="Expected source-pinned file checksum")
    parser.add_argument("--require-complete", action="store_true")
    args = parser.parse_args()
    rows = table_rows(args.source)
    font_hash = hashlib.sha256(args.font.read_bytes()).hexdigest()
    if args.sha256 and font_hash.lower() != args.sha256.lower():
        parser.error("font SHA256 does not match pinned input")
    cmap = codepoints(args.font)
    nerd = sorted({ord(row[1]) for row in rows})
    unicode = sorted({ord(row[2]) for row in rows})
    missing = sorted(set(nerd) - cmap)
    fallback_missing = sorted(set(unicode) - cmap)
    print(f"font={args.font} bytes={args.font.stat().st_size} sha256={font_hash}")
    print(f"rows={len(rows)} nerd_unique={len(nerd)} nerd_present={len(nerd)-len(missing)} nerd_missing={len(missing)}")
    print(f"fallback_unique={len(unicode)} fallback_present={len(unicode)-len(fallback_missing)} fallback_missing={len(fallback_missing)}")
    print("nerd_missing_codepoints=" + ",".join(f"U+{v:04X}" for v in missing))
    print("fallback_missing_codepoints=" + ",".join(f"U+{v:04X}" for v in fallback_missing))
    print("missing_rows=" + ",".join(row[0] for row in rows if ord(row[1]) in missing))
    if args.require_complete and missing:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
